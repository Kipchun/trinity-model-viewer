//! Reads files out of the game's packed file system (arc/data.trpfd + arc/data.trpfs in the RomFS). The RomFS can be
//! a dumped folder (the usual emulator "dump RomFS" result) or a single image file with a JSON index (see `open`).
//!   data.trpfd  root { file_hashes: [u64], pack_names: [string], file_info: [{pack_index: u64}],
//!               pack_info: [{size: u64, file_count: u64}] }
//!   data.trpfs  "ONEPACK\0", u64 offset of root { pack_hashes: [u64], pack_offsets: [u64] }
//!   pack        root { file_hashes: [u64], files: [{?, compression: i8 (-1 none), level: u8, size: u64, data: [u8]}] }
//! Files are known by the FNV-1a 64 hash of their path; most are Oodle Kraken compressed.

use crate::fb::{Table, u64_at};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Where the two archive files are.
enum Layout {
    /// arc/data.trpfd and arc/data.trpfs as files (a dumped RomFS folder).
    Folder { trpfd: PathBuf, trpfs: PathBuf },
    /// One RomFS image with an index.json listing {path, offset, size} per file.
    Image { image: PathBuf, index: PathBuf },
}

/// Finds the archive from whatever the user picked: the RomFS folder, its `arc` folder, a folder that holds a
/// `romfs` folder, data.trpfs / data.trpfd themselves, or a folder with romfs.bin + index.json.
fn locate(picked: &Path) -> Result<Layout> {
    let dir = if picked.is_file() {
        picked.parent().unwrap_or(picked).to_path_buf()
    } else {
        picked.to_path_buf()
    };
    for arc in [dir.join("arc"), dir.clone(), dir.join("romfs").join("arc")] {
        let (trpfd, trpfs) = (arc.join("data.trpfd"), arc.join("data.trpfs"));
        if trpfd.is_file() && trpfs.is_file() {
            return Ok(Layout::Folder { trpfd, trpfs });
        }
    }
    let (image, index) = (dir.join("romfs.bin"), dir.join("index.json"));
    if image.is_file() && index.is_file() {
        return Ok(Layout::Image { image, index });
    }
    bail!(
        "{} does not contain the game's archive. Pick the dumped RomFS folder: the one with an \"arc\" folder \
         holding data.trpfd and data.trpfs.",
        picked.display()
    )
}

/// A file of the RomFS outside the Trinity archive (e.g. `audio/wwise/bin/Switch/Stream.pck`), opened read-only:
/// in a dumped folder it is a file of its own; in an image it is a byte range of `romfs.bin` (from `index.json`).
pub struct RomFile {
    file: File,
    base: u64,
    pub size: u64,
}

impl RomFile {
    /// Opens `rel` (a path from the RomFS root) of the RomFS in or near `picked` (see `locate`).
    pub fn open(picked: &Path, rel: &str) -> Result<RomFile> {
        match locate(picked)? {
            Layout::Folder { trpfd, .. } => {
                let root = trpfd
                    .parent()
                    .and_then(Path::parent)
                    .ok_or_else(|| anyhow!("no RomFS root above {}", trpfd.display()))?;
                let path = root.join(rel);
                let file =
                    File::open(&path).with_context(|| format!("opening {}", path.display()))?;
                let size = file.metadata()?.len();
                Ok(RomFile {
                    file,
                    base: 0,
                    size,
                })
            }
            Layout::Image { image, index } => {
                let index: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&index)
                        .with_context(|| format!("reading {}", index.display()))?,
                )?;
                let e = index["files"]
                    .as_array()
                    .and_then(|v| v.iter().find(|e| e["path"] == rel))
                    .ok_or_else(|| anyhow!("{rel} is not in index.json"))?;
                let file =
                    File::open(&image).with_context(|| format!("opening {}", image.display()))?;
                Ok(RomFile {
                    file,
                    base: e["offset"].as_u64().unwrap_or(0),
                    size: e["size"].as_u64().unwrap_or(0),
                })
            }
        }
    }

    /// `n` bytes at `off`, cut short at the end of the file.
    pub fn read_at(&mut self, off: u64, n: usize) -> Result<Vec<u8>> {
        if off > self.size {
            bail!("offset {off} past the end ({} bytes)", self.size);
        }
        let n = n.min((self.size - off) as usize);
        read_at(&mut self.file, self.base + off, n)
    }
}

pub fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xCBF29CE484222645;
    for b in s.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001B3);
    }
    h
}

pub struct Archive {
    img: File,
    fs_off: u64,
    file_pack: HashMap<u64, u32>,
    pub pack_names: Vec<String>,
    pack_size: Vec<u64>,
    pack_offset: HashMap<u64, u64>,
    pack_cache: HashMap<u32, Vec<u8>>,
}

impl Archive {
    /// Opens the archive in or near `picked` (see `locate`).
    pub fn open(picked: &Path) -> Result<Self> {
        // `img` is the file data.trpfs lives in, `fs_off`/`fs_size` where it is in that file.
        let (fd_buf, mut img, fs_off, fs_size) = match locate(picked)? {
            Layout::Folder { trpfd, trpfs } => {
                let fd = std::fs::read(&trpfd)
                    .with_context(|| format!("reading {}", trpfd.display()))?;
                let img =
                    File::open(&trpfs).with_context(|| format!("opening {}", trpfs.display()))?;
                let size = img.metadata()?.len();
                (fd, img, 0, size)
            }
            Layout::Image { image, index } => {
                let index: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&index)
                        .with_context(|| format!("reading {}", index.display()))?,
                )?;
                let find = |path: &str| -> Result<(u64, u64)> {
                    for e in index["files"]
                        .as_array()
                        .ok_or_else(|| anyhow!("index.json has no files list"))?
                    {
                        if e["path"] == path {
                            return Ok((
                                e["offset"].as_u64().unwrap_or(0),
                                e["size"].as_u64().unwrap_or(0),
                            ));
                        }
                    }
                    bail!("{path} is not in index.json")
                };
                let mut img =
                    File::open(&image).with_context(|| format!("opening {}", image.display()))?;
                let (fd_off, fd_size) = find("arc/data.trpfd")?;
                let fd = read_at(&mut img, fd_off, fd_size as usize)?;
                let (fs_off, fs_size) = find("arc/data.trpfs")?;
                (fd, img, fs_off, fs_size)
            }
        };

        let fd = Table::root(&fd_buf, 0).context("data.trpfd is not readable")?;
        let file_hashes = fd.u64s(0)?;
        let pack_names = fd.strings(1)?;
        let file_info = fd.tables(2)?;
        let pack_info = fd.tables(3)?;
        let file_pack = file_hashes
            .iter()
            .zip(&file_info)
            .map(|(h, t)| (*h, t.u64(0, 0) as u32))
            .collect();
        let pack_size = pack_info.iter().map(|t| t.u64(0, 0)).collect();

        let head = read_at(&mut img, fs_off, 16)?;
        if &head[..7] != b"ONEPACK" {
            bail!("data.trpfs does not start with ONEPACK; it is not a Trinity archive");
        }
        let root_at = u64_at(&head, 8)?;
        let fs_buf = read_at(&mut img, fs_off + root_at, (fs_size - root_at) as usize)?;
        let fs = Table::root(&fs_buf, 0)?;
        let pack_offset = fs.u64s(0)?.into_iter().zip(fs.u64s(1)?).collect();

        Ok(Archive {
            img,
            fs_off,
            file_pack,
            pack_names,
            pack_size,
            pack_offset,
            pack_cache: HashMap::new(),
        })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.file_pack.contains_key(&fnv1a64(path))
    }

    /// The unpacked bytes of the file at `path`.
    pub fn read(&mut self, path: &str) -> Result<Vec<u8>> {
        self.read_hash(fnv1a64(path))
            .with_context(|| path.to_string())
    }

    /// Loads (and caches) the raw bytes of pack `pi`.
    fn pack(&mut self, pi: u32) -> Result<&[u8]> {
        if !self.pack_cache.contains_key(&pi) {
            let name = self
                .pack_names
                .get(pi as usize)
                .ok_or_else(|| anyhow!("pack {pi} is out of range"))?;
            let off = *self
                .pack_offset
                .get(&fnv1a64(name))
                .ok_or_else(|| anyhow!("pack {name} has no offset"))?;
            let size = *self
                .pack_size
                .get(pi as usize)
                .ok_or_else(|| anyhow!("pack {pi} has no size"))?;
            let data = read_at(&mut self.img, self.fs_off + off, size as usize)?;
            if self.pack_cache.len() > 64 {
                self.pack_cache.clear();
            }
            self.pack_cache.insert(pi, data);
        }
        Ok(&self.pack_cache[&pi])
    }

    /// The unpacked bytes of the file whose path hashes to `h`.
    pub fn read_hash(&mut self, h: u64) -> Result<Vec<u8>> {
        let pi = *self
            .file_pack
            .get(&h)
            .ok_or_else(|| anyhow!("not in the archive"))?;
        let pack = self.pack(pi)?;
        let root = Table::root(pack, 0)?;
        let k = root
            .u64s(0)?
            .iter()
            .position(|x| *x == h)
            .ok_or_else(|| anyhow!("not in its pack"))?;
        let entry = *root
            .tables(1)?
            .get(k)
            .ok_or_else(|| anyhow!("pack has no entry {k}"))?;
        let data = entry.bytes(entry.slot_count().saturating_sub(1))?;
        if entry.i8(1, -1) == -1 {
            return Ok(data.to_vec());
        }
        let size = entry.u64(3, 0) as usize;
        let mut out = vec![0u8; size];
        let n = oozextract::Extractor::new()
            .read_from_slice(data, &mut out)
            .map_err(|e| anyhow!("Oodle decompression failed: {e:?}"))?;
        if n != size {
            bail!("unpacked {n} bytes, expected {size}");
        }
        Ok(out)
    }

    /// Path hashes of every file in the pack that holds `path` (a pack names only one of its files; the others
    /// are known by hash until their names are guessed).
    pub fn pack_files(&mut self, path: &str) -> Result<Vec<u64>> {
        let pi = *self
            .file_pack
            .get(&fnv1a64(path))
            .ok_or_else(|| anyhow!("{path}: not in the archive"))?;
        self.pack_files_at(pi)
    }

    /// Path hashes of every file in pack `pi` (an index into `pack_names`).
    pub fn pack_files_at(&mut self, pi: u32) -> Result<Vec<u64>> {
        let pack = self.pack(pi)?;
        Table::root(pack, 0)?.u64s(0)
    }

    /// Index of the pack that holds the file whose path hashes to `h`.
    pub fn pack_of(&self, h: u64) -> Option<u32> {
        self.file_pack.get(&h).copied()
    }

    /// Paths of every Pokémon model (pokemon/data/pmNNNN/pmNNNN_FF_VV/pmNNNN_FF_VV.trmdl) found in the archive.
    /// Pack names are paths with the slashes removed, e.g. arc/pokemondatapm0025pm0025_00_00pm0025_00_00_base...,
    /// so the folder and form names are cut out of them and the guessed path is checked against the file table.
    pub fn pokemon_models(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in &self.pack_names {
            let Some(rest) = n.strip_prefix("arc/pokemondata") else {
                continue;
            };
            if rest.len() < 18 || !rest.starts_with("pm") {
                continue;
            }
            let (id, form) = (&rest[..6], &rest[6..18]);
            let p = format!("pokemon/data/{id}/{form}/{form}.trmdl");
            if out.last() != Some(&p) && self.contains(&p) {
                out.push(p);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Paths of the player's part models: chara/model_pc/<folder>/<code>_<VV>_<kind><NN>.trmdl, e.g.
    /// chara/model_pc/p0_hrs0000_defaultf/p0_hrs0000_00_hrs00.trmdl. Folders come from pack names
    /// (arc/charamodel_pcp0_hrs0000_defaultfp0_hrs0000_00_hrs00.trmdl.trpak); every VV 00..03 and NN 00..09 is
    /// checked against the file table. 208 parts in the user's dump, all VV = 00; only the uniform has NN 01..03.
    pub fn player_parts(&self) -> Vec<String> {
        let mut folders: Vec<(&str, &str, &str)> = Vec::new(); // (folder, code, kind)
        for n in &self.pack_names {
            let Some(body) = n.strip_prefix("arc/charamodel_pc") else {
                continue;
            };
            // "p0_hrs0000_defaultfp0_hrs0000_00_hrs00...": code "p0_hrs0000", kind "hrs"
            let b = body.as_bytes();
            if b.len() < 12 || b[0] != b'p' || !b[1].is_ascii_digit() || b[2] != b'_' {
                continue;
            }
            let Some(kind_end) = body[3..]
                .find(|c: char| !c.is_ascii_lowercase())
                .map(|i| i + 3)
            else {
                continue;
            };
            let code_end = kind_end + 4;
            if body.len() <= code_end + 1
                || !b[kind_end..code_end].iter().all(u8::is_ascii_digit)
                || b[code_end] != b'_'
            {
                continue;
            }
            let code = &body[..code_end];
            let Some(split) = (code_end + 2..body.len()).find(|&i| body[i..].starts_with(code))
            else {
                continue;
            };
            let entry = (&body[..split], code, &body[3..kind_end]);
            if !folders.contains(&entry) {
                folders.push(entry);
            }
        }
        let mut out = Vec::new();
        for (folder, code, kind) in folders {
            for v in 0..4 {
                for k in 0..10 {
                    let p = format!("chara/model_pc/{folder}/{code}_{v:02}_{kind}{k:02}.trmdl");
                    if self.contains(&p) {
                        out.push(p);
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Paths of the character models: trainers (chara/model_tr, model_uq, model_vr) and props (chara/model_ob).
    /// Their packs are named after one file in the model's folder, often the `_loc.trskl` rather than the
    /// `.trmdl`, e.g. arc/charamodel_trtr0330_karatetr0330_00_loc.trskl.trpak. The folder (`tr0330_karate`) and
    /// that file's stem give the candidates `<stem>.trmdl` and `<code>_00..03.trmdl`; each is checked against the
    /// file table. Found 264 in the user's dump: 30 tr, 74 uq, 29 vr, 131 ob (vr models are split into body and
    /// head parts; player parts in chara/model_pc are not listed here, they need the base skeleton).
    pub fn character_models(&self) -> Vec<String> {
        const GROUPS: [(&str, &str); 4] = [
            ("trtr", "chara/model_tr/"),
            ("uqtr", "chara/model_uq/"),
            ("vrtr", "chara/model_vr/"),
            ("obob", "chara/model_ob/"),
        ];
        let mut out: Vec<String> = Vec::new();
        for n in &self.pack_names {
            let Some(rest) = n
                .strip_prefix("arc/charamodel_")
                .and_then(|r| r.strip_suffix(".trpak"))
            else {
                continue;
            };
            let Some(&(_, dir)) = GROUPS.iter().find(|(g, _)| rest.starts_with(g)) else {
                continue;
            };
            // "tr0330_karatetr0330_00_loc.trskl": the folder is the code, '_' and a name, up to where the code
            // starts again
            let body = &rest[2..];
            let (Some(code), Some(kind)) = (body.get(..6), body.get(..2)) else {
                continue;
            };
            let starts_code = |s: &[u8]| {
                s.len() > 7
                    && s.starts_with(kind.as_bytes())
                    && s[2..6].iter().all(u8::is_ascii_digit)
                    && s[6] == b'_'
            };
            let b = body.as_bytes();
            let Some(split) = (7..b.len()).find(|&i| starts_code(&b[i..])) else {
                continue;
            };
            let (folder, file) = (&body[..split], &body[split..]);
            let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
            let stem = stem.strip_suffix("_loc").unwrap_or(stem);
            let mut candidates = vec![format!("{dir}{folder}/{stem}.trmdl")];
            candidates.extend((0..4).map(|v| format!("{dir}{folder}/{code}_{v:02}.trmdl")));
            for p in candidates {
                if self.contains(&p) {
                    out.push(p);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

fn read_at(f: &mut File, off: u64, n: usize) -> Result<Vec<u8>> {
    f.seek(SeekFrom::Start(off))?;
    let mut v = vec![0u8; n];
    f.read_exact(&mut v)?;
    Ok(v)
}
