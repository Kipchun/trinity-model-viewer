//! The cutscene catalogue behind the Cutscenes tab: every cutscene timeline (`.trtml`) under `demo/`.
//!
//! How a cutscene is put together (see `timeline.rs` for the timeline's slots):
//! - `demo/<kind>/<name>/<name>.trtml`: the timeline, the cutscene's script.
//! - next to it: camera animations (`.trcma`), object and character animations (`.tracp`, `.tracm`, `.tranm`),
//!   sometimes models (`.trmdl`) and textures; trainers' cutscene clips are under `chara/demo_cc_base/`.
//! - its scenes: `world/scene/parts/demo/<kind>/<name>/<sub>_/<sub>_0.trscn` (rooms, characters per cut),
//!   loaded by the timeline's `Scene/LoadSubScene` clips.
//!
//! Sources: pack names only. A pack is named after one of its files, often a camera or an animation, not the
//! timeline, so each `demo/` pack name is rebuilt into a path (`textures::recover`) and every folder above that
//! file is tried as `<folder>/<folder>.trtml` against the hash table. Brute force over `demo/ev/dNNN`,
//! `demo/ee/eeNNN` and `demo/ej/ejNNN` (0-999) finds no timeline this misses.
//!
//! Kinds (from the folder names and the commands used):
//! - `demo/ev/dNNN`: story events (29), with sub-scenes, characters and message windows.
//! - `demo/ee/eeNNN`: battle cutscenes (ball throws, captures, entries).
//! - `demo/ee/ee_bk_*`: trainer cutscenes in battle, named after the trainer (`ee_bk_botan_01_01`).
//! - `demo/ee/ee_wait*`: battle idle camera loops.
//! - `demo/ej/`: Terastal changes and Tera Raid scenes.
//! - `demo/sd/`: system scenes (title, trade, Mystery Gift, sandwich results, ...).
//! - `demo/test/`, `demo/share/`, templates: test and shared timelines.

use crate::archive::Archive;
use crate::textures::{Dirs, recover};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Group {
    Story,
    Battle,
    TrainerBattle,
    BattleWait,
    Tera,
    System,
    Test,
}

impl Group {
    pub const ALL: [Group; 7] = [
        Group::Story,
        Group::Battle,
        Group::TrainerBattle,
        Group::BattleWait,
        Group::Tera,
        Group::System,
        Group::Test,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Story => "Story events",
            Group::Battle => "Battle",
            Group::TrainerBattle => "Trainers in battle",
            Group::BattleWait => "Battle idle cameras",
            Group::Tera => "Terastal and raids",
            Group::System => "System",
            Group::Test => "Test and templates",
        }
    }

    pub fn of(path: &str) -> Group {
        let p = path.strip_prefix("demo/").unwrap_or(path);
        if p.starts_with("ev/") {
            Group::Story
        } else if p.starts_with("ee/ee_bk") {
            Group::TrainerBattle
        } else if p.starts_with("ee/ee_wait") {
            Group::BattleWait
        } else if p.starts_with("ee/kaiwa") || p.starts_with("test") || p.starts_with("share") {
            Group::Test
        } else if p.starts_with("ee/") {
            Group::Battle
        } else if p.starts_with("ej/") {
            Group::Tera
        } else if p.starts_with("sd/") {
            Group::System
        } else {
            Group::Test
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cutscene {
    pub group: Group,
    /// The timeline's name without `.trtml` (`d010`, `ee_bk_botan_01_01`).
    pub name: String,
    /// The timeline's path.
    pub path: String,
    /// The timeline's folder with a trailing `/`.
    pub folder: String,
    /// The pack holding the timeline (index into `Archive::pack_names`) and its stored size.
    pub pack: Option<u32>,
    pub size: u64,
}

/// Every cutscene timeline found, sorted by group and name. Also returns how many `demo/` pack names could not
/// be rebuilt into paths.
pub fn catalogue(ar: &Archive) -> (Vec<Cutscene>, usize) {
    let mut dirs = Dirs::new();
    let mut found = BTreeSet::new();
    let mut lost = 0;
    for pack_name in &ar.pack_names {
        let Some(collapsed) = pack_name
            .strip_prefix("arc/")
            .and_then(|n| n.strip_suffix(".trpak"))
        else {
            continue;
        };
        if !collapsed.starts_with("demo") || collapsed.starts_with("demoappli") {
            continue;
        }
        let Some(path) = recover(ar, collapsed, &mut dirs) else {
            lost += 1;
            continue;
        };
        if path.ends_with(".trtml") {
            found.insert(path.clone());
        }
        // every folder above the file, as <folder>/<folder>.trtml
        let mut dir = path.as_str();
        while let Some((up, _)) = dir.rsplit_once('/') {
            if up == "demo" || !up.starts_with("demo/") {
                break;
            }
            let name = up.rsplit('/').next().unwrap_or(up);
            let cand = format!("{up}/{name}.trtml");
            if ar.contains(&cand) {
                found.insert(cand);
            }
            dir = up;
        }
    }
    let mut list: Vec<Cutscene> = found
        .into_iter()
        .map(|path| {
            let (folder, file) = path
                .rsplit_once('/')
                .map_or((String::new(), path.as_str()), |(d, f)| {
                    (format!("{d}/"), f)
                });
            let pack = ar.pack_of(crate::archive::fnv1a64(&path));
            Cutscene {
                group: Group::of(&path),
                name: file.trim_end_matches(".trtml").to_string(),
                folder,
                pack,
                size: pack.map_or(0, |p| ar.pack_size(p)),
                path,
            }
        })
        .collect();
    list.sort_by(|a, b| (a.group, &a.name).cmp(&(b.group, &b.name)));
    (list, lost)
}
