//! The texture catalogue behind the Textures tab: every `.bntx` the archive names, sorted into groups (UI,
//! Pokédex, Pokémon, characters, field, ...) and folders.
//!
//! Sources:
//! - Pack names. 4984 packs are named after a `.bntx` (`arc/<path without slashes>.trpak`). `recover` puts the
//!   slashes back by trying splits against the hash table, starting from the folders already found, so most
//!   names cost a handful of hash lookups. Examples: `appli/tex/icon_item/item_0183/item_0183.bntx`,
//!   `demo/appli/pokedex/zk0000_00/pokedex_capture_thum_001.bntx`.
//! - The rest of each pack. A pack is named after one file and can hold many: the item icon pack holds 562
//!   textures. `expand` lists the pack's files by hash, keeps the BNTX ones and names them by the texture name
//!   stored inside (`texture::info`); where `<folder>/<name>.bntx` or `<folder>/<name>/<name>.bntx` is in the
//!   hash table, that is its path.
//!
//! - Model packs. A model's pack holds its textures (Pikachu's: 26 BNTX among 260 files), but is named after the
//!   `.trmdl`, so every model discovery finds (`pokemon_models`, `character_models`, `player_parts`) adds its
//!   folder with no files listed until it is expanded.
//!
//! Folders: a texture's folder is its path's folder, or the one above when the folder is named after the file
//! (`icon_item/item_0183/item_0183.bntx` goes in `icon_item/`), so one-file folders do not flood the list. The
//! Pokémon icons (`pokemon/data/pm0001/pm0001_00_00/icon/pm0001_00_00_00.bntx`, one folder per form) share one
//! folder, `pokemon/data/pm*/icon/`.
//!
//! Checked over the user's dump (`probe --textures decode`, 2026-11-16): all 4984 pack names rebuilt in 0.4 s;
//! expanding every folder (model folders included) took 47 s and gave 36,478 textures (UI & icons 5008,
//! Pokédex 1695, Pokémon 17,390, characters 6295, field 5715, environment 332, effects 38, cutscenes 5);
//! all 36,478 decode (313 s).

use crate::archive::{Archive, fnv1a64};
use crate::texture;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Group {
    Ui,
    Pokedex,
    Pokemon,
    Characters,
    Field,
    Effects,
    Environment,
    Cutscenes,
    Other,
}

impl Group {
    pub const ALL: [Group; 9] = [
        Group::Ui,
        Group::Pokedex,
        Group::Pokemon,
        Group::Characters,
        Group::Field,
        Group::Effects,
        Group::Environment,
        Group::Cutscenes,
        Group::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Ui => "UI & icons",
            Group::Pokedex => "Pokédex",
            Group::Pokemon => "Pokémon",
            Group::Characters => "Characters",
            Group::Field => "Field",
            Group::Effects => "Effects",
            Group::Environment => "Environment",
            Group::Cutscenes => "Cutscenes",
            Group::Other => "Other",
        }
    }

    /// The group of a path, or of a pack name without its slashes (both work: the tests look at prefixes).
    pub fn of(path: &str) -> Group {
        let p = path.replace('/', "");
        if p.starts_with("demoapplipokedex") || p.starts_with("applitexpokedex") {
            Group::Pokedex
        } else if p.starts_with("appli") || p.starts_with("system_resource") {
            Group::Ui
        } else if p.starts_with("pokemon") {
            Group::Pokemon
        } else if p.starts_with("chara") {
            Group::Characters
        } else if p.starts_with("field") || p.starts_with("world") {
            Group::Field
        } else if p.starts_with("effect") {
            Group::Effects
        } else if p.starts_with("env") {
            Group::Environment
        } else if p.starts_with("demo") {
            Group::Cutscenes
        } else {
            Group::Other
        }
    }

    /// The part of folder paths the group's list leaves out.
    fn root(self) -> &'static [&'static str] {
        match self {
            Group::Ui => &["appli/tex/", "appli/"],
            Group::Pokedex => &["demo/appli/pokedex/", "appli/tex/"],
            Group::Pokemon => &["pokemon/data/", "pokemon/"],
            Group::Characters => &["chara/"],
            Group::Field => &["field_graphic/"],
            Group::Effects => &["effect/"],
            Group::Environment => &["env/"],
            Group::Cutscenes => &["demo/"],
            Group::Other => &[],
        }
    }
}

/// Top folders tried first when rebuilding a pack name's path; more are learned from every path found.
const ROOTS: [&str; 13] = [
    "appli/",
    "appli/tex/",
    "demo/",
    "pokemon/",
    "pokemon/data/",
    "field_graphic/",
    "chara/",
    "effect/",
    "env/",
    "system_resource/",
    "world/",
    "ai/",
    "message/",
];

#[derive(Clone, Debug)]
pub struct TexFile {
    pub hash: u64,
    /// File name (`item_0183.bntx`), or the texture name inside it for files known only by hash.
    pub name: String,
    /// The full path, when it is confirmed against the hash table.
    pub path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub group: Group,
    /// Real folder path with a trailing `/`, or `""` for the packs whose names could not be rebuilt.
    pub path: String,
    /// The packs (indices into `Archive::pack_names`) its files come from.
    pub packs: Vec<u32>,
    pub files: Vec<TexFile>,
    /// True once `expand` added the packs' other textures.
    pub expanded: bool,
}

impl Folder {
    /// The folder as the list shows it: without the group's root.
    pub fn label(&self) -> &str {
        if self.path.is_empty() {
            return "(path not found)";
        }
        let short = self
            .group
            .root()
            .iter()
            .find_map(|r| self.path.strip_prefix(r))
            .unwrap_or(&self.path);
        match short.trim_end_matches('/') {
            "" => "(top)",
            s => s,
        }
    }
}

/// Folders known so far: (collapsed form, real path with trailing `/`).
pub(crate) struct Dirs {
    known: Vec<(String, String)>,
    seen: HashSet<String>,
}

impl Dirs {
    pub(crate) fn new() -> Dirs {
        let mut d = Dirs {
            known: Vec::new(),
            seen: HashSet::new(),
        };
        for r in ROOTS {
            d.add(r);
        }
        d
    }

    fn add(&mut self, dir: &str) {
        if self.seen.insert(dir.to_string()) {
            self.known.push((dir.replace('/', ""), dir.to_string()));
        }
    }

    /// Learns every folder of `path`.
    fn learn(&mut self, path: &str) {
        for (i, _) in path.match_indices('/') {
            self.add(&path[..=i]);
        }
    }
}

/// Puts exactly `left` slashes into `rest` (after `acc`) in every way that keeps the extension in the last part,
/// and returns the first result the archive contains.
fn split_into(ar: &Archive, acc: &mut String, rest: &str, left: usize) -> bool {
    if left == 0 {
        let len = acc.len();
        acc.push_str(rest);
        if ar.contains(acc) {
            return true;
        }
        acc.truncate(len);
        return false;
    }
    for (i, c) in rest.char_indices().skip(1) {
        if c == '.' {
            break; // folder names never hold the extension's dot
        }
        let len = acc.len();
        acc.push_str(&rest[..i]);
        acc.push('/');
        if split_into(ar, acc, &rest[i..], left - 1) {
            return true;
        }
        acc.truncate(len);
    }
    false
}

/// Rebuilds the path of `collapsed` (a pack name without `arc/` and `.trpak`): the longest known folders that it
/// starts with are tried first, with up to 2 more slashes (3 below a folder one or two levels deep:
/// `pokemon/data/` + `pm0001/pm0001_00_00/icon/pm0001_00_00_00.bntx`).
pub(crate) fn recover(ar: &Archive, collapsed: &str, dirs: &mut Dirs) -> Option<String> {
    let mut cands: Vec<(usize, String)> = dirs
        .known
        .iter()
        .filter(|(c, _)| collapsed.len() > c.len() && collapsed.starts_with(c.as_str()))
        .map(|(c, real)| (c.len(), real.clone()))
        .collect();
    cands.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    cands.truncate(12);
    for (n, real) in cands {
        let rest = &collapsed[n..];
        let max = if real.matches('/').count() <= 2 { 3 } else { 2 };
        for k in 0..=max {
            let mut acc = real.clone();
            if split_into(ar, &mut acc, rest, k) {
                dirs.learn(&acc);
                return Some(acc);
            }
        }
    }
    None
}

/// The folder a texture is listed in (see the module comment).
fn folder_of(path: &str) -> String {
    if path.starts_with("pokemon/data/pm") && path.contains("/icon/") {
        return "pokemon/data/pm*/icon/".to_string();
    }
    let (dir, file) = path.rsplit_once('/').map_or(("", path), |(d, f)| (d, f));
    let stem = file.split('.').next().unwrap_or(file);
    match dir.rsplit_once('/') {
        Some((parent, last)) if last == stem => format!("{parent}/"),
        _ => format!("{dir}/"),
    }
}

/// Builds the catalogue from the pack names. Returns the folders sorted by group and path, and how many pack
/// names could not be rebuilt (their packs are listed in each group's "(path not found)" folder).
pub fn catalogue(ar: &Archive) -> (Vec<Folder>, usize) {
    let mut dirs = Dirs::new();
    let mut by_folder: std::collections::BTreeMap<(Group, String), Folder> = Default::default();
    let mut lost = 0;
    for (pi, name) in ar.pack_names.iter().enumerate() {
        let Some(collapsed) = name
            .strip_prefix("arc/")
            .and_then(|n| n.strip_suffix(".trpak"))
            .filter(|n| n.ends_with(".bntx"))
        else {
            continue;
        };
        let path = recover(ar, collapsed, &mut dirs);
        let (group, folder) = match &path {
            Some(p) => (Group::of(p), folder_of(p)),
            None => {
                lost += 1;
                (Group::of(collapsed), String::new())
            }
        };
        let f = by_folder
            .entry((group, folder.clone()))
            .or_insert_with(|| Folder {
                group,
                path: folder,
                packs: Vec::new(),
                files: Vec::new(),
                expanded: false,
            });
        if !f.packs.contains(&(pi as u32)) {
            f.packs.push(pi as u32);
        }
        if let Some(p) = path {
            let file = p.rsplit('/').next().unwrap_or(&p).to_string();
            f.files.push(TexFile {
                hash: fnv1a64(&p),
                name: file,
                path: Some(p),
            });
        }
    }
    // models: their packs hold their textures
    let models = ar
        .pokemon_models()
        .into_iter()
        .chain(ar.character_models())
        .chain(ar.player_parts());
    for m in models {
        let (Some(pi), Some((dir, _))) = (ar.pack_of(fnv1a64(&m)), m.rsplit_once('/')) else {
            continue;
        };
        let group = Group::of(&m);
        let f = by_folder
            .entry((group, format!("{dir}/")))
            .or_insert_with(|| Folder {
                group,
                path: format!("{dir}/"),
                packs: Vec::new(),
                files: Vec::new(),
                expanded: false,
            });
        if !f.packs.contains(&pi) {
            f.packs.push(pi);
        }
    }
    let mut folders: Vec<Folder> = by_folder.into_values().collect();
    for f in &mut folders {
        sort_files(&mut f.files);
    }
    (folders, lost)
}

fn sort_files(files: &mut [TexFile]) {
    files.sort_by(|a, b| a.name.cmp(&b.name).then(a.hash.cmp(&b.hash)));
}

/// The textures in `folder`'s packs that it does not list yet (see the module comment). Files that are not
/// BNTX or do not unpack are skipped; the second value counts them.
pub fn expand(ar: &mut Archive, folder: &Folder) -> (Vec<TexFile>, usize) {
    let known: HashSet<u64> = folder.files.iter().map(|f| f.hash).collect();
    // where the unnamed files may live: the folder, its files' folders and the folders above those
    let mut dirs: Vec<String> = Vec::new();
    let mut add_dir = |d: &str| {
        if !d.is_empty() && !d.contains('*') && !dirs.iter().any(|x| x == d) {
            dirs.push(d.to_string());
        }
    };
    add_dir(&folder.path);
    for p in folder.files.iter().filter_map(|f| f.path.as_deref()) {
        if let Some((d, _)) = p.rsplit_once('/') {
            add_dir(&format!("{d}/"));
            if let Some((up, _)) = d.rsplit_once('/') {
                add_dir(&format!("{up}/"));
            }
        }
    }
    let mut found = Vec::new();
    let mut skipped = 0;
    for &pi in &folder.packs {
        let Ok(hashes) = ar.pack_files_at(pi) else {
            skipped += 1;
            continue;
        };
        for h in hashes {
            if known.contains(&h) || found.iter().any(|f: &TexFile| f.hash == h) {
                continue;
            }
            let Ok(b) = ar.read_hash(h) else {
                skipped += 1;
                continue;
            };
            if !b.starts_with(b"BNTX") {
                continue;
            }
            let name = texture::info(&b)
                .map(|i| i.name)
                .ok()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("#{h:016x}"));
            let path = dirs.iter().find_map(|d| {
                [format!("{d}{name}.bntx"), format!("{d}{name}/{name}.bntx")]
                    .into_iter()
                    .find(|p| fnv1a64(p) == h)
            });
            let name = path
                .as_ref()
                .map_or(name, |p| p.rsplit('/').next().unwrap_or(p).to_string());
            found.push(TexFile {
                hash: h,
                name,
                path,
            });
        }
    }
    sort_files(&mut found);
    (found, skipped)
}

/// Shrinks an image so that its longer side is at most `max` (box filter), for thumbnails.
pub fn shrink(img: &texture::Image, max: u32) -> (u32, u32, Vec<[u8; 4]>) {
    let (w, h) = (img.width.max(1), img.height.max(1));
    let k = w.max(h).div_ceil(max).max(1);
    let (tw, th) = (w.div_ceil(k), h.div_ceil(k));
    let mut out = Vec::with_capacity((tw * th) as usize);
    for ty in 0..th {
        for tx in 0..tw {
            let mut sum = [0u32; 4];
            let mut n = 0;
            for y in ty * k..((ty + 1) * k).min(h) {
                for x in tx * k..((tx + 1) * k).min(w) {
                    let p = img.rgba[(y * w + x) as usize];
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                    n += 1;
                }
            }
            let n = n.max(1);
            out.push(sum.map(|s| (s / n) as u8));
        }
    }
    (tw, th, out)
}
