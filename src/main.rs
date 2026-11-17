//! Trinity model viewer: browse the Pokémon, trainer and prop models in a dumped RomFS and look at them in a simple
//! 3D scene, with textures, skeletons and animations. No game data comes with it: the user picks their own RomFS dump.
//!
//!   cargo run --release                 opens the viewer; asks for the RomFS folder the first time
//!   cargo run --release -- <romfs dir>  uses that RomFS folder (and remembers it)
//!   cargo run --release -- --check      loads every model once and prints failures, without a window
//!   cargo run --release -- --model pm0006_00_00   starts with that model selected
//!   cargo run --release -- --player               starts with the player character (mix and match its parts)
//!   cargo run --release -- --skeleton             starts with the skeleton shown
//!   cargo run --release -- --lit                  starts in Lit mode (one sun with shadows)
//!   cargo run --release -- --anim walk01          starts the first animation whose name contains that text
//!
//! Mouse: left drag orbits, right drag pans (`world::pan`), wheel zooms.
//! F7: play mode (see demo.rs): WASD or arrows move, Shift runs, Space jumps; F7 or Esc leaves it.

mod anim;
mod archive;
mod audio;
mod config;
mod demo;
mod eye;
mod fb;
mod gallery;
mod instances;
mod maps;
mod materials;
mod model;
mod motion;
mod msg;
mod plants;
mod player;
mod scene;
mod skeleton;
mod sound_names;
mod terrain;
mod texture;
mod textures;
mod water;
mod wem;
mod world;
mod wwise;

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::path::PathBuf;
use three_d::*;

/// The RomFS folder: the command-line argument if given, else the one picked last time.
fn romfs_path(arg: Option<&str>) -> Option<PathBuf> {
    arg.map(PathBuf::from).or_else(config::load_romfs_path)
}

/// Opens the native folder picker.
fn pick_romfs_folder() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Choose your dumped RomFS folder (the one with the \"arc\" folder)")
        .pick_folder()
}

/// "pokemon/data/pm0025/pm0025_00_00/pm0025_00_00.trmdl" -> "pm0025_00_00";
/// "chara/model_tr/tr0330_karate/tr0330_00.trmdl" -> "tr0330_karate/tr0330_00" (the folder carries the name).
fn short_name(path: &str) -> &str {
    let path = path.strip_suffix(".trmdl").unwrap_or(path);
    if let Some(rest) = path.strip_prefix("chara/") {
        return rest.split_once('/').map_or(rest, |(_, r)| r);
    }
    path.rsplit('/').next().unwrap_or(path)
}

/// The model lists in the left panel.
#[derive(Clone, Copy, PartialEq)]
enum Category {
    Pokemon,
    Trainers,
    Props,
    /// Not a list: the player character, assembled from the parts picked in the panel.
    Player,
    /// Not a list: the overworld terrain (see `world.rs`).
    World,
    /// Not a model: every texture and sprite, by group (see `gallery.rs`).
    Textures,
    /// Not a model: music and sound effects (see `audio.rs`).
    Sound,
}

impl Category {
    const ALL: [Category; 7] = [
        Category::Pokemon,
        Category::Trainers,
        Category::Props,
        Category::Player,
        Category::World,
        Category::Textures,
        Category::Sound,
    ];

    fn of(path: &str) -> Category {
        if path.starts_with("chara/model_ob/") {
            Category::Props
        } else if path.starts_with("chara/") {
            Category::Trainers
        } else {
            Category::Pokemon
        }
    }

    fn label(self) -> &'static str {
        match self {
            Category::Pokemon => "Pokémon",
            Category::Trainers => "Trainers",
            Category::Props => "Props",
            Category::Player => "Player",
            Category::World => "World",
            Category::Textures => "Textures",
            Category::Sound => "Sound",
        }
    }

    /// Tabs that fill the window with panels instead of the 3D view.
    fn flat(self) -> bool {
        matches!(self, Category::Textures | Category::Sound)
    }

    fn search_hint(self) -> &'static str {
        match self {
            Category::Pokemon => "search, e.g. pm0025 or 0006",
            Category::Trainers => "search, e.g. karate or tr0080",
            Category::Props => "search, e.g. phone or ob0001",
            Category::Player | Category::World | Category::Textures | Category::Sound => "",
        }
    }
}

/// A model ready to show, from the list or assembled from player parts. `skel` and `anims` are `None` when the
/// current ones stay (a LOD change).
struct Prepared {
    model: model::Model,
    skel: Option<Result<skeleton::Skeleton, String>>,
    anims: Option<Vec<(String, String)>>,
    baked: HashMap<String, materials::Baked>,
    warnings: Vec<String>,
    /// A trainer's or NPC's motions (`None` for Pokémon, props and the player); only with a fresh skeleton.
    motion: Option<CharMotion>,
}

/// A human character's motions: what was found for it, and the body type whose clips are listed (`None`: its
/// own list).
struct CharMotion {
    found: motion::Found,
    body: Option<String>,
}

/// Where the clips of trainers and NPCs are looked up: the motion folders and the shared body types.
#[derive(Default)]
struct Motions {
    folders: Vec<motion::Folder>,
    bodies: Vec<String>,
}

impl Motions {
    fn new(ar: &archive::Archive) -> Self {
        let folders = motion::folders(ar);
        let bodies = motion::body_types(&folders);
        Motions { folders, bodies }
    }
}

/// Loads a listed model; with `fresh`, also its skeleton and animation list. The NPC parts of chara/model_vr name
/// no skeleton and are put on the shared base skeleton; trainers and NPCs get their clips from `motion`.
fn load_listed(
    arc: &mut archive::Archive,
    motions: &Motions,
    path: &str,
    lod: usize,
    fresh: bool,
) -> Result<Prepared> {
    let (m, skel, baked, warnings) = if motion::is_vr_part(path) {
        let a = player::assemble_on(arc, motion::VR_BASE_SKELETON, path, &[path], lod)?;
        (
            a.model,
            fresh.then_some(Ok(a.skeleton)),
            a.baked,
            a.warnings,
        )
    } else {
        let m = model::load(arc, path, lod)?;
        let skel = fresh.then(|| match &m.skeleton_file {
            Some(f) => skeleton::load(arc, &m.dir, f).map_err(|e| format!("{e:#}")),
            None => Err("the model names no skeleton".into()),
        });
        let (baked, warnings) = materials::bake(arc, &m);
        (m, skel, baked, warnings)
    };
    let (anims, motion) = if !fresh {
        (None, None)
    } else if motion::is_human(path) {
        let found = motion::find(arc, &motions.folders, path);
        let body = motion::default_body(&found, path, &motions.bodies);
        let clips = motion::clips(arc, &found, body.as_deref());
        (Some(clips), Some(CharMotion { found, body }))
    } else {
        (Some(anim::list(arc, &m.path)), None)
    };
    Ok(Prepared {
        model: m,
        skel,
        anims,
        baked,
        warnings,
        motion,
    })
}

/// Assembles the player from the chosen parts, with its skeleton and animation list.
fn load_player(
    arc: &mut archive::Archive,
    parts: &[player::Part],
    outfit: &player::Outfit,
    lod: usize,
) -> Result<Prepared> {
    let a = player::assemble(arc, &outfit.paths(parts), lod)?;
    Ok(Prepared {
        model: a.model,
        skel: Some(Ok(a.skeleton)),
        anims: Some(anim::list_at(arc, player::ANIM_LIST, player::ANIM_STEM)),
        baked: a.baked,
        warnings: a.warnings,
        motion: None,
    })
}

/// A small random number generator for "Random outfit" (xorshift, seeded from the clock).
fn random_below(state: &mut u64, n: usize) -> usize {
    if *state == 0 {
        *state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9_7f4a_7c15, |d| d.as_nanos() as u64)
            | 1;
    }
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state % n.max(1) as u64) as usize
}

/// The player panel: outfit set, one drop-down per slot with previous / next buttons, Random and Default.
/// Returns true when the choice changed.
fn player_panel(
    ui: &mut egui::Ui,
    parts: &[player::Part],
    outfit: &mut player::Outfit,
    rng: &mut u64,
) -> bool {
    use player::Slot;
    let before = outfit.clone();
    let options = |slot: Slot, body: u8| -> Vec<usize> {
        (0..parts.len())
            .filter(|&i| parts[i].slot == slot && (parts[i].body == 0 || parts[i].body == body))
            .collect()
    };
    ui.horizontal(|ui| {
        ui.label("Outfit set");
        let old = outfit.body;
        ui.radio_value(&mut outfit.body, 1, "1")
            .on_hover_text("Outfits made for body 1 (p1_ files)");
        ui.radio_value(&mut outfit.body, 2, "2")
            .on_hover_text("Outfits made for body 2 (p2_ files)");
        if outfit.body != old {
            // keep the same outfit from the other set when it has one
            let label = outfit.get(Slot::Outfit).map(|i| parts[i].label.clone());
            let list = options(Slot::Outfit, outfit.body);
            let same = list
                .iter()
                .copied()
                .find(|&i| Some(&parts[i].label) == label.as_ref());
            outfit.set(Slot::Outfit, same.or(list.first().copied()));
        }
    });
    egui::Grid::new("player_parts")
        .num_columns(2)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            for slot in Slot::ALL {
                let list = options(slot, outfit.body);
                if list.is_empty() {
                    continue;
                }
                ui.label(slot.label());
                ui.horizontal(|ui| {
                    let current = outfit.get(slot);
                    // position in the list: None first, then the parts
                    let pos = current
                        .and_then(|c| list.iter().position(|&i| i == c))
                        .map_or(0, |p| p + 1);
                    let step = |d: isize| {
                        let n = list.len() as isize + 1;
                        let p = (pos as isize + d).rem_euclid(n) as usize;
                        (p > 0).then(|| list[p - 1])
                    };
                    let mut v = current;
                    if ui.small_button("◀").clicked() {
                        v = step(-1);
                    }
                    let text = current.map_or("None", |i| parts[i].label.as_str());
                    egui::ComboBox::from_id_salt(slot.label())
                        .selected_text(text)
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut v, None, "None");
                            for &i in &list {
                                ui.selectable_value(&mut v, Some(i), &parts[i].label);
                            }
                        });
                    if ui.small_button("▶").clicked() {
                        v = step(1);
                    }
                    if v != current {
                        outfit.set(slot, v);
                    }
                });
                ui.end_row();
            }
        });
    ui.horizontal(|ui| {
        if ui
            .button("Random")
            .on_hover_text(
                "A random part in every slot that has one (headwear, eyewear, bag: sometimes none)",
            )
            .clicked()
        {
            outfit.body = 1 + random_below(rng, 2) as u8;
            for slot in Slot::ALL {
                let list = options(slot, outfit.body);
                let optional = matches!(
                    slot,
                    Slot::Headwear | Slot::Eyewear | Slot::Bag | Slot::Accessory
                );
                let pick = if list.is_empty() || (optional && random_below(rng, 2) == 0) {
                    None
                } else {
                    Some(list[random_below(rng, list.len())])
                };
                outfit.set(slot, pick);
            }
        }
        if ui.button("Default").clicked() {
            *outfit = player::Outfit::default_for(parts);
        }
    });
    *outfit != before
}

/// Every model the viewer lists: Pokémon, then trainers and props.
fn all_models(ar: &archive::Archive) -> Vec<String> {
    let mut models = ar.pokemon_models();
    models.extend(ar.character_models());
    models
}

/// A three-d material for a Trinity alpha mode: blended ones are drawn after the opaque ones, back to front, and do
/// not write depth (three-d's transparent pass).
fn transparent_material(
    context: &Context,
    m: &CpuMaterial,
    alpha: materials::AlphaMode,
) -> PhysicalMaterial {
    use materials::AlphaMode;
    let blend = match alpha {
        AlphaMode::Opaque | AlphaMode::Cutout(_) => {
            return PhysicalMaterial::new_opaque(context, m);
        }
        AlphaMode::Blend => Blend::TRANSPARENCY,
        AlphaMode::Add => Blend::Enabled {
            source_rgb_multiplier: BlendMultiplierType::SrcAlpha,
            source_alpha_multiplier: BlendMultiplierType::Zero,
            destination_rgb_multiplier: BlendMultiplierType::One,
            destination_alpha_multiplier: BlendMultiplierType::One,
            rgb_equation: BlendEquationType::Add,
            alpha_equation: BlendEquationType::Add,
        },
        AlphaMode::Multiply => Blend::Enabled {
            source_rgb_multiplier: BlendMultiplierType::DstColor,
            source_alpha_multiplier: BlendMultiplierType::Zero,
            destination_rgb_multiplier: BlendMultiplierType::Zero,
            destination_alpha_multiplier: BlendMultiplierType::One,
            rgb_equation: BlendEquationType::Add,
            alpha_equation: BlendEquationType::Add,
        },
    };
    let mut pm = PhysicalMaterial::new_transparent(context, m);
    pm.render_states.blend = blend;
    pm
}

/// A stable, soft colour per material name.
fn material_color(name: &str) -> Srgba {
    let h = archive::fnv1a64(name);
    let hue = (h % 360) as f32;
    let (s, l) = (0.45, 0.62);
    let c = (1.0 - (2.0 * l - 1.0f32).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to8 = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Srgba::new(to8(r), to8(g), to8(b), 255)
}

fn check(ar: &mut archive::Archive) {
    let models = all_models(ar);
    let mut failed = 0;
    for m in &models {
        if let Err(e) = model::load(ar, m, 0) {
            failed += 1;
            println!("FAIL {m}: {e:#}");
        }
    }
    println!(
        "{} models, {} loaded, {failed} failed",
        models.len(),
        models.len() - failed
    );
    // trainers and NPCs as the viewer opens them: skeleton (model_vr parts on the base skeleton) and clip list
    let motions = Motions::new(ar);
    let (mut humans, mut no_skel, mut no_clips) = (0, 0, 0);
    for m in models.iter().filter(|m| motion::is_human(m)) {
        humans += 1;
        match load_listed(ar, &motions, m, 0, true) {
            Ok(p) => {
                if let Some(Err(e)) = &p.skel {
                    no_skel += 1;
                    println!("NO SKELETON {m}: {e}");
                } else if p.anims.as_ref().is_none_or(|a| a.is_empty()) {
                    no_clips += 1;
                    println!("NO CLIPS {m}");
                }
            }
            Err(e) => println!("FAIL {m}: {e:#}"),
        }
    }
    println!("{humans} trainers and NPCs: {no_skel} without a skeleton, {no_clips} without clips");
    // every map's tiles (Paldea, Kitakami, the Terarium)
    let tiles: Vec<(i32, i32, String)> = maps::overworlds()
        .into_iter()
        .filter_map(|m| m.grid)
        .flat_map(|g| g.tiles(ar))
        .collect();
    let mut failed = 0;
    let mut layers_checked = false;
    for (_, _, path) in &tiles {
        // heights, then the splat material: the tile's control map and (once) the shared ground layers
        let result = terrain::load_heights(ar, path).and_then(|_| {
            let m = terrain::tile_material(ar, path)?;
            texture::decode_bntx(&ar.read(&m.splat_alpha)?)?;
            if !layers_checked {
                let l = terrain::load_layers(ar, &m)?;
                println!(
                    "{} ground texture layers, {} normal layers",
                    l.colors.len(),
                    l.normals.len()
                );
                if l.normals.len() != l.colors.len() {
                    anyhow::bail!("the normal layers did not load");
                }
                layers_checked = true;
            }
            Ok(())
        });
        if let Err(e) = result {
            failed += 1;
            println!("FAIL {path}: {e:#}");
        }
    }
    println!(
        "{} terrain tiles, {} loaded, {failed} failed",
        tiles.len(),
        tiles.len() - failed
    );
    // the tiles' trees: every listed instance file parses and every listed model loads
    let (mut trees, mut tree_failed) = (0, 0);
    let mut models: HashMap<String, bool> = HashMap::new();
    for (_, _, path) in &tiles {
        let list = match terrain::tile_instances(ar, path) {
            Ok(l) => l,
            Err(e) => {
                tree_failed += 1;
                println!("FAIL {path} tree list: {e:#}");
                continue;
            }
        };
        for (model_path, files) in list {
            for f in files {
                match ar.read(&f).and_then(|b| instances::parse(&b)) {
                    Ok(ins) => trees += ins.matrices.len(),
                    Err(e) => {
                        tree_failed += 1;
                        println!("FAIL {f}: {e:#}");
                    }
                }
            }
            if let std::collections::hash_map::Entry::Vacant(v) = models.entry(model_path) {
                let ok = match model::load(ar, v.key(), 0) {
                    Ok(_) => true,
                    Err(e) => {
                        tree_failed += 1;
                        println!("FAIL {}: {e:#}", v.key());
                        false
                    }
                };
                v.insert(ok);
            }
        }
    }
    println!(
        "{trees} trees of {} models on the tiles, {tree_failed} failures",
        models.len()
    );
    // the streamed scenes of the towns and tiles: parse, every model loads, every instance file parses
    let scenes = maps::field_scenes(ar);
    let (mut placed, mut instanced, mut scene_failed) = (0, 0, 0);
    for (label, path) in &scenes {
        let c = match scene::load_content(ar, path) {
            Ok(c) => c,
            Err(e) => {
                scene_failed += 1;
                println!("FAIL scene {label}: {e:#}");
                continue;
            }
        };
        for e in &c.errors {
            scene_failed += 1;
            println!("FAIL scene {label}: {e}");
        }
        placed += c.placements.len();
        let mut wanted: Vec<String> = c.placements.into_iter().map(|p| p.model).collect();
        for scene::Instancer { model, file: f, .. } in c.instancers {
            match ar.read(&f).and_then(|b| instances::parse(&b)) {
                Ok(ins) => instanced += ins.matrices.len(),
                Err(e) => {
                    scene_failed += 1;
                    println!("FAIL {f}: {e:#}");
                }
            }
            wanted.push(model);
        }
        for m in wanted {
            if let std::collections::hash_map::Entry::Vacant(v) = models.entry(m) {
                let ok = match model::load(ar, v.key(), 0) {
                    Ok(_) => true,
                    Err(e) => {
                        scene_failed += 1;
                        println!("FAIL {}: {e:#}", v.key());
                        false
                    }
                };
                v.insert(ok);
            }
        }
    }
    println!(
        "{} streamed scenes: {placed} placed objects, {instanced} instances, {} models in all, {scene_failed} failures",
        scenes.len(),
        models.len()
    );
    // every map's resident scenes (water, outer scenery) and every room, with their sub-scenes: each placed
    // model loads; "Water" parts counted
    let mut resident: Vec<String> = maps::overworlds()
        .into_iter()
        .flat_map(|m| m.resident)
        .collect();
    let rooms = maps::rooms(ar);
    resident.extend(rooms.iter().map(|(_, p)| p.clone()));
    let (mut water_parts, mut placed, mut res_failed) = (0, 0, 0);
    for path in &resident {
        let c = match scene::load_content(ar, path) {
            Ok(c) => c,
            Err(e) => {
                res_failed += 1;
                println!("FAIL scene {path}: {e:#}");
                continue;
            }
        };
        for e in &c.errors {
            res_failed += 1;
            println!("FAIL {path}: {e}");
        }
        placed += c.placements.len();
        for p in c.placements {
            match model::load(ar, &p.model, 0) {
                Ok(m) => {
                    water_parts += m
                        .submeshes
                        .iter()
                        .filter(|x| {
                            m.materials
                                .get(&x.material)
                                .is_some_and(|y| y.shader == "Water")
                        })
                        .count()
                }
                Err(e) => {
                    res_failed += 1;
                    println!("FAIL {}: {e:#}", p.model);
                }
            }
        }
    }
    println!(
        "{} resident and room scenes ({} rooms): {placed} placed models, {water_parts} water parts, {res_failed} failures",
        resident.len(),
        rooms.len()
    );
}

#[derive(Clone, Copy, PartialEq)]
enum Shading {
    Textured,
    MaterialColours,
    Plain,
}

/// Unlit: soft light from all round, every side readable. Lit: one sun with shadows, so form and height show.
#[derive(Clone, Copy, PartialEq)]
enum Lighting {
    Unlit,
    Lit,
}

/// Shadow map size of the sun in Lit mode.
const SHADOW_MAP_SIZE: u32 = 2048;

struct Loaded {
    model: model::Model,
    baked: HashMap<String, materials::Baked>,
    warnings: Vec<String>,
    meshes: Vec<Gm<Mesh, eye::ModelMaterial>>,
    /// Bind-pose vertex data of each mesh in `meshes`, for skinning; `None` for parts without skin weights.
    skin: Vec<Option<SkinPart>>,
    floor: Gm<Mesh, PhysicalMaterial>,
}

struct SkinPart {
    /// Parts of the same shape share their vertices, so their positions and normals are skinned once per frame.
    shape: String,
    positions: Vec<Vec3>,
    normals: Option<Vec<Vec3>>,
    /// Tangents are this part's own: they are computed from its triangles only (NaN at the shape's other
    /// vertices), and the handedness can differ between parts of one shape, so they are never shared.
    tangents: Option<Vec<Vec4>>,
    joints: Vec<[u8; 4]>,
    weights: Vec<[f32; 4]>,
}

impl SkinPart {
    /// Linear blend skinning: vertex v's matrix is the weighted average of its joints' skin matrices, or `None`
    /// when it has no usable joint (it then stays where it is).
    fn matrix(&self, v: usize, mats: &[Mat4]) -> Option<Mat4> {
        let (j, w) = (self.joints[v], self.weights[v]);
        let mut sum = Mat4::from_scale(0.0);
        let mut total = 0.0;
        for k in 0..4 {
            if let Some(m) = mats.get(j[k] as usize).filter(|_| w[k] > 0.0) {
                sum += m * w[k];
                total += w[k];
            }
        }
        (total > 0.0).then(|| sum / total)
    }
}

type Skinned = (Vec<Vec3>, Option<Vec<Vec3>>);

/// Skinned positions and normals of a part (shared by every part of its shape).
fn skin_part(part: &SkinPart, mats: &[Mat4]) -> Skinned {
    let n = part.positions.len();
    let mut pos = Vec::with_capacity(n);
    let mut nrm = part.normals.as_ref().map(|_| Vec::with_capacity(n));
    for v in 0..n {
        let m = part.matrix(v, mats);
        pos.push(m.map_or(part.positions[v], |m| {
            (m * part.positions[v].extend(1.0)).truncate()
        }));
        if let (Some(o), Some(ns)) = (nrm.as_mut(), &part.normals) {
            o.push(m.map_or(ns[v], |m| (m * ns[v].extend(0.0)).truncate().normalize()));
        }
    }
    (pos, nrm)
}

/// Skinned tangents of one part (see [SkinPart::tangents]); the handedness in w is kept.
fn skin_tangents(part: &SkinPart, mats: &[Mat4]) -> Option<Vec<Vec4>> {
    let ts = part.tangents.as_ref()?;
    Some(
        ts.iter()
            .enumerate()
            .map(|(v, t)| match part.matrix(v, mats) {
                Some(m) => (m * t.truncate().extend(0.0))
                    .truncate()
                    .normalize()
                    .extend(t.w),
                None => *t,
            })
            .collect(),
    )
}

/// Poses every skinned part with the given skin matrices.
// three-d's set_* methods compare the new vertex count with the index count (Mesh::vertex_count returns the
// number of indices), so a mesh with more vertices than indices is refilled through the older buffer accessors.
#[allow(deprecated)]
fn apply_skin(loaded: &mut Loaded, mats: &[Mat4]) {
    let mut done: HashMap<String, Skinned> = HashMap::new();
    for (gm, part) in loaded.meshes.iter_mut().zip(&loaded.skin) {
        let Some(part) = part else { continue };
        let (pos, nrm) = done
            .entry(part.shape.clone())
            .or_insert_with(|| skin_part(part, mats));
        let tan = skin_tangents(part, mats);
        let mesh = &mut gm.geometry;
        // set_positions_partially also grows the culling box; it checks the length against the index count, so
        // fall back to a plain fill when there are more vertices than indices.
        if mesh.set_positions_partially(0, pos).is_err() {
            mesh.positions_mut().fill(pos);
        }
        if let Some(n) = nrm.as_ref()
            && mesh.set_normals_partially(0, n).is_err()
            && let Some(buf) = mesh.normals_mut().as_mut()
        {
            buf.fill(n);
        }
        if let Some(t) = tan.as_ref()
            && mesh.set_tangents_partially(0, t).is_err()
            && let Some(buf) = mesh.tangents_mut().as_mut()
        {
            buf.fill(t);
        }
    }
}

fn cpu_texture(img: &texture::Image) -> CpuTexture {
    CpuTexture {
        data: TextureData::RgbaU8(img.rgba.clone()),
        width: img.width,
        height: img.height,
        ..Default::default()
    }
}

fn build(
    context: &Context,
    model: model::Model,
    baked: HashMap<String, materials::Baked>,
    warnings: Vec<String>,
    shading: Shading,
    normal_maps: bool,
) -> Loaded {
    // One GPU material per Trinity material, shared by every part that uses it (textures are uploaded once).
    let mut gpu_materials: HashMap<String, eye::ModelMaterial> = HashMap::new();
    let mut meshes = Vec::new();
    let mut skin = Vec::new();
    for s in &model.submeshes {
        let mat = model.materials.get(&s.material);
        let bk = baked
            .get(&s.material)
            .filter(|_| shading == Shading::Textured);
        let [su, sv, ou, ov] = mat
            .and_then(|m| m.params.get("UVScaleOffset"))
            .copied()
            .unwrap_or([1.0, 1.0, 0.0, 0.0]);
        let mut cpu = CpuMesh {
            positions: Positions::F32(s.positions.iter().map(|p| vec3(p[0], p[1], p[2])).collect()),
            indices: Indices::U32(s.indices.clone()),
            // Trinity UVs have V running down the image (row 0 at V = 0 is the top); the textures are uploaded
            // top row first, so V is flipped for OpenGL's bottom-up convention.
            uvs: s.uvs.as_ref().map(|uv| {
                uv.iter()
                    .map(|t| vec2(t[0] * su + ou, 1.0 - (t[1] * sv + ov)))
                    .collect()
            }),
            ..Default::default()
        };
        match &s.normals {
            Some(n) => {
                cpu.normals = Some(
                    n.iter()
                        .map(|n| vec3(n[0], n[1], n[2]).normalize())
                        .collect(),
                )
            }
            None => cpu.compute_normals(),
        }
        let use_normal_map =
            normal_maps && bk.is_some_and(|b| b.normal.is_some()) && cpu.uvs.is_some();
        // Pokémon eyes: the eye material, which needs UVs, tangents (for its coat normals) and a colour texture
        let eye_bake = bk
            .filter(|b| b.albedo.is_some() && cpu.uvs.is_some())
            .and_then(|b| b.eye.as_ref().map(|e| (b, e)));
        if use_normal_map || eye_bake.is_some() {
            cpu.compute_tangents();
        }
        let material = gpu_materials
            .entry(s.material.clone())
            .or_insert_with(|| {
                if let Some((b, e)) = eye_bake
                    && let Some(a) = &b.albedo
                {
                    let tex = |i: &texture::Image| {
                        Texture2DRef::from_cpu_texture(context, &cpu_texture(i))
                    };
                    // colour in linear space, as three-d's physical material uploads it
                    let mut linear = cpu_texture(a);
                    linear.data.to_linear_srgb();
                    return eye::ModelMaterial::Eye(eye::EyeMaterial {
                        albedo: Texture2DRef::from_cpu_texture(context, &linear),
                        normal: b.normal.as_ref().filter(|_| normal_maps).map(tex),
                        coat: e.coat_normal.as_ref().filter(|_| normal_maps).map(tex),
                        // as the physical material before, so the eye's colours look the same
                        roughness: 0.7,
                        highlight: e.highlight,
                        highlight_roughness: e.highlight_roughness,
                    });
                }
                let grey = Srgba::new(205, 205, 210, 255);
                let mut m = CpuMaterial {
                    roughness: 0.7,
                    metallic: 0.0,
                    albedo: grey,
                    ..Default::default()
                };
                let mut alpha = materials::AlphaMode::Opaque;
                match (shading, bk) {
                    (Shading::Textured, Some(b)) => {
                        if let Some(a) = &b.albedo {
                            m.albedo = Srgba::WHITE;
                            m.albedo_texture = Some(cpu_texture(a));
                        } else if let Some(c) = b.color {
                            let to8 = |v: f32| (materials::linear_to_srgb(v) * 255.0).round() as u8;
                            m.albedo = Srgba::new(
                                to8(c[0]),
                                to8(c[1]),
                                to8(c[2]),
                                (c[3].clamp(0.0, 1.0) * 255.0).round() as u8,
                            );
                        }
                        alpha = b.alpha;
                        if let materials::AlphaMode::Cutout(t) = alpha {
                            m.alpha_cutout = Some(t);
                        }
                        if use_normal_map {
                            m.normal_texture = b.normal.as_ref().map(cpu_texture);
                        }
                    }
                    (Shading::MaterialColours, _) => m.albedo = material_color(&s.material),
                    _ => {}
                }
                let cutout = match alpha {
                    materials::AlphaMode::Cutout(t) => Some(t),
                    _ => None,
                };
                eye::ModelMaterial::physical(transparent_material(context, &m, alpha), cutout)
            })
            .clone();
        meshes.push(Gm::new(Mesh::new(context, &cpu), material));
        skin.push(match (&s.joints, &s.weights, &cpu.positions) {
            (Some(j), Some(w), Positions::F32(p)) if j.len() == p.len() && w.len() == p.len() => {
                Some(SkinPart {
                    shape: s.shape.clone(),
                    positions: p.clone(),
                    normals: cpu.normals.clone(),
                    tangents: cpu.tangents.clone(),
                    joints: j.clone(),
                    weights: w.clone(),
                })
            }
            _ => None,
        });
    }

    let (min, max) = (Vec3::from(model.min), Vec3::from(model.max));
    let center = (min + max) * 0.5;
    let radius = ((max - min).magnitude() * 0.5).max(0.01);
    let mut floor = Gm::new(
        Mesh::new(context, &CpuMesh::square()),
        PhysicalMaterial::new_opaque(
            context,
            &CpuMaterial {
                albedo: Srgba::new(70, 74, 82, 255),
                roughness: 1.0,
                ..Default::default()
            },
        ),
    );
    floor.set_transformation(
        Mat4::from_translation(vec3(center.x, min.y, center.z))
            * Mat4::from_scale(radius * 4.0)
            * Mat4::from_angle_x(degrees(-90.0)),
    );
    Loaded {
        model,
        baked,
        warnings,
        meshes,
        skin,
        floor,
    }
}

/// The model view's Auto LOD: LOD 0 within 6 model radii of the camera, LOD 1 within 14, then LOD 2 (capped at
/// the model's last). A switch needs the distance 15% past a boundary, so it does not flip back and forth (each
/// switch reloads the model).
fn auto_lod(radii: f32, current: usize, count: usize) -> usize {
    const EDGES: [f32; 2] = [6.0, 14.0];
    let pick = |r: f32| EDGES.iter().filter(|e| r >= **e).count();
    let up = pick(radii / 1.15);
    let down = pick(radii * 1.15);
    let want = if up > current {
        up
    } else if down < current {
        down
    } else {
        current
    };
    want.min(count.saturating_sub(1))
}

fn model_radius(model: &model::Model) -> f32 {
    ((Vec3::from(model.max) - Vec3::from(model.min)).magnitude() * 0.5).max(0.01)
}

type SkeletonGm = (
    Gm<InstancedMesh, ColorMaterial>,
    Gm<InstancedMesh, ColorMaterial>,
);

/// Bones as cones from each bone to its parent and joints as small spheres. Skinning joints are blue, helper bones
/// grey, the selected bone and the segment to its parent yellow. With `xray` they are drawn over the mesh.
fn build_skeleton(
    context: &Context,
    skel: &skeleton::Skeleton,
    positions: &[Vec3],
    size: f32,
    selected: Option<usize>,
    xray: bool,
) -> SkeletonGm {
    let colour = |i: usize| {
        if Some(i) == selected {
            Srgba::new(255, 220, 40, 255)
        } else if skel.bones[i].joint.is_some() {
            Srgba::new(90, 175, 255, 255)
        } else {
            Srgba::new(170, 170, 175, 255)
        }
    };
    let mut seg = Instances {
        transformations: Vec::new(),
        texture_transformations: None,
        colors: Some(Vec::new()),
    };
    let mut dot = Instances {
        transformations: Vec::new(),
        texture_transformations: None,
        colors: Some(Vec::new()),
    };
    for (i, b) in skel.bones.iter().enumerate() {
        let p = positions[i];
        let r = if b.joint.is_some() { size } else { size * 0.6 };
        dot.transformations
            .push(Mat4::from_translation(p) * Mat4::from_scale(r));
        dot.colors.as_mut().unwrap().push(colour(i));
        let Some(parent) = b.parent else { continue };
        let pp = positions[parent];
        let d = p - pp;
        let len = d.magnitude();
        if len < size * 0.5 {
            continue;
        }
        // The cone runs along +X from radius 1 at x = 0 to a point at x = 1: base at the parent, tip at the child.
        let rot = Mat4::from(Quat::from_arc(
            vec3(1.0, 0.0, 0.0),
            d / len,
            Some(vec3(0.0, 1.0, 0.0)),
        ));
        let w = (len * 0.12).min(size * 0.9);
        seg.transformations
            .push(Mat4::from_translation(pp) * rot * Mat4::from_nonuniform_scale(len, w, w));
        seg.colors.as_mut().unwrap().push(colour(i));
    }
    let material = || ColorMaterial {
        color: Srgba::WHITE,
        render_states: RenderStates {
            depth_test: if xray {
                DepthTest::Always
            } else {
                DepthTest::Less
            },
            ..Default::default()
        },
        ..Default::default()
    };
    (
        Gm::new(
            InstancedMesh::new(context, &seg, &CpuMesh::cone(10)),
            material(),
        ),
        Gm::new(
            InstancedMesh::new(context, &dot, &CpuMesh::sphere(8)),
            material(),
        ),
    )
}

/// The bone hierarchy as a collapsible tree; a click selects a bone.
fn bone_tree(
    ui: &mut egui::Ui,
    skel: &skeleton::Skeleton,
    i: usize,
    depth: usize,
    selected: &mut Option<usize>,
) {
    let b = &skel.bones[i];
    let label = if b.joint.is_some() {
        b.name.clone()
    } else {
        format!("{} (helper)", b.name)
    };
    let children: Vec<usize> = skel.children(i).collect();
    if children.is_empty() {
        if ui.selectable_label(*selected == Some(i), label).clicked() {
            *selected = Some(i);
        }
        return;
    }
    let id = ui.make_persistent_id(("bone", i));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth < 3)
        .show_header(ui, |ui| {
            if ui.selectable_label(*selected == Some(i), label).clicked() {
                *selected = Some(i);
            }
        })
        .body(|ui| {
            for c in children {
                bone_tree(ui, skel, c, depth + 1, selected);
            }
        });
}

/// One playing animation: the clip, its clock (in frames), and for each bone the track that drives it.
struct Clip {
    anim: anim::Animation,
    frame: f32,
    track_of_bone: Vec<Option<usize>>,
}

impl Clip {
    fn new(anim: anim::Animation, skel: Option<&skeleton::Skeleton>) -> Clip {
        let track_of_bone = skel
            .map(|s| {
                s.bones
                    .iter()
                    .map(|b| anim.tracks.iter().position(|t| t.name == b.name))
                    .collect()
            })
            .unwrap_or_default();
        Clip {
            anim,
            frame: 0.0,
            track_of_bone,
        }
    }

    /// The last frame, where the clock wraps or stops.
    fn last(&self) -> f32 {
        (self.anim.frames.max(2) - 1) as f32
    }

    /// How far through the clip the clock is, 0 to 1.
    fn phase(&self) -> f32 {
        self.frame / self.last()
    }

    /// Moves the clock on by `seconds`; false when it reached the end without looping.
    fn advance(&mut self, seconds: f32, looping: bool) -> bool {
        self.frame += seconds * self.anim.fps as f32;
        let last = self.last();
        if self.frame > last {
            if looping {
                self.frame %= last;
            } else {
                self.frame = last;
                return false;
            }
        }
        true
    }

    /// Local transforms of all bones at the current frame; bones the clip does not drive keep the rest pose.
    fn locals(&self, s: &skeleton::Skeleton) -> Vec<skeleton::Local> {
        (0..s.bones.len())
            .map(|i| {
                let rest = s.rest_local(i);
                match self.track_of_bone.get(i).copied().flatten() {
                    Some(t) => self.anim.tracks[t].sample(self.frame).or_rest(rest),
                    None => rest,
                }
            })
            .collect()
    }
}

/// Mixes pose `b` into pose `a` (per bone) with weight `t`: 0 keeps `a`, 1 gives `b`.
fn mix_pose(a: &mut [skeleton::Local], b: &[skeleton::Local], t: f32) {
    for (x, y) in a.iter_mut().zip(b) {
        *x = skeleton::Local::lerp(*x, *y, t);
    }
}

/// The ground of play mode: a large checkered square, so that movement shows. It is moved under the character
/// in steps of one checker period (two tiles), which leaves the pattern in place.
struct DemoFloor {
    gm: Gm<Mesh, PhysicalMaterial>,
    tile: f32,
    y: f32,
}

impl DemoFloor {
    fn new(context: &Context, tile: f32, y: f32) -> DemoFloor {
        const N: u32 = 64; // texture size: one checker period
        let (a, b) = ([74u8, 78, 88, 255], [58u8, 62, 70, 255]);
        let data = (0..N * N)
            .map(|i| {
                if (i % N < N / 2) == (i / N < N / 2) {
                    a
                } else {
                    b
                }
            })
            .collect();
        let periods = 60.0; // from the centre to each edge
        let h = periods * 2.0 * tile;
        let mesh = CpuMesh {
            positions: Positions::F32(vec![
                vec3(-h, 0.0, -h),
                vec3(h, 0.0, -h),
                vec3(h, 0.0, h),
                vec3(-h, 0.0, h),
            ]),
            indices: Indices::U32(vec![0, 2, 1, 0, 3, 2]),
            normals: Some(vec![vec3(0.0, 1.0, 0.0); 4]),
            uvs: Some(vec![
                vec2(0.0, 0.0),
                vec2(periods * 2.0, 0.0),
                vec2(periods * 2.0, periods * 2.0),
                vec2(0.0, periods * 2.0),
            ]),
            ..Default::default()
        };
        let material = CpuMaterial {
            albedo: Srgba::WHITE,
            albedo_texture: Some(CpuTexture {
                data: TextureData::RgbaU8(data),
                width: N,
                height: N,
                ..Default::default()
            }),
            roughness: 1.0,
            ..Default::default()
        };
        DemoFloor {
            gm: Gm::new(
                Mesh::new(context, &mesh),
                PhysicalMaterial::new_opaque(context, &material),
            ),
            tile,
            y,
        }
    }

    fn follow(&mut self, pos: Vec3) {
        let step = 2.0 * self.tile;
        let snap = |v: f32| (v / step).round() * step;
        self.gm.set_transformation(Mat4::from_translation(vec3(
            snap(pos.x),
            self.y,
            snap(pos.z),
        )));
    }
}

/// Starts play mode for a model: loads the clips it needs from its animation list.
fn start_demo(
    context: &Context,
    arc: &mut archive::Archive,
    list: &[(String, String)],
    skel: Option<&skeleton::Skeleton>,
    model: &model::Model,
) -> (demo::Demo, DemoFloor) {
    let picks = demo::pick(list);
    let clips: [Option<Clip>; demo::ROLES] = std::array::from_fn(|r| {
        let (label, path) = &list[picks[r]?];
        anim::load(arc, path, label)
            .ok()
            .map(|a| Clip::new(a, skel))
    });
    let labels =
        std::array::from_fn(|r| clips[r].as_ref().and(picks[r]).map(|i| list[i].0.clone()));
    let height = model.max[1] - model.min[1];
    let radius = model_radius(model);
    (
        demo::Demo::new(clips, labels, skel, height, radius),
        DemoFloor::new(context, radius * 0.5, model.min[1]),
    )
}

/// The keys play mode listens to.
const DEMO_KEYS: [Key; 9] = [
    Key::W,
    Key::A,
    Key::S,
    Key::D,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::Space,
];

fn frame_camera(camera: &mut Camera, control: &mut OrbitControl, model: &model::Model) {
    let (min, max) = (Vec3::from(model.min), Vec3::from(model.max));
    let center = (min + max) * 0.5;
    let radius = ((max - min).magnitude() * 0.5).max(0.01);
    let eye = center + vec3(0.6, 0.35, 1.0).normalize() * radius * 2.8;
    camera.set_view(eye, center, vec3(0.0, 1.0, 0.0));
    camera.set_perspective_projection(degrees(40.0), radius * 0.01, radius * 200.0);
    *control = OrbitControl::new(center, radius * 0.2, radius * 30.0);
}

/// Puts the camera close in front of a bone (the first whose name contains `name`), e.g. "head" for a face.
fn look_at_bone(
    camera: &mut Camera,
    control: &mut OrbitControl,
    skel: &skeleton::Skeleton,
    name: &str,
) {
    let Some(i) = skel
        .bones
        .iter()
        .position(|b| b.name == name)
        .or_else(|| skel.bones.iter().position(|b| b.name.contains(name)))
    else {
        return;
    };
    let target = skel.position(i, true);
    camera.set_view(target + vec3(0.0, 0.02, 0.45), target, vec3(0.0, 1.0, 0.0));
    camera.set_perspective_projection(degrees(40.0), 0.005, 50.0);
    *control = OrbitControl::new(target, 0.05, 10.0);
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check_only = args.iter().any(|a| a == "--check");
    let start_model = args
        .iter()
        .position(|a| a == "--model")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "pm0025_00_00".to_string());
    let romfs_arg = args
        .iter()
        .enumerate()
        // the romfs folder is the one argument that is neither an option nor the value of --model / --anim / --tiles
        .find(|(i, a)| {
            !a.starts_with("--")
                && (*i == 0
                    || !matches!(
                        args[i - 1].as_str(),
                        "--model"
                            | "--anim"
                            | "--tiles"
                            | "--look"
                            | "--camera"
                            | "--map"
                            | "--textures"
                            | "--sound"
                    ))
        })
        .map(|(_, s)| s.as_str());
    let romfs = romfs_path(romfs_arg);
    if check_only {
        let path = romfs.ok_or_else(|| anyhow!("no RomFS folder: pass it as an argument"))?;
        check(&mut archive::Archive::open(&path)?);
        return Ok(());
    }
    // Without a usable folder the window opens on the welcome screen, which asks for one.
    let mut romfs_error: Option<String> = None;
    let mut ar: Option<archive::Archive> = None;
    let mut romfs_shown = String::new();
    // The RomFS that is open: the Textures tab's worker opens it again for itself.
    let mut romfs_open: Option<PathBuf> = None;
    if let Some(p) = &romfs {
        match archive::Archive::open(p) {
            Ok(a) => {
                config::save_romfs_path(p);
                romfs_open = Some(p.clone());
                romfs_shown = p.display().to_string();
                ar = Some(a);
            }
            Err(e) => romfs_error = Some(format!("{e:#}")),
        }
    }
    let mut models = ar.as_ref().map(all_models).unwrap_or_default();
    let mut motions = ar.as_ref().map(Motions::new).unwrap_or_default();
    let mut romfs_pick: Option<PathBuf> = None;

    let window = Window::new(WindowSettings {
        title: "Trinity Model Viewer".to_string(),
        max_size: None,
        ..Default::default()
    })?;
    let context = window.gl();
    let mut camera = Camera::new_perspective(
        window.viewport(),
        vec3(0.0, 0.5, 2.0),
        vec3(0.0, 0.3, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(40.0),
        0.01,
        100.0,
    );
    let mut control = OrbitControl::new(vec3(0.0, 0.3, 0.0), 0.05, 50.0);
    let mut gui = three_d::GUI::new(&context);

    let ambient = AmbientLight::new(&context, 0.45, Srgba::WHITE);
    let key = DirectionalLight::new(&context, 2.2, Srgba::WHITE, vec3(-0.6, -1.0, -0.8));
    let fill = DirectionalLight::new(
        &context,
        0.7,
        Srgba::new(200, 210, 255, 255),
        vec3(0.8, -0.4, 0.6),
    );
    let rim = DirectionalLight::new(&context, 0.5, Srgba::WHITE, vec3(0.0, -0.3, 1.0));
    // Lit: a warm sun high up that casts the shadows, and a cool sky that fills the shade. The sky reaches the
    // model's shaded side and the shadow on the floor alike, so the two match (a side light would brighten only
    // the model). The sun is kept moderate so white surfaces do not burn out. A faint upward bounce, as off the
    // ground, lifts only the undersides; the floor faces away from it.
    let sky = AmbientLight::new(&context, 0.45, Srgba::new(210, 222, 255, 255));
    let mut sun = DirectionalLight::new(
        &context,
        1.4,
        Srgba::new(255, 246, 232, 255),
        vec3(-0.45, -1.0, -0.55),
    );
    let bounce = DirectionalLight::new(
        &context,
        0.15,
        Srgba::new(235, 230, 220, 255),
        vec3(0.0, 1.0, 0.0),
    );
    let mut lighting = if args.iter().any(|a| a == "--lit") {
        Lighting::Lit
    } else {
        Lighting::Unlit
    };

    let mut search = String::new();
    let mut selected: Option<usize> = None;
    let mut pending: Option<usize> = models.iter().position(|m| m.contains(&start_model));
    let mut category = pending.map_or(Category::Pokemon, |i| Category::of(&models[i]));
    // The player character: the parts found, the ones picked, and whether it is what is shown.
    let mut parts: Vec<player::Part> = ar.as_ref().map(player::catalog).unwrap_or_default();
    let mut outfit = player::Outfit::default_for(&parts);
    let mut rng: u64 = 0;
    let mut player_shown = false;
    // Some(reframe): assemble the player next frame (reframe: also move the camera to it)
    let mut player_request: Option<bool> = None;
    if args.iter().any(|a| a == "--player") {
        category = Category::Player;
        pending = None;
        player_request = Some(true);
    }
    // The World view: created when its tab is first opened; `world_was_shown` notices tab switches.
    let mut world: Option<world::World> = None;
    let mut world_was_shown = false;
    let mut world_rebuild = false;
    // --tiles 05_03,06_03: open the World tab showing only these tiles
    let start_tiles = args
        .iter()
        .position(|a| a == "--tiles")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // --camera ex,ey,ez,tx,ty,tz: in the World view, where the camera stands and looks once the tiles loaded
    let mut start_camera: Option<[f32; 6]> = args
        .iter()
        .position(|a| a == "--camera")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| {
            let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            n.try_into().ok()
        });
    // --map Kitakami / Terarium / Paldea, or a room scene path: open the World tab on it
    let mut start_map: Option<maps::Map> = args
        .iter()
        .position(|a| a == "--map")
        .and_then(|i| args.get(i + 1))
        .map(|name| {
            maps::overworlds()
                .into_iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .unwrap_or_else(|| maps::room(name.rsplit('/').next().unwrap_or(name), name))
        });
    if args.iter().any(|a| a == "--world") || start_tiles.is_some() || start_map.is_some() {
        category = Category::World;
    }
    // The Textures tab: started (with its worker thread) when first opened.
    let mut gallery: Option<gallery::Gallery> = None;
    // The Sound tab: started (with its worker) when first opened; it keeps playing on other tabs.
    let mut sound_tab: Option<audio::AudioTab> = None;
    // --sound [group]: open the Sound tab, on the first group whose name contains [group] (e.g. music)
    let mut start_sound_group: Option<String> = None;
    if let Some(i) = args.iter().position(|a| a == "--sound") {
        category = Category::Sound;
        start_sound_group = args.get(i + 1).filter(|a| !a.starts_with("--")).cloned();
    }
    // --textures [folder]: open the Textures tab, on the first folder whose path contains [folder]
    let mut start_folder: Option<String> = None;
    if let Some(i) = args.iter().position(|a| a == "--textures") {
        category = Category::Textures;
        start_folder = args.get(i + 1).filter(|a| !a.starts_with("--")).cloned();
    }
    let mut lod: usize = 0;
    // Auto LOD in the model view: the LOD follows the camera's distance (see `auto_lod`)
    let mut lod_auto = false;
    let mut shading = Shading::Textured;
    let mut normal_maps = true;
    let mut show_floor = true;
    let mut rebuild = false;
    let mut current: Option<Loaded> = None;
    let mut error: Option<String> = None;

    let mut show_mesh = true;
    let mut show_skeleton = args.iter().any(|a| a == "--skeleton");
    let mut xray = true;
    let mut bind_pose = true;
    let mut skel: Option<skeleton::Skeleton> = None;
    let mut skel_error: Option<String> = None;
    let mut skel_gm: Option<SkeletonGm> = None;
    let mut skel_dirty = false;
    let mut selected_bone: Option<usize> = None;

    // Animation: the model's list (label, path), the selected entry and the clip loaded from it.
    let mut anim_list: Vec<(String, String)> = Vec::new();
    // A trainer's or NPC's motions, and a body type picked in the panel (Some(None): its own clips).
    let mut char_motion: Option<CharMotion> = None;
    let mut body_request: Option<Option<String>> = None;
    let mut anim: Option<Clip> = None;
    let mut anim_index: Option<usize> = None;
    let mut anim_request: Option<Option<usize>> = None;
    let mut anim_error: Option<String> = None;
    // Blending: a second clip mixed over the first with `blend_weight`, its clock either following the first
    // clip's phase (`blend_sync`) or running on its own.
    let mut blend: Option<Clip> = None;
    let mut blend_index: Option<usize> = None;
    let mut blend_request: Option<Option<usize>> = None;
    let mut blend_error: Option<String> = None;
    let mut blend_weight = 0.5f32;
    let mut blend_sync = true;
    // Crossfade: on a switch of either clip, the last pose shown is held and faded out over `crossfade` seconds.
    let mut crossfade = 0.25f32;
    let mut fade: Option<(Vec<skeleton::Local>, f32)> = None;
    let mut last_pose: Option<Vec<skeleton::Local>> = None;
    let mut playing = true;
    let mut speed = 1.0f32;
    // Play mode (F7): the controller, its ground, and the keyboard state it reads.
    let mut demo: Option<demo::Demo> = None;
    let mut demo_floor: Option<DemoFloor> = None;
    let mut demo_toggle = false;
    let mut keys_held: std::collections::HashSet<Key> = Default::default();
    let mut shift = false;
    let mut jump_queued = false;
    let mut loop_anim = true;
    // --look head: frame the start model's camera on that bone
    let mut look_bone = args
        .iter()
        .position(|a| a == "--look")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let start_anim = args
        .iter()
        .position(|a| a == "--anim")
        .and_then(|i| args.get(i + 1))
        .cloned();

    window.render_loop(move |mut frame_input| {
        // A folder picked last frame: open it and start over with its models.
        if let Some(p) = romfs_pick.take() {
            match archive::Archive::open(&p) {
                Ok(a) => {
                    config::save_romfs_path(&p);
                    romfs_shown = p.display().to_string();
                    romfs_open = Some(p.clone());
                    gallery = None;
                    sound_tab = None;
                    romfs_error = None;
                    models = all_models(&a);
                    motions = Motions::new(&a);
                    char_motion = None;
                    parts = player::catalog(&a);
                    outfit = player::Outfit::default_for(&parts);
                    player_shown = false;
                    player_request = None;
                    ar = Some(a);
                    world = None;
                    current = None;
                    skel = None;
                    skel_gm = None;
                    anim = None;
                    blend = None;
                    fade = None;
                    last_pose = None;
                    demo = None;
                    anim_list.clear();
                    selected = None;
                    error = None;
                    pending = models.iter().position(|m| m.contains(&start_model)).or(
                        (!models.is_empty()).then_some(0),
                    );
                    if category != Category::World && !category.flat() {
                        category = pending.map_or(Category::Pokemon, |i| Category::of(&models[i]));
                    }
                }
                Err(e) => romfs_error = Some(format!("{e:#}")),
            }
        }

        // No archive yet: the welcome screen asks for the RomFS folder.
        let Some(arc) = ar.as_mut() else {
            gui.update(
                &mut frame_input.events,
                frame_input.accumulated_time,
                frame_input.viewport,
                frame_input.device_pixel_ratio,
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() * 0.25);
                            ui.heading("Trinity Model Viewer");
                            ui.add_space(8.0);
                            ui.label("Views the models, textures, skeletons and animations of your own game dump.");
                            ui.label("No game files come with this program: choose the RomFS folder you dumped,");
                            ui.label("the one that contains the \"arc\" folder with data.trpfd and data.trpfs.");
                            ui.add_space(12.0);
                            if ui.button("  Choose RomFS folder…  ").clicked() {
                                romfs_pick = pick_romfs_folder();
                            }
                            if let Some(e) = &romfs_error {
                                ui.add_space(12.0);
                                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                            }
                        });
                    });
                },
            );
            let screen = frame_input.screen();
            screen.clear(ClearState::color_and_depth(0.13, 0.14, 0.17, 1.0, 1.0));
            screen.write(|| gui.render()).ok();
            return FrameOutput::default();
        };

        // The World view: load one queued tile per frame; move the camera on entering and leaving the tab.
        let world_shown = category == Category::World;
        if world_shown {
            // a map picked in the panel last frame: a new World (the old one's GPU data is dropped)
            if let Some(m) = world.as_mut().and_then(|w| w.switch_to.take()) {
                let mut w = world::World::new(arc, m);
                w.request_shown();
                world = Some(w);
                world_was_shown = false;
            }
            let w = world.get_or_insert_with(|| {
                let map = start_map
                    .take()
                    .unwrap_or_else(|| maps::overworlds().remove(0));
                let mut w = world::World::new(arc, map);
                if let Some(list) = &start_tiles {
                    w.show_only(list);
                }
                w.request_shown();
                w
            });
            if std::mem::take(&mut world_rebuild) {
                w.rebuild(&context);
            }
            w.update(&context, arc, camera.position());
            let loaded = std::mem::take(&mut w.reframe);
            if loaded || !world_was_shown {
                w.frame(&mut camera, &mut control);
            }
            if loaded && let Some(c) = start_camera.take() {
                let target = vec3(c[3], c[4], c[5]);
                camera.set_view(vec3(c[0], c[1], c[2]), target, vec3(0.0, 1.0, 0.0));
                camera.set_perspective_projection(degrees(45.0), 0.1, 20000.0);
                control = OrbitControl::new(target, 1.0, 20000.0);
            }
        } else if world_was_shown && let Some(c) = &current {
            frame_camera(&mut camera, &mut control, &c.model);
        }
        world_was_shown = world_shown;

        // Load what the list or the player panel asked for last frame. `reframe`: a different model, so the
        // camera moves to it; a LOD change keeps the skeleton and animation.
        let mut job: Option<(bool, Result<Prepared>)> = None;
        if let Some(i) = pending.take() {
            let reframe = selected != Some(i) || player_shown;
            selected = Some(i);
            player_shown = false;
            job = Some((reframe, load_listed(arc, &motions, &models[i], lod, reframe)));
        } else if let Some(reframe) = player_request.take() {
            selected = None;
            player_shown = true;
            job = Some((reframe, load_player(arc, &parts, &outfit, lod)));
        }
        if let Some((reframe, result)) = job {
            match result {
                Ok(p) => {
                    if reframe {
                        frame_camera(&mut camera, &mut control, &p.model);
                    }
                    if let Some(s) = p.skel {
                        (skel, skel_error) = match s {
                            Ok(s) => (Some(s), None),
                            Err(e) => (None, Some(e)),
                        };
                        // a new outfit on the same player keeps the clips that were playing
                        let keep = (!reframe && player_shown).then_some((anim_index, blend_index));
                        selected_bone = None;
                        skel_dirty = true;
                        anim_list = p.anims.unwrap_or_default();
                        char_motion = p.motion;
                        anim = None;
                        anim_index = None;
                        anim_error = None;
                        blend = None;
                        blend_index = None;
                        blend_error = None;
                        fade = None;
                        last_pose = None;
                        if demo.is_some() {
                            let (d, f) = start_demo(&context, arc, &anim_list, skel.as_ref(), &p.model);
                            demo = Some(d);
                            demo_floor = Some(f);
                        }
                        if let Some((a, b)) = keep {
                            anim_request = a.map(Some);
                            blend_request = b.map(Some);
                        } else if let Some(want) = &start_anim {
                            // --anim <part of a name> picks the first match, for the start model
                            anim_request = anim_list.iter().position(|(l, _)| l.contains(want.as_str())).map(Some);
                        }
                    }
                    if let (Some(name), Some(s)) = (look_bone.take(), skel.as_ref()) {
                        look_at_bone(&mut camera, &mut control, s, &name);
                    }
                    current = Some(build(&context, p.model, p.baked, p.warnings, shading, normal_maps));
                    error = None;
                }
                Err(e) => {
                    current = None;
                    skel = None;
                    skel_gm = None;
                    demo = None;
                    char_motion = None;
                    anim_list.clear();
                    anim = None;
                    anim_index = None;
                    blend = None;
                    blend_index = None;
                    error = Some(format!("{e:#}"));
                }
            }
        }
        if rebuild {
            rebuild = false;
            if let Some(c) = current.take() {
                current = Some(build(&context, c.model, c.baked, c.warnings, shading, normal_maps));
            }
        }

        // A body type picked last frame: list its clips, and keep playing the same clips where it has them.
        if let Some(body) = body_request.take()
            && let Some(cm) = char_motion.as_mut()
            && cm.body != body
        {
            let label_of = |i: Option<usize>| i.and_then(|i| anim_list.get(i)).map(|(l, _)| l.clone());
            let (playing_label, blend_label) = (label_of(anim_index), label_of(blend_index));
            cm.body = body;
            anim_list = motion::clips(arc, &cm.found, cm.body.as_deref());
            let find = |l: &Option<String>| l.as_ref().and_then(|l| anim_list.iter().position(|(x, _)| x == l));
            // the old indices point into the old list
            anim_index = None;
            blend_index = None;
            if playing_label.is_some() {
                anim_request = Some(find(&playing_label));
            }
            if blend_label.is_some() {
                blend_request = Some(find(&blend_label));
            }
            if demo.is_some()
                && let Some(c) = &current
            {
                let (d, f) = start_demo(&context, arc, &anim_list, skel.as_ref(), &c.model);
                demo = Some(d);
                demo_floor = Some(f);
            }
        }

        // Switch animation if the panel asked for it last frame.
        // A switch of either clip fades out of the pose shown last frame (none when the model stood still).
        let held_pose = |last: &Option<Vec<skeleton::Local>>| last.clone().filter(|_| crossfade > 0.0).map(|p| (p, 0.0));
        if let Some(req) = anim_request.take() {
            fade = held_pose(&last_pose);
            anim_index = req;
            anim_error = None;
            anim = None;
            match req.and_then(|i| anim_list.get(i)) {
                Some((label, path)) => match anim::load(arc, path, label) {
                    Ok(a) => {
                        anim = Some(Clip::new(a, skel.as_ref()));
                        playing = true;
                    }
                    Err(e) => anim_error = Some(format!("{e:#}")),
                },
                // back to the still model: rebuild the meshes in their bind pose
                None => {
                    rebuild = true;
                    blend = None;
                    blend_index = None;
                    fade = None;
                }
            }
            if let Some(b) = blend.as_mut().filter(|_| !blend_sync) {
                b.frame = 0.0;
            }
            skel_dirty = true;
        }
        if let Some(req) = blend_request.take() {
            fade = held_pose(&last_pose);
            blend_index = req;
            blend_error = None;
            blend = None;
            if let Some((label, path)) = req.and_then(|i| anim_list.get(i)) {
                match anim::load(arc, path, label) {
                    Ok(a) => blend = Some(Clip::new(a, skel.as_ref())),
                    Err(e) => blend_error = Some(format!("{e:#}")),
                }
            }
        }

        // Enter or leave play mode if F7 (or the panel) asked for it last frame.
        if std::mem::take(&mut demo_toggle) {
            if demo.take().is_some() {
                keys_held.clear();
                if let Some(c) = current.as_mut() {
                    for gm in c.meshes.iter_mut() {
                        gm.set_transformation(Mat4::identity());
                    }
                    frame_camera(&mut camera, &mut control, &c.model);
                }
                // without an animation the meshes go back to the bind pose
                rebuild |= anim.is_none();
                fade = None;
                last_pose = None;
                skel_dirty = true;
            } else if let Some(c) = &current {
                let (d, f) = start_demo(&context, arc, &anim_list, skel.as_ref(), &c.model);
                demo = Some(d);
                demo_floor = Some(f);
                jump_queued = false;
            }
        }

        // Advance time and pose the skeleton.
        let seconds = frame_input.elapsed_time as f32 / 1000.0;
        if let (Some(a), true) = (anim.as_mut(), playing) {
            if !a.advance(seconds * speed, loop_anim) {
                playing = false;
            }
            if let Some(b) = blend.as_mut().filter(|_| !blend_sync) {
                b.advance(seconds * speed, loop_anim);
            }
        }
        if let (Some(a), Some(b), true) = (&anim, blend.as_mut(), blend_sync) {
            // the same point of the cycle, so two gaits of different lengths stay in step
            b.frame = a.phase() * b.last();
        }
        if let Some((_, t)) = fade.as_mut() {
            *t += seconds;
            if *t >= crossfade {
                fade = None;
            }
        }
        let posed: Option<Vec<Mat4>> = if let Some(d) = demo.as_mut() {
            let held = |keys: &[Key]| keys.iter().any(|k| keys_held.contains(k)) as i32 as f32;
            let input = demo::Input {
                forward: held(&[Key::W, Key::ArrowUp]) - held(&[Key::S, Key::ArrowDown]),
                right: held(&[Key::D, Key::ArrowRight]) - held(&[Key::A, Key::ArrowLeft]),
                run: shift,
                jump: std::mem::take(&mut jump_queued),
            };
            let before = d.pos;
            // the checker floor; the World view's terrain implements the same trait (`world::TerrainGround`)
            let pose = d.update(
                input,
                seconds,
                camera.view_direction(),
                skel.as_ref(),
                crossfade,
                &demo::FlatGround,
            );
            // the camera follows along the ground (not up and down, so jumps show against the view)
            let step = vec3(d.pos.x - before.x, 0.0, d.pos.z - before.z);
            camera.translate(step);
            control.target += step;
            if let Some(f) = demo_floor.as_mut() {
                f.follow(d.pos);
            }
            match (&skel, pose) {
                (Some(s), Some(p)) => Some(s.posed_world(&p)),
                _ => None,
            }
        } else {
            match (&skel, &anim) {
            (Some(s), Some(a)) => {
                let mut pose = a.locals(s);
                if let Some(b) = blend.as_ref().filter(|_| blend_weight > 0.0) {
                    mix_pose(&mut pose, &b.locals(s), blend_weight);
                }
                if let Some((from, t)) = &fade {
                    // eased in and out, so the hand-over has no jolt at either end
                    let x = (t / crossfade).clamp(0.0, 1.0);
                    let mut held = from.clone();
                    mix_pose(&mut held, &pose, x * x * (3.0 - 2.0 * x));
                    pose = held;
                }
                let world = s.posed_world(&pose);
                last_pose = Some(pose);
                Some(world)
            }
            _ => {
                last_pose = None;
                None
            }
        }
        };
        if let (Some(s), Some(world), Some(c)) = (&skel, &posed, current.as_mut()) {
            apply_skin(c, &s.skin_matrices(world));
        }
        // In play mode the whole model stands where the character is.
        let model_transform = demo.as_ref().map_or(Mat4::identity(), |d| d.transform());
        if let (Some(_), Some(c)) = (&demo, current.as_mut()) {
            for gm in c.meshes.iter_mut() {
                gm.set_transformation(model_transform);
            }
        }
        if skel_dirty || (posed.is_some() && show_skeleton) {
            skel_dirty = false;
            skel_gm = match (&skel, &current) {
                (Some(s), Some(c)) => {
                    let positions: Vec<Vec3> = match &posed {
                        Some(w) => w.iter().map(|m| (model_transform * m.w).truncate()).collect(),
                        None => (0..s.bones.len()).map(|i| s.position(i, bind_pose)).collect(),
                    };
                    let size = model_radius(&c.model) * 0.012;
                    Some(build_skeleton(&context, s, &positions, size, selected_bone, xray))
                }
                _ => None,
            };
        }

        let mut panel_width = 0.0f32;
        let mut right_width = 0.0f32;
        gui.update(
            &mut frame_input.events,
            frame_input.accumulated_time,
            frame_input.viewport,
            frame_input.device_pixel_ratio,
            |ui| {
                let resp = egui::Panel::left("models").default_size(300.0).resizable(true).show_inside(ui, |ui| {
                    ui.heading(match category {
                        Category::Textures => "Textures",
                        Category::Sound => "Sound",
                        _ => "Models",
                    });
                    ui.horizontal(|ui| {
                        for c in Category::ALL {
                            ui.selectable_value(&mut category, c, c.label());
                        }
                    });
                    let textures_shown = category == Category::Textures;
                    if textures_shown {
                        if let Some(p) = &romfs_open {
                            gallery.get_or_insert_with(|| gallery::Gallery::new(p.clone(), start_folder.take())).left(ui);
                        }
                    } else if category == Category::Sound {
                        if let Some(p) = &romfs_open {
                            sound_tab
                                .get_or_insert_with(|| audio::AudioTab::new(p.clone(), start_sound_group.take()))
                                .left(ui);
                        }
                    } else if category == Category::World {
                        if let Some(w) = world.as_mut() {
                            world_rebuild |= w.panel(ui, arc, &mut camera, &mut control);
                        }
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut lighting, Lighting::Unlit, "Unlit")
                                .on_hover_text("Soft light from all sides");
                            ui.radio_value(&mut lighting, Lighting::Lit, "Lit")
                                .on_hover_text("One sun (no shadows on terrain yet)");
                        });
                        ui.separator();
                    } else if category == Category::Player {
                        if parts.is_empty() {
                            ui.label("No player parts found in this dump.");
                        } else {
                            ui.small(format!("{} parts. Pick one per slot; changes show at once.", parts.len()));
                            ui.separator();
                            if player_panel(ui, &parts, &mut outfit, &mut rng) || (!player_shown && player_request.is_none()) {
                                player_request = Some(!player_shown);
                            }
                        }
                        ui.separator();
                    } else {
                    ui.add(egui::TextEdit::singleline(&mut search).hint_text(category.search_hint()));
                    let q = search.to_lowercase();
                    let in_category: Vec<usize> =
                        (0..models.len()).filter(|i| Category::of(&models[*i]) == category).collect();
                    let shown: Vec<usize> =
                        in_category.iter().copied().filter(|i| q.is_empty() || models[*i].contains(&q)).collect();
                    ui.small(format!("{} of {} models", shown.len(), in_category.len()));
                    ui.separator();

                    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
                    egui::ScrollArea::vertical().max_height(ui.available_height() * 0.6).auto_shrink([false, false]).show_rows(
                        ui,
                        row_h,
                        shown.len(),
                        |ui, range| {
                            for &i in &shown[range] {
                                if ui.selectable_label(selected == Some(i), short_name(&models[i])).clicked() {
                                    lod = 0;
                                    pending = Some(i);
                                }
                            }
                        },
                    );
                    ui.separator();
                    }

                    if category != Category::World && !category.flat() {
                    ui.heading("Display");
                    let before = (shading, normal_maps);
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut shading, Shading::Textured, "Textures");
                        ui.radio_value(&mut shading, Shading::MaterialColours, "Material colours");
                        ui.radio_value(&mut shading, Shading::Plain, "Plain");
                    });
                    if shading == Shading::Textured {
                        ui.checkbox(&mut normal_maps, "Normal maps");
                    }
                    if (shading, normal_maps) != before {
                        rebuild = true;
                    }
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut lighting, Lighting::Unlit, "Unlit")
                            .on_hover_text("Soft light from all sides");
                        ui.radio_value(&mut lighting, Lighting::Lit, "Lit")
                            .on_hover_text("One sun with shadows on the floor");
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut show_mesh, "Mesh");
                        ui.checkbox(&mut show_floor, "Floor");
                    });
                    let before = (show_skeleton, xray, bind_pose);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut show_skeleton, "Skeleton");
                        if show_skeleton {
                            ui.checkbox(&mut xray, "X-ray");
                        }
                    });
                    if show_skeleton {
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut bind_pose, true, "Bind pose")
                                .on_hover_text("Bone positions the mesh was skinned in (inverse bind matrices)");
                            ui.radio_value(&mut bind_pose, false, "Rest pose")
                                .on_hover_text("Bone positions from the stored transforms; animations start here");
                        });
                    }
                    if (show_skeleton, xray, bind_pose) != before {
                        skel_dirty = true;
                    }
                    if let Some(c) = &current {
                        if c.model.mesh_files > 1 {
                            let mut l = lod;
                            ui.horizontal(|ui| {
                                ui.add_enabled(
                                    !lod_auto,
                                    egui::Slider::new(&mut l, 0..=c.model.mesh_files - 1).text("LOD"),
                                );
                                ui.checkbox(&mut lod_auto, "Auto").on_hover_text(
                                    "Pick the LOD by the camera's distance: LOD 0 within 6 model radii,                                      LOD 1 within 14, then LOD 2",
                                );
                            });
                            if lod_auto {
                                let center = (Vec3::from(c.model.min) + Vec3::from(c.model.max)) * 0.5;
                                let r = camera.position().distance(center) / model_radius(&c.model);
                                l = auto_lod(r, lod, c.model.mesh_files);
                            }
                            if l != lod {
                                lod = l;
                                if player_shown {
                                    player_request = Some(false);
                                } else {
                                    pending = selected;
                                }
                            }
                        }
                        if ui.button("Reset camera").clicked() {
                            frame_camera(&mut camera, &mut control, &c.model);
                        }
                    }

                    ui.separator();
                    ui.heading("Play mode");
                    if ui
                        .button(if demo.is_some() { "Leave play mode (F7)" } else { "Enter play mode (F7)" })
                        .on_hover_text("Walk the model around to see how its animations hand over to each other")
                        .clicked()
                    {
                        demo_toggle = true;
                    }
                    if let Some(d) = &demo {
                        ui.small("WASD / arrows: move, relative to the camera · Shift: run · Space: jump · F7 / Esc: leave");
                        ui.label(format!("{:?} · speed {:.2}", d.state, d.speed));
                        ui.small(format!(
                            "walk {:.2}, run {:.2} units/s ({})",
                            d.walk_speed,
                            d.run_speed,
                            if d.measured { "from the clips' root motion" } else { "guessed from the model size" }
                        ));
                        ui.small("Ground blend:");
                        for (i, name) in ["Idle", "Walk", "Run"].iter().enumerate() {
                            ui.add(
                                egui::ProgressBar::new(d.weights[i])
                                    .desired_height(14.0)
                                    .text(format!("{name} {:.0}%", d.weights[i] * 100.0)),
                            );
                        }
                        ui.add(egui::Slider::new(&mut crossfade, 0.0..=1.0).text("crossfade (s)"))
                            .on_hover_text("Fade time between states; one-shot clips get at most half their length");
                        ui.collapsing("Clips used", |ui| {
                            for (r, (role, _, _)) in demo::ROLE_INFO.iter().enumerate() {
                                ui.small(format!("{role}: {}", d.labels[r].as_deref().unwrap_or("not in this model")));
                            }
                        });
                    }

                    ui.separator();
                    ui.heading("Animation");
                    if let Some(cm) = &char_motion {
                        let name = |b: &Option<String>| b.clone().unwrap_or_else(|| "own clips".to_string());
                        ui.horizontal(|ui| {
                            ui.label("Body motions");
                            egui::ComboBox::from_id_salt("body")
                                .selected_text(name(&cm.body))
                                .height(400.0)
                                .show_ui(ui, |ui| {
                                    if cm.found.own.is_some()
                                        && ui.selectable_label(cm.body.is_none(), "own clips").clicked()
                                    {
                                        body_request = Some(None);
                                    }
                                    for b in &motions.bodies {
                                        if ui.selectable_label(cm.body.as_ref() == Some(b), b).clicked() {
                                            body_request = Some(Some(b.clone()));
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(
                                    "Trainers and NPCs share 25 body types of motions (kid, adult m/f, and styles);                                      many trainers also have their own clips. Any type plays on any of them.",
                                );
                        });
                        let note = match (&cm.body, &cm.found.named_body) {
                            (None, _) => "Its own clips.".to_string(),
                            (Some(b), Some(n)) if b == n => "The body type its files name.".to_string(),
                            (Some(_), _) if cm.found.own.is_some() => "Another body type than its own clips.".to_string(),
                            _ => "No own clips: body type guessed from the name.".to_string(),
                        };
                        let extra = cm.found.extra.len();
                        ui.small(if extra > 0 { format!("{note} Plus {extra} special clips of its own.") } else { note });
                    }
                    if demo.is_some() {
                        ui.small("Controlled by play mode.");
                    } else if anim_list.is_empty() {
                        ui.small("This model lists no animations.");
                    } else {
                        let current_label = anim_index
                            .and_then(|i| anim_list.get(i))
                            .map(|(l, _)| l.as_str())
                            .unwrap_or("none (still)");
                        egui::ComboBox::from_id_salt("anim")
                            .selected_text(current_label)
                            .width(ui.available_width() - 8.0)
                            .height(400.0)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(anim_index.is_none(), "none (still)").clicked() {
                                    anim_request = Some(None);
                                }
                                for (i, (label, _)) in anim_list.iter().enumerate() {
                                    if ui.selectable_label(anim_index == Some(i), label).clicked() {
                                        anim_request = Some(Some(i));
                                    }
                                }
                            });
                        ui.small(format!("{} animations", anim_list.len()));
                        ui.add(egui::Slider::new(&mut crossfade, 0.0..=1.0).text("crossfade (s)"))
                            .on_hover_text("Time to ease from the current pose into a newly picked animation");
                        if let Some(c) = &mut anim {
                            ui.horizontal(|ui| {
                                if ui.button(if playing { "Pause" } else { "Play" }).clicked() {
                                    playing = !playing;
                                }
                                if ui.button("Restart").clicked() {
                                    c.frame = 0.0;
                                    if let Some(b) = &mut blend {
                                        b.frame = 0.0;
                                    }
                                    playing = true;
                                }
                                ui.checkbox(&mut loop_anim, "Loop");
                            });
                            let last = c.last();
                            if ui.add(egui::Slider::new(&mut c.frame, 0.0..=last).text("frame")).dragged() {
                                playing = false;
                            }
                            ui.add(egui::Slider::new(&mut speed, 0.1..=2.0).text("speed"));
                            let a = &c.anim;
                            let driven = c.track_of_bone.iter().filter(|t| t.is_some()).count();
                            ui.small(format!(
                                "{} frames at {} fps ({:.2} s){} · {} tracks, {} drive bones",
                                a.frames,
                                a.fps,
                                a.frames as f32 / a.fps as f32,
                                if a.looping { ", looping" } else { "" },
                                a.tracks.len(),
                                driven
                            ));
                        }
                        if let Some(e) = &anim_error {
                            ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                        }
                        if anim.is_some() {
                            ui.add_space(6.0);
                            ui.label("Blend with");
                            let blend_label = blend_index
                                .and_then(|i| anim_list.get(i))
                                .map(|(l, _)| l.as_str())
                                .unwrap_or("none");
                            egui::ComboBox::from_id_salt("blend")
                                .selected_text(blend_label)
                                .width(ui.available_width() - 8.0)
                                .height(400.0)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(blend_index.is_none(), "none").clicked() {
                                        blend_request = Some(None);
                                    }
                                    for (i, (label, _)) in anim_list.iter().enumerate() {
                                        if ui.selectable_label(blend_index == Some(i), label).clicked() {
                                            blend_request = Some(Some(i));
                                        }
                                    }
                                });
                            if let Some(b) = &blend {
                                ui.add(egui::Slider::new(&mut blend_weight, 0.0..=1.0).text("weight"))
                                    .on_hover_text("0 shows only the first animation, 1 only this one");
                                ui.checkbox(&mut blend_sync, "Sync to first animation").on_hover_text(
                                    "Keeps both at the same point of their cycle, stretching this one to the first \
                                     one's length; when off, it plays at its own frame rate",
                                );
                                let driven = b.track_of_bone.iter().filter(|t| t.is_some()).count();
                                ui.small(format!(
                                    "{} frames at {} fps · {} drive bones",
                                    b.anim.frames, b.anim.fps, driven
                                ));
                            }
                            if let Some(e) = &blend_error {
                                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                            }
                        }
                    }
                    }
                    if let Some(e) = &error {
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                    }
                    ui.separator();
                    ui.small(if category == Category::Sound {
                        "Click: play · Up/Down: next/previous · Space: play/pause · Click the waveform: jump"
                    } else if textures_shown {
                        "Click: select · Double-click: open large · Arrows: next/previous · Ctrl+wheel: zoom · Esc: grid"
                    } else {
                        "Left drag: orbit · Right drag: pan · Wheel: zoom · F7: play mode"
                    });
                    ui.separator();
                    ui.small(format!("RomFS: {romfs_shown}"));
                    if ui.small_button("Change RomFS folder…").clicked() {
                        romfs_pick = pick_romfs_folder();
                    }
                    if let Some(e) = &romfs_error {
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                    }
                });
                panel_width = resp.response.rect.width();

                let right = egui::Panel::right("details").default_size(320.0).resizable(true).show_inside(ui, |ui| {
                  egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if category == Category::World {
                        if let Some(w) = &world {
                            w.details(ui, control.target);
                        }
                        return;
                    }
                    if category == Category::Textures {
                        if let Some(g) = gallery.as_mut() {
                            g.details(ui);
                        }
                        return;
                    }
                    if category == Category::Sound {
                        if let Some(t) = sound_tab.as_mut() {
                            t.details(ui);
                        }
                        return;
                    }
                    if let Some(c) = &current {
                        ui.heading(short_name(&c.model.path));
                        ui.small(&c.model.path);
                        ui.label(format!("mesh file: {}", c.model.mesh_file));
                        ui.label(format!("{} parts, {} triangles", c.model.submeshes.len(), c.model.triangle_count()));
                        let size = Vec3::from(c.model.max) - Vec3::from(c.model.min);
                        ui.label(format!("size: {:.2} x {:.2} x {:.2}", size.x, size.y, size.z));
                        ui.collapsing("Parts", |ui| {
                            for s in &c.model.submeshes {
                                ui.small(format!("{}  [{}]  {} tris", s.shape, s.material, s.indices.len() / 3));
                            }
                        });
                        ui.collapsing("Materials", |ui| {
                            let mut names: Vec<&String> = c.baked.keys().collect();
                            names.sort();
                            let desc = |i: &Option<texture::Image>| match i {
                                Some(i) => format!("{}x{} {}", i.width, i.height, i.format),
                                None => "none".to_string(),
                            };
                            for n in names {
                                let b = &c.baked[n];
                                let kind = c.model.materials.get(n).map(|m| m.render_type.as_str()).unwrap_or("");
                                ui.small(format!(
                                    "{n} ({kind}): colour {}, normal {}",
                                    desc(&b.albedo),
                                    desc(&b.normal)
                                ));
                            }
                        });
                        if !c.warnings.is_empty() {
                            ui.collapsing(format!("Texture warnings ({})", c.warnings.len()), |ui| {
                                for w in &c.warnings {
                                    ui.small(w);
                                }
                            });
                        }
                    }

                    ui.separator();
                    ui.heading("Skeleton");
                    if let Some(e) = &skel_error {
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                    }
                    if let Some(s) = &skel {
                        let joints = s.bones.iter().filter(|b| b.joint.is_some()).count();
                        ui.small(format!("{}: {} bones, {} skinning joints", s.file, s.bones.len(), joints));
                        if let Some(err) = s.max_bind_error().filter(|e| *e > 1e-3) {
                            ui.small(format!(
                                "Bind and rest pose differ here (up to {err:.2}); the bind pose matches the mesh."
                            ));
                        }
                        // Details of the selected bone.
                        if let Some(i) = selected_bone {
                            let b = &s.bones[i];
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.strong(&b.name);
                                let parent = b.parent.map(|p| s.bones[p].name.as_str()).unwrap_or("none");
                                ui.small(format!("parent: {parent}"));
                                ui.small(match b.joint {
                                    Some(j) => format!("skinning joint {j}"),
                                    None => "helper bone (not skinned)".to_string(),
                                });
                                let deg = b.rotation.map(|r| r.to_degrees());
                                ui.small(format!(
                                    "local T {:.3} {:.3} {:.3}",
                                    b.translation[0], b.translation[1], b.translation[2]
                                ));
                                ui.small(format!("local R {:.1}° {:.1}° {:.1}°", deg[0], deg[1], deg[2]));
                                ui.small(format!("local S {:.3} {:.3} {:.3}", b.scale[0], b.scale[1], b.scale[2]));
                                let p = s.position(i, bind_pose);
                                ui.small(format!("position {:.3} {:.3} {:.3}", p.x, p.y, p.z));
                                if ui.small_button("Clear selection").clicked() {
                                    selected_bone = None;
                                }
                            });
                        }
                        let before = selected_bone;
                        ui.collapsing(format!("Bones ({})", s.bones.len()), |ui| {
                            for root in (0..s.bones.len()).filter(|i| s.bones[*i].parent.is_none()) {
                                bone_tree(ui, s, root, 0, &mut selected_bone);
                            }
                        });
                        if selected_bone != before {
                            skel_dirty = true;
                            show_skeleton = true;
                        }
                    }
                  });
                });
                right_width = right.response.rect.width();
                if category == Category::Textures
                    && let Some(g) = gallery.as_mut()
                {
                    egui::CentralPanel::default().show_inside(ui, |ui| g.central(ui));
                }
                if category == Category::Sound
                    && let Some(t) = sound_tab.as_mut()
                {
                    egui::CentralPanel::default().show_inside(ui, |ui| t.central(ui));
                }
            },
        );

        // Keyboard: F7 toggles play mode; in it, the movement keys are held or released. Keys typed into a text
        // field are already marked handled by the GUI. Releases always count, so no key stays stuck.
        for e in frame_input.events.iter_mut() {
            match e {
                Event::KeyPress {
                    kind,
                    modifiers,
                    handled,
                } => {
                    shift = modifiers.shift;
                    if *handled {
                        continue;
                    }
                    match *kind {
                        Key::F7 if category != Category::World && !category.flat() => demo_toggle = true,
                        Key::Escape if demo.is_some() => demo_toggle = true,
                        k if demo.is_some() && DEMO_KEYS.contains(&k) => {
                            if k == Key::Space && !keys_held.contains(&k) {
                                jump_queued = true;
                            }
                            keys_held.insert(k);
                        }
                        _ => continue,
                    }
                    *handled = true;
                }
                Event::KeyRelease { kind, modifiers, .. } => {
                    shift = modifiers.shift;
                    keys_held.remove(kind);
                }
                Event::ModifiersChange { modifiers } => shift = modifiers.shift,
                _ => {}
            }
        }

        // The 3D view takes the space between the two panels.
        let px = (panel_width * frame_input.device_pixel_ratio) as u32;
        let rx = (right_width * frame_input.device_pixel_ratio) as u32;
        let vp = Viewport {
            x: px as i32,
            y: 0,
            width: frame_input.viewport.width.saturating_sub(px + rx).max(1),
            height: frame_input.viewport.height,
        };
        camera.set_viewport(vp);
        let textures_shown = category.flat();
        if !textures_shown {
            world::pan(&mut camera, &mut control, &mut frame_input.events);
            control.handle_events(&mut camera, &mut frame_input.events);
        }

        let screen = frame_input.screen();
        screen.clear(ClearState::color_and_depth(0.13, 0.14, 0.17, 1.0, 1.0));
        let unlit: [&dyn Light; 4] = [&ambient, &key, &fill, &rim];
        if let (true, Some(w)) = (world_shown, &world) {
            sun.clear_shadow_map();
            let lit: [&dyn Light; 3] = [&sky, &sun, &bounce];
            let lights: &[&dyn Light] = match lighting {
                Lighting::Unlit => &unlit,
                Lighting::Lit => &lit,
            };
            w.render(&screen, &camera, lights);
        } else if let (false, Some(c)) = (textures_shown, &current) {
            let floor = show_floor.then_some(match (&demo, &demo_floor) {
                (Some(_), Some(f)) => &f.gm,
                _ => &c.floor,
            });
            if lighting == Lighting::Lit {
                // redone every frame, as the pose changes; only the model casts, so the map covers just it
                if show_mesh {
                    sun.generate_shadow_map(SHADOW_MAP_SIZE, &c.meshes).ok();
                } else {
                    sun.clear_shadow_map();
                }
            }
            let lit: [&dyn Light; 3] = [&sky, &sun, &bounce];
            let lights: &[&dyn Light] = match lighting {
                Lighting::Unlit => &unlit,
                Lighting::Lit => &lit,
            };
            let meshes = c.meshes.iter().filter(|_| show_mesh).map(|m| m as &dyn Object);
            screen.render(&camera, meshes.chain(floor.map(|f| f as &dyn Object)), lights);
        }
        // Drawn after the mesh so that, in x-ray mode, it shows through it.
        if let (true, false, false, Some((bones, joints))) = (show_skeleton, world_shown, textures_shown, &skel_gm) {
            screen.render(&camera, [bones as &dyn Object, joints as &dyn Object], &[]);
        }
        screen.write(|| gui.render()).ok();
        FrameOutput::default()
    });
    Ok(())
}
