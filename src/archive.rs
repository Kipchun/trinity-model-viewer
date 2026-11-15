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
        let h = fnv1a64(path);
        let pi = *self
            .file_pack
            .get(&h)
            .ok_or_else(|| anyhow!("{path}: not in the archive"))?;
        if !self.pack_cache.contains_key(&pi) {
            let name = &self.pack_names[pi as usize];
            let off = *self
                .pack_offset
                .get(&fnv1a64(name))
                .ok_or_else(|| anyhow!("pack {name} has no offset"))?;
            let data = read_at(
                &mut self.img,
                self.fs_off + off,
                self.pack_size[pi as usize] as usize,
            )?;
            if self.pack_cache.len() > 64 {
                self.pack_cache.clear();
            }
            self.pack_cache.insert(pi, data);
        }
        let pack = &self.pack_cache[&pi];
        let root = Table::root(pack, 0)?;
        let k = root
            .u64s(0)?
            .iter()
            .position(|x| *x == h)
            .ok_or_else(|| anyhow!("{path}: not in its pack"))?;
        let entry = root.tables(1)?[k];
        let data = entry.bytes(entry.slot_count() - 1)?;
        if entry.i8(1, -1) == -1 {
            return Ok(data.to_vec());
        }
        let size = entry.u64(3, 0) as usize;
        let mut out = vec![0u8; size];
        let n = oozextract::Extractor::new()
            .read_from_slice(data, &mut out)
            .map_err(|e| anyhow!("{path}: Oodle decompression failed: {e:?}"))?;
        if n != size {
            bail!("{path}: unpacked {n} bytes, expected {size}");
        }
        Ok(out)
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
}

fn read_at(f: &mut File, off: u64, n: usize) -> Result<Vec<u8>> {
    f.seek(SeekFrom::Start(off))?;
    let mut v = vec![0u8; n];
    f.read_exact(&mut v)?;
    Ok(v)
}
