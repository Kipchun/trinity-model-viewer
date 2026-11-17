//! The effect catalogue behind the Effects tab: every particle effect (`.ptcl`) and animation effect list
//! (`.traef`) the archive names. Nothing is decoded or played yet; this is a list to browse and sort.
//!
//! Sources: pack names only. A pack named after a `.ptcl` or `.traef` (`arc/<path without slashes>.trpak`) is one
//! entry; `textures::recover` puts the slashes back against the hash table. A pack can hold more files than the
//! one it is named after; those are not listed yet.
//!
//! What the files are (checked on a few files, 2026-11-16; see `.claude/docs/FORMATS.md`):
//! - `.ptcl`: Nintendo's VFXB effect binary (NintendoWare / Eft2), version 0x16, under `effect/`. Sections seen:
//!   `ESTA` (emitter set table), `ESET` (emitter set, named e.g. `nushiflying_rocksmoke`), `EMTR` (emitters,
//!   e.g. `smoke_front`), `GRTF` with an embedded `BNTX` (textures), `G3PR` (primitive meshes), `GRSN` / `GRSC`
//!   (compiled shaders).
//! - `.traef`: small FlatBuffers named after an animation clip of a Pokémon or character
//!   (`pokemon/data/pm0145/pm0145_00_00/pm0145_00_00_20500_damage01.traef`); guess: effect events (which
//!   effect, which frame, which bone) for that clip. The damage clip looked at was a near-empty 24-byte table.
//!
//! Groups: `effect/battle_*` battle, `effect/appli*` menus, `effect/demo*` cutscenes, `effect/field*` field;
//! `.traef` files go under Pokémon or Characters by their path.

use crate::archive::Archive;
use crate::textures::{Dirs, recover};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Kind {
    /// `.ptcl`: a VFXB particle effect.
    Particles,
    /// `.traef`: effect events for one animation clip.
    Events,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Particles => "Particles",
            Kind::Events => "Anim events",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Kind::Particles => ".ptcl",
            Kind::Events => ".traef",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Group {
    Battle,
    Field,
    Menus,
    Cutscenes,
    Pokemon,
    Characters,
    Other,
}

impl Group {
    pub const ALL: [Group; 7] = [
        Group::Battle,
        Group::Field,
        Group::Menus,
        Group::Cutscenes,
        Group::Pokemon,
        Group::Characters,
        Group::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Battle => "Battle",
            Group::Field => "Field",
            Group::Menus => "Menus",
            Group::Cutscenes => "Cutscenes",
            Group::Pokemon => "Pokémon",
            Group::Characters => "Characters",
            Group::Other => "Other",
        }
    }

    /// The group of a path, or of a pack name without its slashes (only prefixes are looked at).
    pub fn of(path: &str) -> Group {
        let p = path.replace('/', "");
        if p.starts_with("effectbattle") {
            Group::Battle
        } else if p.starts_with("effectfield") {
            Group::Field
        } else if p.starts_with("effectappli") {
            Group::Menus
        } else if p.starts_with("effectdemo") {
            Group::Cutscenes
        } else if p.starts_with("pokemon") {
            Group::Pokemon
        } else if p.starts_with("chara") {
            Group::Characters
        } else {
            Group::Other
        }
    }
}

#[derive(Clone, Debug)]
pub struct Effect {
    pub kind: Kind,
    pub group: Group,
    /// File name without the extension (`nushiflying_rocksmoke`).
    pub name: String,
    /// The full path, when it could be rebuilt and is confirmed against the hash table.
    pub path: Option<String>,
    /// Folder with a trailing `/`, or `""` when the path was not rebuilt.
    pub folder: String,
    /// The pack it is named after (an index into `Archive::pack_names`).
    pub pack: u32,
    /// The pack's stored size in bytes.
    pub size: u64,
}

/// Builds the list from the pack names, sorted by group and path. Returns it with how many pack names could not
/// be rebuilt into paths (those are still listed, by their collapsed name).
pub fn catalogue(ar: &Archive) -> (Vec<Effect>, usize) {
    let mut dirs = Dirs::new();
    let mut list = Vec::new();
    let mut lost = 0;
    for (pi, pack_name) in ar.pack_names.iter().enumerate() {
        let Some(collapsed) = pack_name
            .strip_prefix("arc/")
            .and_then(|n| n.strip_suffix(".trpak"))
        else {
            continue;
        };
        let kind = if collapsed.ends_with(Kind::Particles.ext()) {
            Kind::Particles
        } else if collapsed.ends_with(Kind::Events.ext()) {
            Kind::Events
        } else {
            continue;
        };
        let path = recover(ar, collapsed, &mut dirs);
        if path.is_none() {
            lost += 1;
        }
        let shown = path.as_deref().unwrap_or(collapsed);
        let (folder, file) = match &path {
            Some(p) => p
                .rsplit_once('/')
                .map_or((String::new(), p.as_str()), |(d, f)| (format!("{d}/"), f)),
            None => (String::new(), collapsed),
        };
        list.push(Effect {
            kind,
            group: Group::of(shown),
            name: file.strip_suffix(kind.ext()).unwrap_or(file).to_string(),
            folder,
            path,
            pack: pi as u32,
            size: ar.pack_size(pi as u32),
        });
    }
    list.sort_by(|a, b| (a.group, &a.folder, &a.name).cmp(&(b.group, &b.folder, &b.name)));
    (list, lost)
}
