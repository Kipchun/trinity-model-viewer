//! The game's sound: Wwise sound banks and file packages in the RomFS (outside the Trinity archive), at
//! `audio/wwise/bin/Switch/`. Bank version 140. No names are stored, only 32-bit ids.
//!   .bnk  sections: u32 tag, u32 size. BKHD (u32 version), DIDX (id, offset, size: 12 bytes per sound kept in the
//!         bank), DATA (those sounds), HIRC (u32 count, then objects: u8 type, u32 size, data).
//!         HIRC type 2 Sound: id, u32 plugin, u8 stream type, u32 source id (data +9).
//!         HIRC type 11 Music track: id, u8 flags, u32 source count, then 14-byte sources (plugin u32, stream
//!         type u8, source id u32 at +5, memory size u32, u8 bits).
//!   .pck  "AKPK", u32 header size, u32 version, then the sizes of the language map, the bank table, the stream
//!         table and the externals table; tables follow from offset 28: u32 count, then 20-byte entries (u32 id,
//!         u32 block size, u32 size, u32 start block, u32 language). A file sits at start block x block size.
//! Every streamed file in the user's dump is named by exactly one bank's Sound or Music track, which gives its
//! group (music, Pokémon cries, moves, ...). Counts (2026-11-16): Stream.pck 8938, SDC0.pck 1768, SDC2.pck 2364
//! files; banks hold 4,600 more. VIB.bnk is controller rumble, not sound, and is left out.

use crate::archive::RomFile;
use crate::sound_names;
use crate::wem;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::path::Path;

pub const DIR: &str = "audio/wwise/bin/Switch/";

const BANKS: [&str; 15] = [
    "BGM",
    "ME",
    "COMMON",
    "PM_VOICE",
    "PM",
    "PM_SKILLS",
    "BATTLE_SYSTEM",
    "UI",
    "UI_ADD",
    "PL",
    "NPC",
    "PROPS",
    "ENV",
    "DEMO",
    "VIB",
];
const PACKAGES: [&str; 3] = ["Stream", "SDC0", "SDC2"];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Group {
    Music,
    Jingles,
    Cries,
    Pokemon,
    Moves,
    Battle,
    Menus,
    Player,
    Npcs,
    Props,
    Ambience,
    Cutscenes,
    Common,
    Other,
}

impl Group {
    pub const ALL: [Group; 14] = [
        Group::Music,
        Group::Jingles,
        Group::Cries,
        Group::Pokemon,
        Group::Moves,
        Group::Battle,
        Group::Menus,
        Group::Player,
        Group::Npcs,
        Group::Props,
        Group::Ambience,
        Group::Cutscenes,
        Group::Common,
        Group::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Music => "Music",
            Group::Jingles => "Jingles",
            Group::Cries => "Pokémon cries",
            Group::Pokemon => "Pokémon sounds",
            Group::Moves => "Moves",
            Group::Battle => "Battle",
            Group::Menus => "Menus",
            Group::Player => "Player",
            Group::Npcs => "People",
            Group::Props => "Objects",
            Group::Ambience => "Ambience",
            Group::Cutscenes => "Cutscenes",
            Group::Common => "Common",
            Group::Other => "Other",
        }
    }

    fn of_bank(bank: &str) -> Group {
        match bank {
            "BGM" => Group::Music,
            "ME" => Group::Jingles,
            "PM_VOICE" => Group::Cries,
            "PM" => Group::Pokemon,
            "PM_SKILLS" => Group::Moves,
            "BATTLE_SYSTEM" => Group::Battle,
            "UI" | "UI_ADD" => Group::Menus,
            "PL" => Group::Player,
            "NPC" => Group::Npcs,
            "PROPS" => Group::Props,
            "ENV" => Group::Ambience,
            "DEMO" => Group::Cutscenes,
            "COMMON" => Group::Common,
            _ => Group::Other,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sound {
    pub id: u32,
    pub group: Group,
    /// The bank whose Sound / Music track names it ("" if none does).
    pub bank: &'static str,
    /// Named by a Music track (part of the interactive music), not a Sound.
    pub music: bool,
    /// The file it is in (`Stream.pck`, `BGM.bnk`, ...), and where.
    pub file: &'static str,
    pub offset: u64,
    pub size: u32,
    pub header: Result<wem::Header, String>,
    /// The game's own title for it (from its music menus), when known.
    pub title: Option<String>,
    /// The developers' name for it: the music state path or event that selects it (see `sound_names`).
    pub dev_name: Option<String>,
}

fn file_name(stem: &str, ext: &str) -> &'static str {
    // the names are few and fixed; leaking keeps `Sound` cheap to clone
    Box::leak(format!("{stem}.{ext}").into_boxed_str())
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

struct Bank {
    name: &'static str,
    file: &'static str,
    /// (id, offset in the file, size)
    media: Vec<(u32, u64, u32)>,
    /// (source id, is a music track)
    refs: Vec<(u32, bool)>,
    hirc: Vec<u8>,
}

fn read_bank(romfs: &Path, name: &'static str) -> Result<Bank> {
    let file = file_name(name, "bnk");
    let mut f = RomFile::open(romfs, &format!("{DIR}{file}"))?;
    let (mut didx, mut data_at, mut hirc) = (Vec::new(), 0u64, Vec::new());
    let mut p = 0u64;
    while p + 8 <= f.size {
        let head = f.read_at(p, 8)?;
        let size = u32le(&head, 4).unwrap_or(0) as u64;
        match &head[..4] {
            b"DIDX" => didx = f.read_at(p + 8, size as usize)?,
            b"DATA" => data_at = p + 8,
            b"HIRC" => hirc = f.read_at(p + 8, size as usize)?,
            _ => {}
        }
        p += 8 + size;
    }
    let media = didx
        .as_chunks::<12>()
        .0
        .iter()
        .map(|e| {
            let (id, off, size) = (
                u32le(e, 0).unwrap_or(0),
                u32le(e, 4).unwrap_or(0),
                u32le(e, 8).unwrap_or(0),
            );
            (id, data_at + off as u64, size)
        })
        .collect();
    let mut refs = Vec::new();
    let count = u32le(&hirc, 0).unwrap_or(0);
    let mut q = 4usize;
    for _ in 0..count {
        let (Some(&kind), Some(size)) = (hirc.get(q), u32le(&hirc, q + 1)) else {
            break;
        };
        let d = q + 5;
        match kind {
            2 => refs.extend(u32le(&hirc, d + 9).map(|id| (id, false))),
            11 => {
                let n = u32le(&hirc, d + 5).unwrap_or(0).min(256) as usize;
                for k in 0..n {
                    refs.extend(u32le(&hirc, d + 9 + 14 * k + 5).map(|id| (id, true)));
                }
            }
            _ => {}
        }
        q = d + size as usize;
    }
    Ok(Bank {
        name,
        file,
        media,
        refs,
        hirc,
    })
}

/// (id, offset, size) of every file in a package's stream table.
fn read_package(romfs: &Path, file: &str) -> Result<Vec<(u32, u64, u32)>> {
    let mut f = RomFile::open(romfs, &format!("{DIR}{file}"))?;
    let head = f.read_at(0, 32)?;
    if !head.starts_with(b"AKPK") {
        return Err(anyhow!("{file} is not an AKPK package"));
    }
    let hsize = u32le(&head, 4).unwrap_or(0) as usize;
    let field = |k: usize| u32le(&head, 12 + 4 * k).unwrap_or(0) as usize;
    let (langs, banks) = (field(0), field(1));
    let t = f.read_at(8, hsize)?;
    let at = 20 + langs + banks; // the stream table, from the end of the 8-byte tag + size
    let count = u32le(&t, at).unwrap_or(0) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = at + 4 + 20 * i;
        let (Some(id), Some(block), Some(size), Some(start)) = (
            u32le(&t, e),
            u32le(&t, e + 4),
            u32le(&t, e + 8),
            u32le(&t, e + 12),
        ) else {
            break;
        };
        out.push((id, start as u64 * block as u64, size));
    }
    Ok(out)
}

/// Every sound in the dump, grouped by the bank that names it, with its header read. Problems with single files
/// are kept as notes; only an unreadable RomFS fails.
pub fn catalogue(romfs: &Path) -> Result<(Vec<Sound>, Vec<String>)> {
    let mut notes = Vec::new();
    let mut banks = Vec::new();
    for name in BANKS {
        match read_bank(romfs, name) {
            Ok(b) => banks.push(b),
            Err(e) => notes.push(format!("{name}.bnk: {e:#}")),
        }
    }
    if banks.is_empty() {
        return Err(anyhow!(
            "no Wwise banks found in {DIR}: {}",
            notes.join("; ")
        ));
    }
    // who names each source: the first bank wins
    let mut named: HashMap<u32, (&'static str, bool)> = HashMap::new();
    for b in &banks {
        for &(id, music) in &b.refs {
            named.entry(id).or_insert((b.name, music));
        }
    }
    // where each file is: packages first (a bank may keep only the start of a streamed sound)
    let mut place: Vec<(u32, &'static str, u64, u32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for stem in PACKAGES {
        let file = file_name(stem, "pck");
        match read_package(romfs, file) {
            Ok(list) => {
                for (id, off, size) in list {
                    if seen.insert(id) {
                        place.push((id, file, off, size));
                    }
                }
            }
            Err(e) => notes.push(format!("{file}: {e:#}")),
        }
    }
    for b in &banks {
        for &(id, off, size) in &b.media {
            if seen.insert(id) {
                place.push((id, b.file, off, size));
            }
        }
    }
    // headers: the first 4 KB of each file hold fmt, smpl and the data chunk's start
    let mut open: HashMap<&'static str, RomFile> = HashMap::new();
    let hircs: Vec<(&str, Vec<u8>)> = banks
        .iter_mut()
        .map(|b| (b.name, std::mem::take(&mut b.hirc)))
        .collect();
    let found = sound_names::find(romfs, &hircs, "English");
    notes.extend(found.notes);
    let mut sounds = Vec::with_capacity(place.len());
    for (id, file, offset, size) in place {
        let (bank, music) = named.get(&id).copied().unwrap_or(("", false));
        if bank == "VIB" || (bank.is_empty() && file == "VIB.bnk") {
            continue; // controller rumble
        }
        if !open.contains_key(file) {
            match RomFile::open(romfs, &format!("{DIR}{file}")) {
                Ok(f) => {
                    open.insert(file, f);
                }
                Err(e) => {
                    notes.push(format!("{file}: {e:#}"));
                    continue;
                }
            }
        }
        let f = open.get_mut(file).expect("opened above");
        let header = f
            .read_at(offset, (size as usize).min(4096))
            .and_then(|b| wem::header(&b))
            .map_err(|e| format!("{e:#}"));
        sounds.push(Sound {
            id,
            group: Group::of_bank(bank),
            bank,
            music,
            file,
            offset,
            size,
            header,
            title: found.title.get(&id).cloned(),
            dev_name: found.dev.get(&id).cloned(),
        });
    }
    sounds.sort_by_key(|s| (s.group, s.id));
    Ok((sounds, notes))
}

/// The whole `.wem` of `s`.
pub fn read(romfs: &Path, s: &Sound) -> Result<Vec<u8>> {
    RomFile::open(romfs, &format!("{DIR}{}", s.file))?.read_at(s.offset, s.size as usize)
}
