//! The cast of a cutscene (step C2 of `.claude/docs/CUTSCENE_PLAN.md`): characters, props and Pokémon placed by
//! its scenes, posed by their cutscene clips.
//!
//! Field slots, checked against the game's files (d010's cast; see `.claude/docs/FORMATS.md`):
//! - An actor is a scene object with a model component; its clips are in an animation container, another object
//!   with a `pe_AnimationContainerComponent` (data slot 0: `.tracn` files; the body's, and a `_secondary` one for
//!   the head). The timeline links the two: `Animation/ChangeCharaCreateAnimation` on the actor's track names
//!   `Container` (`d060.tr0040_friend`) and `FileName` (the `.tracn` stem; `PartsName` "default" = the body).
//!   Pokémon and props use `Animation/AddConfig` (same parameters, `PartName` empty for the body). Without either,
//!   d010's pairs `<code>_Mod` / `<code>_Anm` are used; a Pokémon with no clips at all plays its own idle loop
//!   (`defaultwait01_loop` from its `_base.tracr`; such Pokémon run the game's idle state machine, `idle_int`). The timeline addresses objects as
//!   `<scene>.<object>` (`d010.tr0630_A_Mod`, `d060.tr0040`).
//! - `trinity_CharacterCreationMasterComponent` data slot 15: `[{0: "default", 1: .ccdatam, 2: variant}]`.
//!   `.ccdatam` `{0: [{0: variant, 1: .ccdata (relative), 2: race}]}`.
//! - `trinity_CharacterCreationComponent` (props) data slot 0: a `.ccdata`.
//! - `.ccdata` `{0: [part {0: slot ("default", "bag", "drs", "hdw", "head", "ftw", ...), 2: .trmdl (relative),
//!   3: .trmmt, 5: version, 6: [.tracn]}], 1: body scalers, 3: [variant {0: name ("race00"..), 1: [{0: slot,
//!   1: version, 2: [{0: tag}]}]}]}`. The `.ccdatam` entry's slot 2 names the variant. The `default` part is the
//!   base: `model_cc_base/model/cc_base.trmdl` (a skeleton only) for crowd characters, the whole model for named
//!   ones (`model_uq/tr0080_principal/tr0080_00.trmdl`).
//! - `.trmmt` (a part's material sets) `{2: [version {0: name ("normal", "ver_00"), 1: [.trmtr], 2: [{0: mesh,
//!   1: flags}], 3: [group {0: name ("vis_00", "vis_hdw_00", "discard_03"), 1: material changes, 5: [mesh]}]}]}`.
//!   A variant's `vis_*` tags **hide** their groups' meshes: crowd students' variants hide the hat
//!   (`vis_hdw_00` = every mesh of `hdw00`), the head's hat-hair cap (`vis_03`) and glasses (`vis_00`); shown,
//!   those draw as white domes over the hair. `discard_*` (hair cut by a discard mask) and `color_*` are not
//!   read, nor the version's own `.trmtr` (other colours).
//! - `ti_PokemonModelComponent`: no data; the species is in the object's name (`pm0280_Mod` ->
//!   `pokemon/data/pm0280/pm0280_00_00/pm0280_00_00.trmdl`).
//! - `.tracn` `{0: [{0: kind, 1: file}]}`: `resource` names the `.tracr` clip list next to it.
//! - Which clip plays: `Animation/SetInt int_cutnum = k` on the `_Mod` track starts a cut. Clips are named after
//!   the camera shot they go with (`d010_tr0110_anim_c020_A` with shot `d010_cam_c020`), so the clip whose name
//!   holds the cut id of the shot starting then (`c020`, or `c012_a` -> `c012_A`) is picked; else the k-th of
//!   the clips without `_loop`, sorted by name (true for every cut in d010 that also matches by name); else the
//!   last. `<clip>_loop` follows its clip. `probe --cast` counts the picks of each kind.

use crate::archive::Archive;
use crate::fb::Table;
use crate::skin::{self, Clip, Loaded, Shading};
use crate::{anim, materials, model, player, scene, skeleton};
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use three_d::*;

/// What an actor is built from.
#[derive(Clone, Debug)]
pub enum Source {
    /// A `.ccdata` (characters and props).
    Ccdata(String),
    /// A Pokémon model.
    Pokemon(String),
}

/// One actor as the scenes describe it.
#[derive(Clone, Debug)]
pub struct Spec {
    /// The scene's name (`d010`, `d010_chara_c06`).
    pub scene: String,
    /// The `_Mod` object's name.
    pub object: String,
    pub source: Source,
    /// The `_Mod` object's transform.
    pub placed: Mat4,
    /// The `_Anm` container's `.tracn` files.
    pub tracn: Vec<String>,
}

impl Spec {
    /// The name timeline tracks use.
    pub fn key(&self) -> String {
        format!("{}.{}", self.scene, self.object)
    }
}

/// The actors one scene places, and its animation containers (`<scene>.<object>` -> `.tracn` files).
pub fn specs(
    ar: &Archive,
    scene_name: &str,
    sc: &scene::Scene,
) -> (Vec<Spec>, HashMap<String, Vec<String>>) {
    struct Found {
        name: String,
        placed: Mat4,
        source: Option<Source>,
        tracn: Vec<String>,
    }
    let mut objects: Vec<Found> = Vec::new();
    for top in &sc.nodes {
        top.walk(&mut Vec::new(), &mut |n, parents| {
            let Ok(Some((name, srt))) = n.object() else {
                // a component: give it to the nearest object above
                let Some(owner) = parents.iter().rev().find_map(|p| p.object().ok().flatten())
                else {
                    return;
                };
                let Some(f) = objects.iter_mut().rev().find(|f| f.name == owner.0) else {
                    return;
                };
                let t = n.table();
                match n.kind.as_str() {
                    "trinity_CharacterCreationMasterComponent" => {
                        if let Some(p) = t.and_then(|t| master_ccdata(&t).ok()) {
                            f.source = Some(Source::Ccdata(p));
                        }
                    }
                    "trinity_CharacterCreationComponent" => {
                        if let Some(p) = t.and_then(|t| t.string(0).ok().flatten()) {
                            f.source = Some(Source::Ccdata(p));
                        }
                    }
                    "ti_PokemonModelComponent" => {
                        if let Some(p) = pokemon_model(ar, &f.name) {
                            f.source = Some(Source::Pokemon(p));
                        }
                    }
                    "pe_AnimationContainerComponent" => {
                        if let Some(v) = t.and_then(|t| t.strings(0).ok()) {
                            f.tracn = v;
                        }
                    }
                    _ => {}
                }
                return;
            };
            // objects nest: chain the parents' transforms
            let mut m = srt.matrix();
            for p in parents.iter().rev() {
                if let Ok(Some((_, ps))) = p.object() {
                    m = ps.matrix() * m;
                }
            }
            objects.push(Found {
                name,
                placed: m,
                source: None,
                tracn: Vec::new(),
            });
        });
    }
    let mut out = Vec::new();
    for f in &objects {
        let Some(source) = &f.source else {
            continue;
        };
        // the `_Mod` / `_Anm` pair, if named so
        let tracn = f
            .name
            .strip_suffix("_Mod")
            .and_then(|code| objects.iter().find(|o| o.name == format!("{code}_Anm")))
            .map(|o| o.tracn.clone())
            .unwrap_or_default();
        out.push(Spec {
            scene: scene_name.to_string(),
            object: f.name.clone(),
            source: source.clone(),
            placed: f.placed,
            tracn,
        });
    }
    let containers = objects
        .iter()
        .filter(|o| !o.tracn.is_empty())
        .map(|o| (format!("{scene_name}.{}", o.name), o.tracn.clone()))
        .collect();
    (out, containers)
}

/// What a CharacterCreationMasterComponent picks, as `<.ccdatam>#<variant>`: slot 15's first entry names a
/// `.ccdatam` and a variant, which `ccdatam_pick` maps to a `.ccdata` when the actor loads.
fn master_ccdata(t: &Table) -> Result<String> {
    let entry = t
        .tables(15)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no character entry"))?;
    let ccdatam = entry.string(1)?.ok_or_else(|| anyhow!("no .ccdatam"))?;
    let variant = entry.string(2)?.unwrap_or_default();
    Ok(format!("{ccdatam}#{variant}"))
}

/// `pm0280_Mod` -> the species' first model that exists (`pm0280_00_00`). `pm0426_429_Mod` -> `pm0426`.
fn pokemon_model(ar: &Archive, object: &str) -> Option<String> {
    let digits: String = object
        .strip_prefix("pm")?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.len() != 4 {
        return None;
    }
    ["00_00", "00_01", "01_00"]
        .iter()
        .map(|f| format!("pokemon/data/pm{digits}/pm{digits}_{f}/pm{digits}_{f}.trmdl"))
        .find(|p| ar.contains(p))
}

/// A `.ccdatam` entry's `.ccdata` and variant (race): the entry named `variant`, else the first.
fn ccdatam_pick(ar: &mut Archive, path: &str, variant: &str) -> Result<(String, String)> {
    let b = ar.read(path)?;
    let root = Table::root(&b, 0)?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    let entries = root.tables(0)?;
    let e = entries
        .iter()
        .find(|e| e.string(0).ok().flatten().as_deref() == Some(variant))
        .or(entries.first())
        .ok_or_else(|| anyhow!("empty .ccdatam"))?;
    let rel = e.string(1)?.ok_or_else(|| anyhow!("no .ccdata"))?;
    Ok((model::resolve(dir, &rel), e.string(2)?.unwrap_or_default()))
}

/// One part of a `.ccdata`.
pub struct Part {
    pub slot: String,
    pub model: String,
    pub trmmt: Option<String>,
    pub version: String,
}

/// A `.ccdata`'s parts, and its variants: name -> [(slot, version, tags)].
pub type Variants = HashMap<String, Vec<(String, String, Vec<String>)>>;

pub fn ccdata(ar: &mut Archive, path: &str) -> Result<(Vec<Part>, Variants)> {
    let b = ar.read(path)?;
    let root = Table::root(&b, 0)?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    let mut parts = Vec::new();
    for p in root.tables(0)? {
        let Some(m) = p.string(2)? else { continue };
        let text = |i: usize| p.string(i).ok().flatten();
        parts.push(Part {
            slot: text(0).unwrap_or_default(),
            model: model::resolve(dir, &m),
            trmmt: text(3).map(|t| model::resolve(dir, &t)),
            version: text(5).unwrap_or_default(),
        });
    }
    // variants are optional: entries that do not read as expected are left out
    let mut variants = Variants::new();
    for v in root.tables(3).unwrap_or_default() {
        let Some(name) = v.string(0).ok().flatten() else {
            continue;
        };
        let mut per = Vec::new();
        for e in v.tables(1).unwrap_or_default() {
            let (Some(slot), version) = (e.string(0).ok().flatten(), e.string(1).ok().flatten())
            else {
                continue;
            };
            // tags: a vector of {0: name} tables (or of strings)
            let tags = match e.tables(2) {
                Ok(t) if t.iter().all(|t| t.string(0).ok().flatten().is_some()) => t
                    .iter()
                    .filter_map(|t| t.string(0).ok().flatten())
                    .collect(),
                _ => e.strings(2).unwrap_or_default(),
            };
            per.push((slot, version.unwrap_or_default(), tags));
        }
        variants.insert(name, per);
    }
    if parts.is_empty() {
        bail!("no parts");
    }
    Ok((parts, variants))
}

/// A `.trmmt`'s mesh groups for one version (the version named, else the first): group -> meshes.
pub fn trmmt_groups(
    ar: &mut Archive,
    path: &str,
    version: &str,
) -> Result<HashMap<String, Vec<String>>> {
    let b = ar.read(path)?;
    let root = Table::root(&b, 0)?;
    let versions = root.tables(2)?;
    let v = versions
        .iter()
        .find(|v| v.string(0).ok().flatten().as_deref() == Some(version))
        .or(versions.first())
        .ok_or_else(|| anyhow!("no versions"))?;
    let mut out = HashMap::new();
    for g in v.tables(3).unwrap_or_default() {
        let Some(name) = g.string(0).ok().flatten() else {
            continue;
        };
        let meshes = g
            .tables(5)
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m.string(0).ok().flatten())
            .collect();
        out.insert(name, meshes);
    }
    Ok(out)
}

/// The meshes a variant hides, as (part model file stem, mesh name).
fn hidden_meshes(
    ar: &mut Archive,
    parts: &[Part],
    variant: &[(String, String, Vec<String>)],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (slot, version, tags) in variant {
        let Some(part) = parts.iter().find(|p| &p.slot == slot) else {
            continue;
        };
        let Some(trmmt) = &part.trmmt else { continue };
        let version = if version.is_empty() {
            &part.version
        } else {
            version
        };
        let Ok(groups) = trmmt_groups(ar, trmmt, version) else {
            continue;
        };
        let stem = part
            .model
            .rsplit('/')
            .next()
            .unwrap_or(&part.model)
            .trim_end_matches(".trmdl")
            .to_string();
        for t in tags.iter().filter(|t| t.starts_with("vis")) {
            for m in groups.get(t).into_iter().flatten() {
                out.push((stem.clone(), m.clone()));
            }
        }
    }
    out
}

/// The clips a `.tracn` offers, as cuts: per cut (sorted by name, `_loop` clips left out) the clip and its loop.
pub fn cut_clips(ar: &mut Archive, tracn: &str) -> Result<Vec<(String, Option<String>)>> {
    let b = ar.read(tracn)?;
    let root = Table::root(&b, 0)?;
    let dir = tracn.rsplit_once('/').map_or("", |(d, _)| d);
    let mut tracr = None;
    for e in root.tables(0)? {
        if e.string(0)?.as_deref() == Some("resource") {
            tracr = e.string(1)?;
        }
    }
    let tracr = model::resolve(
        dir,
        &tracr.ok_or_else(|| anyhow!("no resource in {tracn}"))?,
    );
    let list = anim::list_at(ar, &tracr, "");
    let mut main: Vec<&(String, String)> =
        list.iter().filter(|(l, _)| !l.ends_with("_loop")).collect();
    main.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(main
        .into_iter()
        .map(|(l, p)| {
            let looped = list
                .iter()
                .find(|(m, _)| *m == format!("{l}_loop"))
                .map(|(_, p)| p.clone());
            (p.clone(), looped)
        })
        .collect())
}

/// An actor on the GPU.
pub struct Actor {
    pub loaded: Loaded,
    pub skeleton: Option<skeleton::Skeleton>,
    /// Per cut: the clip and its loop (loaded on first use).
    cuts: Vec<(String, Option<String>)>,
    clips: HashMap<String, Option<Clip>>,
    pub warnings: Vec<String>,
}

/// An actor read from the archive, before it goes to the GPU.
pub struct Prepared {
    pub model: model::Model,
    pub skeleton: Option<skeleton::Skeleton>,
    pub baked: HashMap<String, materials::Baked>,
    pub cuts: Vec<(String, Option<String>)>,
    pub warnings: Vec<String>,
}

/// Loads an actor: its model (assembled from its parts for characters), then puts it on the GPU.
pub fn load(context: &three_d::Context, ar: &mut Archive, spec: &Spec) -> Result<Actor> {
    let p = prepare(ar, spec)?;
    Ok(Actor {
        loaded: skin::build(
            context,
            p.model,
            p.baked,
            Vec::new(),
            Shading::Textured,
            true,
        ),
        skeleton: p.skeleton,
        cuts: p.cuts,
        clips: HashMap::new(),
        warnings: p.warnings,
    })
}

/// Reads an actor's model, skeleton, textures and cut list.
pub fn prepare(ar: &mut Archive, spec: &Spec) -> Result<Prepared> {
    let mut warnings = Vec::new();
    let (model, skel, baked) = match &spec.source {
        Source::Pokemon(path) => {
            let m = model::load(ar, path, 0)?;
            let skel = match &m.skeleton_file {
                Some(f) => match skeleton::load(ar, &m.dir, f) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        warnings.push(format!("{path}: skeleton: {e:#}"));
                        None
                    }
                },
                None => None,
            };
            let (baked, w) = materials::bake(ar, &m);
            warnings.extend(w);
            (m, skel, baked)
        }
        Source::Ccdata(p) => {
            let (path, race) = match p.split_once('#') {
                Some((ccdatam, variant)) => ccdatam_pick(ar, ccdatam, variant)?,
                None => (p.clone(), String::new()),
            };
            let (all, variants) = ccdata(ar, &path)?;
            // the variant the .ccdatam names, else the first one listed
            let variant = variants
                .get(&race)
                .or_else(|| variants.values().next())
                .cloned()
                .unwrap_or_default();
            let hidden = hidden_meshes(ar, &all, &variant);
            let parts: Vec<(String, String)> = all
                .iter()
                .map(|p| (p.slot.clone(), p.model.clone()))
                .collect();
            // the default part's skeleton is the base the other parts hang from
            let base_model = parts
                .iter()
                .find(|(s, _)| s == "default")
                .map(|(_, m)| m.clone())
                .unwrap_or_else(|| parts[0].1.clone());
            let base_skel = base_skeleton(ar, &base_model)?;
            let paths: Vec<&str> = parts.iter().map(|(_, m)| m.as_str()).collect();
            let mut a = player::assemble_on(ar, &base_skel, &spec.object, &paths, 0)?;
            // materials are named "<part stem>/<material>" by the assembly
            a.model.submeshes.retain(|s| {
                let part = s.material.split('/').next().unwrap_or("");
                // LOD shapes carry a suffix (`_lod1`), so compare the start of the name too
                !hidden.iter().any(|(stem, mesh)| {
                    stem == part && (s.shape == *mesh || s.shape.starts_with(mesh.as_str()))
                })
            });
            // a skeleton-only base model has no meshes: not worth a warning
            warnings.extend(
                a.warnings
                    .into_iter()
                    .filter(|w| !w.contains("cc_base.trmdl")),
            );
            (a.model, Some(a.skeleton), a.baked)
        }
    };
    let mut cuts = match spec.tracn.first() {
        Some(t) => cut_clips(ar, t).unwrap_or_else(|e| {
            warnings.push(format!("{t}: {e:#}"));
            Vec::new()
        }),
        None => Vec::new(),
    };
    // a Pokémon without cutscene clips: its own idle loop, as the only cut
    if cuts.is_empty()
        && let Source::Pokemon(path) = &spec.source
    {
        let list = anim::list(ar, path);
        if let Some((_, p)) = list
            .iter()
            .find(|(l, _)| l.contains("defaultwait01_loop"))
            .or(list.first())
        {
            cuts.push((p.clone(), Some(p.clone())));
        }
    }
    Ok(Prepared {
        model,
        skeleton: skel,
        baked,
        cuts,
        warnings,
    })
}

/// The skeleton file of a base model (`cc_base.trmdl` -> `cc_base.trskl`).
fn base_skeleton(ar: &mut Archive, model_path: &str) -> Result<String> {
    // read only the model's header: the skeleton name is in the .trmdl itself
    let m = model::load(ar, model_path, 0);
    if let Ok(m) = &m
        && let Some(f) = &m.skeleton_file
    {
        return Ok(model::resolve(&m.dir, f));
    }
    // skeleton-only models may not load as models; their skeleton sits next to them
    let guess = model_path.replace(".trmdl", ".trskl");
    if ar.contains(&guess) {
        return Ok(guess);
    }
    Err(m
        .err()
        .unwrap_or_else(|| anyhow!("{model_path}: no skeleton")))
}

/// How a clip was picked for a cut (`pick_cut`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Pick {
    /// The clip's name holds the shot's cut id.
    ByShot,
    /// The k-th clip.
    ByNumber,
    /// Neither: the last clip.
    Last,
}

/// The clip (index into `cuts`) for cut `k` (1-based) while shot `shot` (`d010_cam_c012_a`) starts.
pub fn pick_cut(
    cuts: &[(String, Option<String>)],
    k: usize,
    shot: Option<&str>,
) -> Option<(usize, Pick)> {
    if cuts.is_empty() {
        return None;
    }
    let stem = |p: &str| -> String {
        p.rsplit('/')
            .next()
            .unwrap_or(p)
            .trim_end_matches(".tranm")
            .to_lowercase()
    };
    if let Some(id) = shot.and_then(|s| {
        s.to_lowercase()
            .split_once("_cam_")
            .map(|(_, id)| id.to_string())
    }) {
        // the full id first (`c012_a`), then its cut number alone (`c012`)
        let base = id.split('_').next().unwrap_or(&id).to_string();
        for want in [id.as_str(), base.as_str()] {
            let tag = format!("_{want}");
            if let Some(i) = cuts.iter().position(|(m, _)| {
                let s = stem(m);
                s.ends_with(&tag) || s.contains(&format!("{tag}_"))
            }) {
                return Some((i, Pick::ByShot));
            }
        }
    }
    if (1..=cuts.len()).contains(&k) {
        return Some((k - 1, Pick::ByNumber));
    }
    Some((cuts.len() - 1, Pick::Last))
}

impl Actor {
    fn clip(&mut self, ar: &mut Archive, path: &str) -> Option<&mut Clip> {
        if !self.clips.contains_key(path) {
            let c = anim::load(ar, path, "")
                .ok()
                .map(|a| Clip::new(a, self.skeleton.as_ref()));
            if c.is_none() {
                self.warnings.push(format!("{path}: could not be loaded"));
            }
            self.clips.insert(path.to_string(), c);
        }
        self.clips.get_mut(path)?.as_mut()
    }

    /// Poses the actor: cut `cut` (1-based, see `pick_cut`), begun while shot `shot` started, `seconds` into it,
    /// at `at`.
    pub fn pose(
        &mut self,
        ar: &mut Archive,
        cut: usize,
        shot: Option<&str>,
        seconds: f32,
        at: Mat4,
    ) {
        if let Some((main, looped)) =
            pick_cut(&self.cuts, cut, shot).and_then(|(i, _)| self.cuts.get(i).cloned())
        {
            let mut pick = None;
            if let Some(c) = self.clip(ar, &main) {
                let length = c.anim.frames.max(2) as f32 / c.anim.fps as f32;
                if seconds <= length || looped.is_none() {
                    c.frame = (seconds * c.anim.fps as f32).min((c.anim.frames.max(2) - 1) as f32);
                    pick = Some(main.clone());
                } else if let Some(l) = &looped {
                    let rest = seconds - length;
                    if let Some(lc) = self.clip(ar, l) {
                        let len = (lc.anim.frames.max(2) - 1) as f32;
                        lc.frame = (rest * lc.anim.fps as f32) % len;
                        pick = Some(l.clone());
                    }
                }
            }
            if let (Some(p), Some(s)) = (pick, &self.skeleton)
                && let Some(Some(c)) = self.clips.get(&p)
            {
                let world = s.posed_world(&c.locals(s));
                skin::apply_skin(&mut self.loaded, &s.skin_matrices(&world));
            }
        }
        for gm in self.loaded.meshes.iter_mut() {
            gm.set_transformation(at);
        }
    }

    pub fn objects(&self) -> impl Iterator<Item = &dyn Object> {
        self.loaded.meshes.iter().map(|m| m as &dyn Object)
    }
}
