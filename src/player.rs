//! The player character, assembled from parts. Checked against the game's files:
//! - chara/model_pc_base/model/p0_base.trmdl has no meshes, only the base skeleton p0_base.trskl (74 bones, 72 joints).
//! - Every visible piece is a separate model in chara/model_pc/<folder>/ (see `Archive::player_parts`): the face, hair,
//!   outfit (`drs`: tops and bottoms, with the skin of the arms and neck), legwear (`lgw`: legs; `barefoot` is bare
//!   legs), gloves (`glv`; `hand` is bare hands), footwear, headwear, eyewear, bag and an accessory. With no outfit,
//!   legwear or gloves the body has holes: those parts are the body.
//! - Parts are `p0_` (shared) or `p1_` / `p2_` (outfits only, 19 each): two outfit sets for two body shapes.
//! - Parts' meshes use one joint list for the whole player; parts with bones of their own (hair, face, bags) bring a
//!   small skeleton that `Skeleton::attach` adds to the base.
//! - Animations: chara/motion_pc/base/p0_base.tracr lists the clips (p0_00_<number>_<name>.tranm).

use crate::archive::Archive;
use crate::materials::{self, Baked};
use crate::model::{self, Model};
use crate::skeleton::{self, Skeleton};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub const BASE_SKELETON: &str = "chara/model_pc_base/model/p0_base.trskl";
pub const ANIM_LIST: &str = "chara/motion_pc/base/p0_base.tracr";
/// Cut off the clip names for their labels.
pub const ANIM_STEM: &str = "p0_00";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Outfit,
    Legwear,
    Gloves,
    Footwear,
    Face,
    Hair,
    Headwear,
    Eyewear,
    Bag,
    Accessory,
}

impl Slot {
    pub const ALL: [Slot; 10] = [
        Slot::Outfit,
        Slot::Legwear,
        Slot::Gloves,
        Slot::Footwear,
        Slot::Face,
        Slot::Hair,
        Slot::Headwear,
        Slot::Eyewear,
        Slot::Bag,
        Slot::Accessory,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Slot::Outfit => "Outfit",
            Slot::Legwear => "Legwear",
            Slot::Gloves => "Gloves",
            Slot::Footwear => "Footwear",
            Slot::Face => "Face",
            Slot::Hair => "Hair",
            Slot::Headwear => "Headwear",
            Slot::Eyewear => "Eyewear",
            Slot::Bag => "Bag",
            Slot::Accessory => "Accessory",
        }
    }

    /// The part kind in file names.
    fn of_kind(kind: &str) -> Option<Slot> {
        Some(match kind {
            "drs" => Slot::Outfit,
            "lgw" => Slot::Legwear,
            "glv" => Slot::Gloves,
            "ftw" => Slot::Footwear,
            "face" => Slot::Face,
            "hrs" => Slot::Hair,
            "hdw" => Slot::Headwear,
            "eyw" => Slot::Eyewear,
            "bag" => Slot::Bag,
            "acc" => Slot::Accessory,
            _ => return None,
        })
    }

    fn index(self) -> usize {
        Slot::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

pub struct Part {
    pub path: String,
    pub slot: Slot,
    /// 1 or 2 for the two outfit sets (p1_ / p2_), 0 for shared parts (p0_).
    pub body: u8,
    /// e.g. "0000 defaultf", "0000 uniform 2" (sub-part drs02)
    pub label: String,
}

/// The parts found in the archive, sorted by slot and label.
pub fn catalog(ar: &Archive) -> Vec<Part> {
    let mut parts: Vec<Part> = ar
        .player_parts()
        .into_iter()
        .filter_map(|path| {
            // chara/model_pc/p0_hrs0000_defaultf/p0_hrs0000_00_hrs00.trmdl
            let mut it = path.rsplit('/');
            let file = it.next()?.strip_suffix(".trmdl")?;
            let folder = it.next()?;
            let body = folder.get(1..2)?.parse().ok()?;
            let rest = folder.get(3..)?; // "hrs0000_defaultf"
            let kind_end = rest.find(|c: char| !c.is_ascii_lowercase())?;
            let slot = Slot::of_kind(&rest[..kind_end])?;
            let number_and_name = rest[kind_end..].replacen('_', " ", 1);
            // the sub-part number: the last two digits of the file name
            let sub: u32 = file.get(file.len().checked_sub(2)?..)?.parse().ok()?;
            let label = if sub == 0 {
                number_and_name
            } else {
                format!("{number_and_name} {sub}")
            };
            Some(Part {
                path,
                slot,
                body,
                label,
            })
        })
        .collect();
    parts.sort_by(|a, b| {
        (a.slot.index(), a.body, &a.label).cmp(&(b.slot.index(), b.body, &b.label))
    });
    parts
}

/// The chosen part per slot (index into the catalog), and which outfit set (1 or 2) the outfit list shows.
#[derive(Clone, PartialEq)]
pub struct Outfit {
    pub body: u8,
    pub chosen: [Option<usize>; Slot::ALL.len()],
}

impl Outfit {
    pub fn get(&self, slot: Slot) -> Option<usize> {
        self.chosen[slot.index()]
    }

    pub fn set(&mut self, slot: Slot, part: Option<usize>) {
        self.chosen[slot.index()] = part;
    }

    /// A complete default: the first outfit of set 1, first face, hair, bare hands, legwear and shoes.
    pub fn default_for(parts: &[Part]) -> Outfit {
        let mut o = Outfit {
            body: 1,
            chosen: [None; Slot::ALL.len()],
        };
        for slot in [
            Slot::Outfit,
            Slot::Legwear,
            Slot::Gloves,
            Slot::Footwear,
            Slot::Face,
            Slot::Hair,
        ] {
            o.set(
                slot,
                parts
                    .iter()
                    .position(|p| p.slot == slot && (p.body == 0 || p.body == o.body)),
            );
        }
        o
    }

    /// The part paths to load, in slot order.
    pub fn paths<'a>(&self, parts: &'a [Part]) -> Vec<&'a str> {
        self.chosen
            .iter()
            .flatten()
            .filter_map(|&i| parts.get(i))
            .map(|p| p.path.as_str())
            .collect()
    }
}

pub struct Assembled {
    pub model: Model,
    pub skeleton: Skeleton,
    pub baked: HashMap<String, Baked>,
    pub warnings: Vec<String>,
}

/// Loads the parts and puts them on one skeleton. Material names get the part's file name in front
/// ("p0_hrs0000_00_hrs00/hair") so parts with the same material names stay apart; each part's textures are
/// resolved from its own folder. A part that fails to load becomes a warning.
pub fn assemble(ar: &mut Archive, paths: &[&str], lod: usize) -> Result<Assembled> {
    let mut skeleton = skeleton::parse(&ar.read(BASE_SKELETON)?, BASE_SKELETON)?;
    let mut warnings = Vec::new();
    let mut baked = HashMap::new();
    let mut model = Model {
        path: "player".to_string(),
        dir: String::new(),
        materials: HashMap::new(),
        mesh_file: format!("{} parts", paths.len()),
        mesh_files: 1,
        skeleton_file: None,
        submeshes: Vec::new(),
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
    };
    for path in paths {
        // the LOD may not exist for every part; fall back to the most detailed
        let m = match model::load(ar, path, lod).or_else(|_| model::load(ar, path, 0)) {
            Ok(m) => m,
            Err(e) => {
                warnings.push(format!("{path}: {e:#}"));
                continue;
            }
        };
        if let Some(f) = &m.skeleton_file {
            match skeleton::load(ar, &m.dir, f) {
                Ok(s) => warnings.extend(skeleton.attach(&s)),
                Err(e) => warnings.push(format!("{path}: skeleton {f}: {e:#}")),
            }
        }
        let tag = path
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .trim_end_matches(".trmdl");
        let (b, w) = materials::bake(ar, &m);
        warnings.extend(w);
        for (name, bk) in b {
            baked.insert(format!("{tag}/{name}"), bk);
        }
        for (name, mat) in m.materials {
            model.materials.insert(format!("{tag}/{name}"), mat);
        }
        for k in 0..3 {
            model.min[k] = model.min[k].min(m.min[k]);
            model.max[k] = model.max[k].max(m.max[k]);
        }
        model.mesh_files = model.mesh_files.max(m.mesh_files);
        for mut s in m.submeshes {
            s.material = format!("{tag}/{}", s.material);
            model.submeshes.push(s);
        }
    }
    if model.submeshes.is_empty() {
        bail!("no part could be loaded");
    }
    Ok(Assembled {
        model,
        skeleton,
        baked,
        warnings,
    })
}
