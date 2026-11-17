#![allow(dead_code)] // shares the viewer modules but uses only part of them
//! Development probe: prints the slot layout of a Trinity FlatBuffers file from the archive and runs format checks
//! over a whole dump. Uses $TRINITY_ROMFS, else the RomFS folder the viewer remembers.
//!   cargo run --release --bin probe -- <archive path> [depth]
//!   cargo run --release --bin probe -- --layers <trmdl path>   material layers and texture means
//!   cargo run --release --bin probe -- --survey                texture roles and formats over all models
//!   cargo run --release --bin probe -- --skeletons             skeleton maths vs. inverse bind matrices, all models
//!   cargo run --release --bin probe -- --skel <trmdl path>     bones whose rest and bind pose differ
//!   cargo run --release --bin probe -- --anims                 parse every animation of every 10th model
//!   cargo run --release --bin probe -- --quat-layouts          packed quaternion layouts vs. rest poses
//!   cargo run --release --bin probe -- --skin                  skin weights and joint indices, all models
//!   cargo run --release --bin probe -- --load <trmdl path>...  load models as the viewer does, print what failed
//!   cargo run --release --bin probe -- --pack <path> [names]  every file in that path's pack (hash, size, head)
//!   cargo run --release --bin probe -- --save <path|#hash> <out>  raw file bytes, for study (keep out of the repo)
//!   cargo run --release --bin probe -- --terrain              load every overworld tile's heights, check seams
//!   cargo run --release --bin probe -- --heightmap <XX_YY>      a tile's R16 heightmap vs. its height file, hole edges
//!   cargo run --release --bin probe -- --lod-survey           LOD data of placed models: ModelComponent slots, trmdl slot 4
//!   cargo run --release --bin probe -- --scene-packs [model]   every scene in the field scenes' packs; which place [model]
//!   cargo run --release --bin probe -- --town-scenes          every town scene: placements, models, instance files
//!   cargo run --release --bin probe -- --scene-tree <trscn> [n]  objects, transforms, models, instance files
//!   cargo run --release --bin probe -- --scene <trscn> [n] [kind]  node kinds, then n nodes with their data's slots
//!   cargo run --release --bin probe -- --tile-instances       every tile's tree instance files and their models
//!   cargo run --release --bin probe -- --guess-ins <path> <stems file> <dir,dir>  name a pack's .trins by brute force
//!   cargo run --release --bin probe -- --instances <path>     every file of that pack as instances, vs. the ground
//!   cargo run --release --bin probe -- --pack-survey <path>   a pack's files grouped by kind
//!   cargo run --release --bin probe -- --split <prefix/> <rest>  find the slashes of a pack name's path (up to 3)
//!   cargo run --release --bin probe -- --grass-cells <XX_YY>   height file slots 12/13 (grass?) vs. splat layers
//!   cargo run --release --bin probe -- --terrain-materials    switches of every tile material, splat params per layer
//!   cargo run --release --bin probe -- --hole-regions        sea vs. inland holes over the stitched map, their edges
//!   cargo run --release --bin probe -- --holes              heights next to holes (skirts?), every tile
//!   cargo run --release --bin probe -- --texture <bntx> <layer> <out>  decoded RGBA8 of one layer (raw, for study)
//!   cargo run --release --bin probe -- --splat                every tile's splat names vs. the layers its control map uses
//!   cargo run --release --bin probe -- --splat-seams          do neighbouring control maps agree along shared edges?
//!   cargo run --release --bin probe -- --vfxb [path]          every .ptcl parsed: field value counts, textures found; or one file's emitters
//!   cargo run --release --bin probe -- --effects               the Effects tab's list: counts by kind and group, paths not rebuilt
//!   cargo run --release --bin probe -- --characters            list the character models discovery finds
//!   cargo run --release --bin probe -- --material <trmdl> <name part>  every field of the matching materials
//!   cargo run --release --bin probe -- --eyes                  Eye / EyeClearCoat materials: highlight switches and values
//!   cargo run --release --bin probe -- --transparency          shader, render type and alpha switches of every material
//!   cargo run --release --bin probe -- --map-tiles             every terrain tile of every grid: origin, size, height range
//!   cargo run --release --bin probe -- --rooms                 every room scene found from the pack names: sub-scenes, models
//!   cargo run --release --bin probe -- --water                 the overworld's water parts vs. the ground under them
//!   cargo run --release --bin probe -- --textures [expand|decode]  the Textures tab's catalogue: groups, folders,
//!                                                     paths not rebuilt; expand: every folder's other pack files;
//!                                                     decode: also decode every texture and count failures
//!   cargo run --release --bin probe -- --audio [names|decode [N]]  the Wwise sounds: groups, codecs, durations;
//!                                                     names: every name found; decode:
//!                                                     decode every Nth sound (default all), count failures
//!   cargo run --release --bin probe -- --grep <text> [pack filter]  files containing <text> (ASCII or UTF-16), in
//!                                                     the packs whose name contains [pack filter] (default all)
//!   cargo run --release --bin probe -- --trainer-anims         every human model: own clip list, named and best-fit body type
#[path = "../anim.rs"]
mod anim;
#[path = "../archive.rs"]
mod archive;
#[path = "../config.rs"]
mod config;
#[path = "../effects.rs"]
mod effects;
#[path = "../fb.rs"]
mod fb;
#[path = "../instances.rs"]
mod instances;
#[path = "../maps.rs"]
mod maps;
#[path = "../materials.rs"]
mod materials;
#[path = "../model.rs"]
mod model;
#[path = "../motion.rs"]
mod motion;
#[path = "../msg.rs"]
mod msg;
#[path = "../player.rs"]
mod player;
#[path = "../scene.rs"]
mod scene;
#[path = "../skeleton.rs"]
mod skeleton;
#[path = "../sound_names.rs"]
mod sound_names;
#[path = "../terrain.rs"]
mod terrain;
#[path = "../texture.rs"]
mod texture;
#[path = "../textures.rs"]
mod textures;
#[path = "../vfxb.rs"]
mod vfxb;
#[path = "../wem.rs"]
mod wem;
#[path = "../wwise.rs"]
mod wwise;

use fb::{Table, u32_at};

fn dump(t: &Table, depth: usize, max_depth: usize) {
    let pad = "  ".repeat(depth);
    for i in 0..t.slot_count() {
        let Some(a) = t.field(i) else { continue };
        let raw = u32_at(t.buf, a).unwrap_or(0);
        let mut line = format!("{pad}[{i}] u32={raw} f32={:.4}", f32::from_bits(raw));
        if let Ok(Some(s)) = t.string(i)
            && !s.is_empty()
            && s.len() < 260
            && s.chars().all(|c| c.is_ascii_graphic())
        {
            line += &format!(" str={s:?}");
        }
        if let Ok((_, n)) = t.vector(i)
            && n < 100000
        {
            line += &format!(" vec_len={n}");
        }
        println!("{line}");
        // a vector of strings (e.g. file lists): print its entries
        if let Ok(v) = t.strings(i)
            && !v.is_empty()
            && v.iter()
                .all(|s| !s.is_empty() && s.len() < 200 && s.chars().all(|c| c.is_ascii_graphic()))
        {
            for (k, s) in v.iter().enumerate().take(40) {
                println!("{pad}  [{i}][{k}] str={s:?}");
            }
            continue;
        }
        if depth / 2 < max_depth {
            if let Ok(v) = t.tables(i)
                && !v.is_empty()
                && v.len() < 200
                && v[0].slot_count() < 24
            {
                for (k, e) in v.iter().enumerate().take(if depth == 0 { 3 } else { 40 }) {
                    println!("{pad}  [{i}][{k}]:");
                    dump(e, depth + 2, max_depth);
                }
                continue;
            }
            if let Ok(Some(sub)) = t.table(i)
                && sub.slot_count() < 24
            {
                println!("{pad}  (table)");
                dump(&sub, depth + 2, max_depth);
            }
        }
    }
}

/// Puts every player part on the base skeleton, one at a time, and the default outfit as a whole. Reports load
/// failures, attach warnings, and skinned vertices that point at a joint without a bone (they would not move).
fn player_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    let parts = player::catalog(ar);
    let mut by_slot = std::collections::BTreeMap::new();
    for p in &parts {
        *by_slot
            .entry(format!("{} (p{})", p.slot.label(), p.body))
            .or_insert(0) += 1;
    }
    println!("{} parts: {by_slot:?}", parts.len());
    let check = |ar: &mut archive::Archive, paths: &[&str]| -> (usize, usize, Vec<String>) {
        match player::assemble(ar, paths, 0) {
            Ok(a) => {
                let (mut bad, mut total) = (0, 0);
                for s in &a.model.submeshes {
                    let (Some(j), Some(w)) = (&s.joints, &s.weights) else {
                        continue;
                    };
                    for (j, w) in j.iter().zip(w) {
                        total += 1;
                        if (0..4)
                            .any(|k| w[k] > 0.0 && a.skeleton.joint_bone(j[k] as usize).is_none())
                        {
                            bad += 1;
                        }
                    }
                }
                let anims = anim::list_at(ar, player::ANIM_LIST, player::ANIM_STEM).len();
                let mut w = a.warnings;
                if anims == 0 {
                    w.push("no animations".into());
                }
                (bad, total, w)
            }
            Err(e) => (0, 0, vec![format!("FAIL {e:#}")]),
        }
    };
    let default = player::Outfit::default_for(&parts);
    let paths = default.paths(&parts);
    let (bad, total, w) = check(ar, &paths);
    println!(
        "default outfit {paths:?}: {bad} of {total} vertices on a joint without a bone, warnings {w:?}"
    );
    let mut failed = 0;
    for p in &parts {
        let (bad, total, w) = check(ar, &[p.path.as_str()]);
        if bad > 0 || !w.is_empty() {
            failed += 1;
            println!(
                "{}: {bad} of {total} vertices unmoved, warnings {w:?}",
                p.path
            );
        }
    }
    println!("{} of {} parts clean", parts.len() - failed, parts.len());
    let list = anim::list_at(ar, player::ANIM_LIST, player::ANIM_STEM);
    // do the clips' bone names match the assembled skeleton?
    if let Ok(a) = player::assemble(ar, &paths, 0) {
        for (label, path) in list.iter().filter(|(l, _)| {
            ["defaultwait01_loop", "walk01_loop", "run01_loop"]
                .iter()
                .any(|w| l.ends_with(w))
        }) {
            match anim::load(ar, path, label) {
                Ok(c) => {
                    let matched = c
                        .tracks
                        .iter()
                        .filter(|t| a.skeleton.bones.iter().any(|b| b.name == t.name))
                        .count();
                    println!(
                        "clip {label}: {} frames, {matched} of {} tracks on a bone",
                        c.frames,
                        c.tracks.len()
                    );
                }
                Err(e) => println!("clip {label}: FAIL {e:#}"),
            }
        }
    }
    println!(
        "{} animations, first: {:?}",
        list.len(),
        list.iter()
            .take(8)
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
    );
    Ok(())
}

/// Loads one model the way the viewer does (mesh, skeleton, baked materials) and prints counts and every warning.
fn load_report(ar: &mut archive::Archive, path: &str) {
    println!("== {path}");
    // "path@N" loads LOD N
    let (path, lod) = match path.rsplit_once('@') {
        Some((p, l)) => (p, l.parse().unwrap_or(0)),
        None => (path, 0),
    };
    let m = match model::load(ar, path, lod) {
        Ok(m) => m,
        Err(e) => {
            println!("  FAIL model: {e:#}");
            return;
        }
    };
    let skinned = m.submeshes.iter().filter(|s| s.joints.is_some()).count();
    println!(
        "  {} mesh files, {} parts ({skinned} skinned), {} triangles, {} materials, size {:.3} x {:.3} x {:.3}",
        m.mesh_files,
        m.submeshes.len(),
        m.triangle_count(),
        m.materials.len(),
        m.max[0] - m.min[0],
        m.max[1] - m.min[1],
        m.max[2] - m.min[2],
    );
    println!("  bounds {:?} .. {:?}", m.min, m.max);
    match &m.skeleton_file {
        Some(f) => match skeleton::load(ar, &m.dir, f) {
            Ok(s) => {
                let joints = s.inverse_bind.iter().filter(|j| j.is_some()).count();
                let max_joint = m
                    .submeshes
                    .iter()
                    .filter_map(|s| s.joints.as_ref())
                    .flatten()
                    .flatten()
                    .max()
                    .copied();
                println!(
                    "  skeleton {f}: {} bones, {joints} joints, highest joint index used {max_joint:?}",
                    s.bones.len()
                );
            }
            Err(e) => println!("  FAIL skeleton {f}: {e:#}"),
        },
        None => println!("  no skeleton"),
    }
    let (baked, warnings) = materials::bake(ar, &m);
    let textured = baked.values().filter(|b| b.albedo.is_some()).count();
    println!(
        "  materials: {textured} of {} with a colour texture",
        baked.len()
    );
    let mut names: Vec<_> = baked.keys().collect();
    names.sort();
    for n in names {
        let b = &baked[n];
        println!(
            "  baked {n}: alpha {:?}, albedo {}",
            b.alpha,
            b.albedo.as_ref().map_or("none".to_string(), |a| format!(
                "{}x{} {}{}",
                a.width,
                a.height,
                a.format,
                if a.has_alpha() { " with alpha" } else { "" }
            ))
        );
    }
    for w in &warnings {
        println!("  warn {w}");
    }
    for sm in &m.submeshes {
        println!(
            "  part {:?}: material {:?} ({}), {} triangles, uvs {}",
            sm.shape,
            sm.material,
            m.materials
                .get(&sm.material)
                .map_or("NOT IN THE MODEL", |x| x.shader.as_str()),
            sm.indices.len() / 3,
            sm.uvs.is_some()
        );
    }
    for (name, mat) in &m.materials {
        println!(
            "  material {name}: render {:?}, roles {:?}",
            mat.render_type,
            mat.textures
                .iter()
                .map(|(r, _)| r.as_str())
                .collect::<Vec<_>>()
        );
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // the RomFS folder: $TRINITY_ROMFS, else the one the viewer remembers
    let romfs = std::env::var_os("TRINITY_ROMFS")
        .map(std::path::PathBuf::from)
        .or_else(config::load_romfs_path)
        .ok_or_else(|| {
            anyhow::anyhow!("set TRINITY_ROMFS or pick a RomFS folder in the viewer first")
        })?;
    let mut ar = archive::Archive::open(&romfs)?;
    if args[0] == "--layers" {
        return layers(&mut ar, &args[1]);
    }
    if args[0] == "--skel" {
        return skel_detail(&mut ar, &args[1]);
    }
    if args[0] == "--quat-layouts" {
        return quat_layouts(&mut ar);
    }
    if args[0] == "--animlist" {
        anim_list(&mut ar, &args[1]);
        return Ok(());
    }
    if args[0] == "--anims" {
        return anim_check(&mut ar);
    }
    if args[0] == "--skin" {
        return skin_check(&mut ar);
    }
    if args[0] == "--quat" {
        return quat_test(&mut ar);
    }
    if args[0] == "--has" {
        has(&ar, &args[1..]);
        return Ok(());
    }
    if args[0] == "--pack" {
        return pack_list(&mut ar, &args[1], &args[2..]);
    }
    if args[0] == "--save" {
        // raw bytes of a file (path or #hash) to a local file, for study outside the repo
        let b = match args[1].strip_prefix("#") {
            Some(h) => ar.read_hash(u64::from_str_radix(h, 16)?)?,
            None => ar.read(&args[1])?,
        };
        std::fs::write(&args[2], &b)?;
        println!("{} bytes", b.len());
        return Ok(());
    }
    if args[0] == "--rooms" {
        return rooms_check(&mut ar);
    }
    if args[0] == "--map-tiles" {
        return map_tiles(&mut ar);
    }
    if args[0] == "--water" {
        return water_check(&mut ar);
    }
    if args[0] == "--terrain" {
        return terrain_check(&mut ar);
    }
    if args[0] == "--heightmap" {
        return heightmap_check(&mut ar, &args[1]);
    }
    if args[0] == "--lod-survey" {
        return lod_survey(&mut ar);
    }
    if args[0] == "--scene-packs" {
        return scene_packs(&mut ar, args.get(1).map_or("", String::as_str));
    }
    if args[0] == "--town-scenes" {
        return town_scenes_check(&mut ar);
    }
    if args[0] == "--scene-tree" {
        let limit = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(80);
        return scene_tree(&mut ar, &args[1], limit);
    }
    if args[0] == "--scene" {
        let limit = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(40);
        return scene_dump(&mut ar, &args[1], limit, args.get(3).map(String::as_str));
    }
    if args[0] == "--tile-instances" {
        return tile_instances_check(&mut ar);
    }
    if args[0] == "--guess-ins" {
        return guess_ins(&mut ar, &args[1], &args[2], &args[3]);
    }
    if args[0] == "--instances" {
        return instances_check(&mut ar, &args[1]);
    }
    if args[0] == "--pack-survey" {
        return pack_survey(&mut ar, &args[1]);
    }
    if args[0] == "--split" {
        split_path(&ar, &args[1], &args[2]);
        return Ok(());
    }
    if args[0] == "--grass-cells" {
        return grass_cells(&mut ar, &args[1]);
    }
    if args[0] == "--terrain-materials" {
        return terrain_materials(&mut ar);
    }
    if args[0] == "--hole-regions" {
        return hole_regions(&mut ar);
    }
    if args[0] == "--holes" {
        return holes_check(&mut ar);
    }
    if args[0] == "--texture" {
        // decoded RGBA8 of one array layer to a raw file (width, height as u32 first), for study outside the repo
        let b = ar.read(&args[1])?;
        let layer: u32 = args[2].parse()?;
        let img = texture::decode_bntx_layer(&b, layer)?;
        let mut out = Vec::with_capacity(8 + img.rgba.len() * 4);
        out.extend(img.width.to_le_bytes());
        out.extend(img.height.to_le_bytes());
        out.extend(img.rgba.iter().flatten());
        std::fs::write(&args[3], out)?;
        println!(
            "{}x{} {} ({} layers)",
            img.width,
            img.height,
            img.format,
            texture::layer_count(&b)?
        );
        return Ok(());
    }
    if args[0] == "--splat-seams" {
        return splat_seams(&mut ar);
    }
    if args[0] == "--splat" {
        return splat_check(&mut ar);
    }
    if args[0] == "--packs" {
        packs(&ar, &args[1]);
        return Ok(());
    }
    if args[0] == "--skeletons" {
        return skeletons(&mut ar);
    }
    if args[0] == "--survey" {
        return survey(&mut ar);
    }
    if args[0] == "--bones" {
        // every bone of a .trskl: index, name, parent, joint index
        let path = &args[1];
        let s = skeleton::parse(&ar.read(path)?, path)?;
        let joints = s.inverse_bind.len();
        println!("{} bones, {joints} joint slots", s.bones.len());
        for (i, b) in s.bones.iter().enumerate() {
            let parent = b.parent.map(|p| s.bones[p].name.as_str()).unwrap_or("-");
            println!("{i:4} {:32} parent {parent:32} joint {:?}", b.name, b.joint);
        }
        return Ok(());
    }
    if args[0] == "--player" {
        return player_check(&mut ar);
    }
    if args[0] == "--material" && args.len() >= 3 {
        // a .trmdl (its materials) or a .trmtr file itself
        let materials = if args[1].ends_with(".trmtr") {
            model::parse_materials(&ar.read(&args[1])?)?
        } else {
            model::load(&mut ar, &args[1], 0)?.materials
        };
        struct M {
            materials: std::collections::HashMap<String, model::Material>,
        }
        let m = M { materials };
        let mut names: Vec<&String> = m
            .materials
            .keys()
            .filter(|n| n.contains(args[2].as_str()))
            .collect();
        names.sort();
        for n in names {
            let mat = &m.materials[n];
            println!(
                "== {n}: shader {:?}, render type {:?}",
                mat.shader, mat.render_type
            );
            println!("  textures {:?}", mat.textures);
            let mut sw: Vec<_> = mat.switches.iter().collect();
            sw.sort();
            println!("  switches {sw:?}");
            let mut f: Vec<_> = mat.floats.iter().collect();
            f.sort_by(|a, b| a.0.cmp(b.0));
            println!("  floats {f:?}");
            let mut p: Vec<_> = mat.params.iter().collect();
            p.sort_by(|a, b| a.0.cmp(b.0));
            for (k, v) in p {
                println!("  {k} {v:?}");
            }
        }
        return Ok(());
    }
    if args[0] == "--eyes" {
        return eyes(&mut ar);
    }
    if args[0] == "--transparency" {
        return transparency(&mut ar);
    }
    if args[0] == "--trainer-anims" {
        return trainer_anims(&mut ar);
    }
    if args[0] == "--grep" {
        return grep_archive(
            &mut ar,
            &args[1],
            args.get(2).map(String::as_str).unwrap_or(""),
        );
    }
    if args[0] == "--audio" {
        return audio_survey(
            &romfs,
            args.get(1).map(String::as_str),
            args.get(2).and_then(|n| n.parse().ok()),
        );
    }
    if args[0] == "--textures" {
        return texture_catalogue(&mut ar, args.get(1).map(String::as_str));
    }
    if args[0] == "--vfxb" {
        return vfxb_check(&mut ar, args.get(1).map(String::as_str));
    }
    if args[0] == "--effects" {
        return effect_catalogue(&ar);
    }
    if args[0] == "--characters" {
        for p in ar.character_models() {
            println!("{p}");
        }
        return Ok(());
    }
    if args[0] == "--load" {
        for path in &args[1..] {
            load_report(&mut ar, path);
        }
        return Ok(());
    }
    // a path, or "#<hex hash>" for a file whose name is not known yet (see --pack)
    let b = match args[0].strip_prefix("#") {
        Some(h) => ar.read_hash(u64::from_str_radix(h, 16)?)?,
        None => ar.read(&args[0])?,
    };
    println!("{} bytes, head {:02x?}", b.len(), &b[..16.min(b.len())]);
    if b.starts_with(b"BNTX") {
        for row in b[..0x40].chunks(16) {
            println!("  {row:02x?}");
        }
        let info_ptrs = u64::from_le_bytes(b[0x28..0x30].try_into()?) as usize;
        let brti = u64::from_le_bytes(b[info_ptrs..info_ptrs + 8].try_into()?) as usize;
        println!("info ptr array {info_ptrs:#x} -> BRTI {brti:#x}");
        for row in b[brti..brti + 0x80].chunks(16) {
            println!("  {row:02x?}");
        }
        return Ok(());
    }
    dump(
        &Table::root(&b, 0)?,
        0,
        args.get(1).and_then(|d| d.parse().ok()).unwrap_or(3),
    );
    Ok(())
}

fn survey(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let mut roles: BTreeMap<String, usize> = BTreeMap::new();
    let mut params: BTreeMap<String, usize> = BTreeMap::new();
    let mut fmts: BTreeMap<String, usize> = BTreeMap::new();
    let mut errs: BTreeMap<String, usize> = BTreeMap::new();
    let (mut mats, mut no_base, mut used_mats) = (0, 0, 0);
    let models = ar.pokemon_models();
    for (mi, m) in models.iter().enumerate() {
        let Ok(md) = model::load(ar, m, 0) else {
            continue;
        };
        mats += md.materials.len();
        for mat in md.materials.values() {
            for (r, _) in &mat.textures {
                *roles.entry(r.clone()).or_default() += 1;
            }
            for p in mat.params.keys() {
                *params.entry(p.clone()).or_default() += 1;
            }
        }
        for s in &md.submeshes {
            used_mats += 1;
            let Some(mat) = md.materials.get(&s.material) else {
                *errs.entry("material missing".into()).or_default() += 1;
                continue;
            };
            let Some(f) = mat.texture("BaseColorMap") else {
                no_base += 1;
                continue;
            };
            if mi % 4 != 0 {
                continue;
            } // decode a quarter of them to keep the run short
            match ar
                .read(&model::resolve(&md.dir, f))
                .and_then(|b| texture::decode_bntx(&b))
            {
                Ok(img) => *fmts.entry(img.format).or_default() += 1,
                Err(e) => {
                    let k = format!("{e:#}");
                    if !errs.contains_key(&k) {
                        println!("{m}: {f}: {k}");
                    }
                    *errs.entry(k).or_default() += 1
                }
            }
        }
    }
    println!("materials {mats}; submeshes {used_mats}, {no_base} without BaseColorMap");
    println!("texture roles: {roles:?}");
    println!(
        "params (top): {:?}",
        params.iter().filter(|(_, n)| **n > 300).collect::<Vec<_>>()
    );
    println!("base colour formats: {fmts:?}");
    println!("errors: {errs:?}");
    Ok(())
}

#[allow(dead_code)]
pub fn layers(ar: &mut archive::Archive, m: &str) -> anyhow::Result<()> {
    let md = model::load(ar, m, 0)?;
    let mut names: Vec<_> = md.materials.keys().cloned().collect();
    names.sort();
    for n in names {
        let mat = &md.materials[&n];
        let mean = |ar: &mut archive::Archive, role: &str| -> Option<(String, [f32; 4])> {
            let f = mat.texture(role)?;
            let img = texture::decode_bntx(&ar.read(&model::resolve(&md.dir, f)).ok()?).ok()?;
            let mut s = [0f32; 4];
            for p in &img.rgba {
                for k in 0..4 {
                    s[k] += p[k] as f32;
                }
            }
            Some((
                img.format.clone(),
                s.map(|v| v / img.rgba.len() as f32 / 255.0),
            ))
        };
        println!(
            "{n} [{}] shader {:?}",
            mat.render_type,
            mat.textures
                .iter()
                .map(|t| t.0.as_str())
                .collect::<Vec<_>>()
        );
        println!(
            "   alb mean {:?}   mask mean {:?}",
            mean(ar, "BaseColorMap"),
            mean(ar, "LayerMaskMap")
        );
        for k in [
            "BaseColor",
            "BaseColorLayer1",
            "BaseColorLayer2",
            "BaseColorLayer3",
            "BaseColorLayer4",
            "BaseColorLayer5",
        ] {
            if let Some(v) = mat.params.get(k) {
                println!("   {k} {v:?}");
            }
        }
    }
    Ok(())
}

pub fn skeletons(ar: &mut archive::Archive) -> anyhow::Result<()> {
    let (mut n, mut none, mut fail) = (0, 0, 0);
    let mut errs: Vec<(f32, String)> = Vec::new();
    for m in ar.pokemon_models() {
        let Ok(md) = model::load(ar, &m, 0) else {
            continue;
        };
        let Some(f) = md.skeleton_file.clone() else {
            none += 1;
            continue;
        };
        match skeleton::load(ar, &md.dir, &f) {
            Ok(s) => {
                n += 1;
                if n <= 2 {
                    for b in s.bones.iter().take(6) {
                        println!(
                            "  {} parent {:?} joint {:?} s {:?} r {:?} t {:?}",
                            b.name, b.parent, b.joint, b.scale, b.rotation, b.translation
                        );
                    }
                }
                errs.push((
                    s.max_bind_error().unwrap_or(-1.0),
                    format!("{m} ({} bones)", s.bones.len()),
                ));
            }
            Err(e) => {
                fail += 1;
                println!("FAIL {m}: {e:#}");
            }
        }
    }
    errs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over = errs.iter().filter(|e| e.0 > 1e-3).count();
    println!("skeletons {n}, models without one {none}, failed {fail}; bind error > 0.001: {over}");
    println!("worst: {:?}", &errs[..errs.len().min(5)]);
    println!("median: {:?}", errs.get(errs.len() / 2));
    Ok(())
}

pub fn skel_detail(ar: &mut archive::Archive, m: &str) -> anyhow::Result<()> {
    use three_d::SquareMatrix;
    let md = model::load(ar, m, 0)?;
    let s = skeleton::load(ar, &md.dir, md.skeleton_file.as_deref().unwrap_or(""))?;
    for (i, b) in s.bones.iter().enumerate() {
        let Some(ibm) = b
            .joint
            .and_then(|j| s.inverse_bind.get(j).copied().flatten())
        else {
            continue;
        };
        let m = b.world * ibm;
        let id = three_d::Mat4::identity();
        let mut e = 0.0f32;
        for c in 0..4 {
            for r in 0..4 {
                e = e.max((m[c][r] - id[c][r]).abs());
            }
        }
        let pe = b.parent.map(|p| {
            let pb = &s.bones[p];
            (pb.name.clone(), pb.scale)
        });
        if e > 1e-3 {
            println!(
                "{i} {} err {e:.3} s {:?} r {:?} parent {:?}",
                b.name, b.scale, b.rotation, pe
            );
        }
    }
    Ok(())
}

pub fn packs(ar: &archive::Archive, pat: &str) {
    for n in ar.pack_names.iter().filter(|n| n.contains(pat)) {
        println!("{n}");
    }
}

/// Every file in the pack holding `path`: hash, size, first bytes, and the name if `path` or one of `names`
/// (relative to `path`'s folder) hashes to it.
pub fn pack_list(ar: &mut archive::Archive, path: &str, names: &[String]) -> anyhow::Result<()> {
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    let mut known: Vec<String> = vec![path.to_string()];
    known.extend(names.iter().map(|n| format!("{dir}/{n}")));
    let hashes = ar.pack_files(path)?;
    println!("{} files", hashes.len());
    for h in hashes {
        let name = known
            .iter()
            .find(|k| archive::fnv1a64(k) == h)
            .cloned()
            .unwrap_or_default();
        match ar.read_hash(h) {
            Ok(b) => println!(
                "{h:016x} {:>9} {:02x?} {name}",
                b.len(),
                &b[..b.len().min(16)]
            ),
            Err(e) => println!("{h:016x} error {e} {name}"),
        }
    }
    Ok(())
}

/// Loads every grid tile's heightfield; checks sizes, origins against `x*500, -y*500-500`, and how well shared
/// edges of neighbouring tiles agree with columns along x (as loaded) and with the grid transposed.
pub fn terrain_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::HashMap;
    let mut tiles = HashMap::new();
    for (x, y, path) in terrain::tiles(ar) {
        match terrain::load_heights(ar, &path) {
            Ok(t) => {
                let (lo, hi) = t
                    .heights
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)));
                let holes = t.heights.iter().filter(|h| **h == 0.0).count();
                let expect = [x as f32 * 500.0, 0.0, -(y as f32) * 500.0 - 500.0];
                println!(
                    "{x:02}_{y:02} {}x{} origin {:?}{} scale {:?} heights {lo:.1}..{hi:.1} holes {holes}",
                    t.width,
                    t.depth,
                    t.origin,
                    if t.origin == expect {
                        ""
                    } else {
                        " (UNEXPECTED)"
                    },
                    t.scale,
                );
                tiles.insert((x, y), t);
            }
            Err(e) => println!("{x:02}_{y:02} FAILED {e:#}"),
        }
    }
    // mean |difference| over samples where both sides are not holes
    let edge = |a: &[f32], b: &[f32]| {
        let (mut sum, mut n) = (0.0f64, 0);
        for (p, q) in a.iter().zip(b) {
            if *p != 0.0 && *q != 0.0 {
                sum += (p - q).abs() as f64;
                n += 1;
            }
        }
        if n == 0 { None } else { Some(sum / n as f64) }
    };
    let col = |t: &terrain::HeightTile, c: usize| {
        (0..t.depth).map(|r| t.height(c, r)).collect::<Vec<_>>()
    };
    let row = |t: &terrain::HeightTile, r: usize| {
        (0..t.width).map(|c| t.height(c, r)).collect::<Vec<_>>()
    };
    let (mut as_loaded, mut transposed) = (Vec::new(), Vec::new());
    for (&(x, y), a) in &tiles {
        // x neighbour: a's last column meets b's first column
        if let Some(b) = tiles.get(&(x + 1, y)) {
            as_loaded.extend(edge(&col(a, a.width - 1), &col(b, 0)));
            transposed.extend(edge(&row(a, a.depth - 1), &row(b, 0)));
        }
        // y + 1 is further towards -z: its last row meets a's first row
        if let Some(b) = tiles.get(&(x, y + 1)) {
            as_loaded.extend(edge(&row(b, b.depth - 1), &row(a, 0)));
            transposed.extend(edge(&col(b, b.width - 1), &col(a, 0)));
        }
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    println!(
        "{} tiles loaded; seams: as loaded {:.3} m mean over {} edges, transposed {:.3} m over {}",
        tiles.len(),
        mean(&as_loaded),
        as_loaded.len(),
        mean(&transposed),
        transposed.len()
    );
    Ok(())
}

/// For every tile: the splat names of its height file (slot 15) and the layer indices (R and G) its control map
/// uses, with how many texels use each.
pub fn splat_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    let mut all = std::collections::BTreeSet::new();
    for (x, y, path) in terrain::tiles(ar) {
        let header = archive::fnv1a64(&path);
        let mut names = Vec::new();
        for h in ar.pack_files(&path)? {
            if h == header {
                continue;
            }
            let Ok(b) = ar.read_hash(h) else { continue };
            if terrain::parse_heights(&b).is_ok() {
                names = fb::Table::root(&b, 0)?.strings(15)?;
                all.extend(names.iter().cloned());
            }
        }
        let mut used = std::collections::BTreeMap::<u8, usize>::new();
        match terrain::tile_material(ar, &path)
            .and_then(|m| texture::decode_bntx(&ar.read(&m.splat_alpha)?))
        {
            Ok(img) => {
                for p in &img.rgba {
                    if p[2] > 0 {
                        *used.entry(p[0]).or_default() += 1;
                    }
                    if p[3] > 0 {
                        *used.entry(p[1]).or_default() += 1;
                    }
                }
            }
            Err(e) => println!("{x:02}_{y:02} material: {e:#}"),
        }
        let names: Vec<String> = names
            .iter()
            .map(|n| {
                n.strip_prefix("splat")
                    .unwrap_or(n)
                    .chars()
                    .take(3)
                    .collect()
            })
            .collect();
        println!("{x:02}_{y:02} names {names:?} used {used:?}");
    }
    // the global layer index is the position in the sorted set of every tile's names
    for (k, n) in all.iter().enumerate() {
        println!("layer {k:2} = {n}");
    }
    Ok(())
}

/// Agreement of the dominant layer along each shared tile edge, for the control maps as loaded and for each
/// row/column flipped, to check how a control map sits on its tile.
pub fn splat_seams(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::HashMap;
    let mut maps: HashMap<(i32, i32), (f32, texture::Image)> = HashMap::new();
    for (x, y, path) in terrain::tiles(ar) {
        let Ok(h) = terrain::load_heights(ar, &path) else {
            continue;
        };
        if let Ok(img) = terrain::tile_material(ar, &path)
            .and_then(|m| texture::decode_bntx(&ar.read(&m.splat_alpha)?))
        {
            maps.insert((x, y), (h.origin[2], img));
        }
    }
    let dom = |img: &texture::Image, c: usize, r: usize| {
        let p = img.rgba[r * img.width as usize + c];
        if p[2] >= p[3] { p[0] } else { p[1] }
    };
    // (name, flip columns, flip rows)
    for (name, fc, fr) in [
        ("as loaded", false, false),
        ("columns flipped", true, false),
        ("rows flipped", false, true),
    ] {
        let at = |img: &texture::Image, c: usize, r: usize| {
            let n = img.width as usize - 1;
            dom(img, if fc { n - c } else { c }, if fr { n - r } else { r })
        };
        let (mut same, mut total, mut same_z, mut total_z) = (0, 0, 0, 0);
        for (&(x, y), (z, a)) in &maps {
            let n = a.width as usize - 1;
            if let Some((_, b)) = maps.get(&(x + 1, y)) {
                for r in 0..=n {
                    total += 1;
                    same += (at(a, n, r) == at(b, 0, r)) as usize;
                }
            }
            // the tile whose origin is 500 m further along -z: its last row meets this one's first row
            let below = maps
                .iter()
                .find(|(k, (zb, _))| k.0 == x && (*zb - (z - 500.0)).abs() < 1.0);
            if let Some((_, (_, b))) = below {
                for c in 0..=n {
                    total_z += 1;
                    same_z += (at(b, c, n) == at(a, c, 0)) as usize;
                }
            }
        }
        println!(
            "{name}: x edges {:.1}% same ({total}), z edges {:.1}% same ({total_z})",
            100.0 * same as f64 / total.max(1) as f64,
            100.0 * same_z as f64 / total_z.max(1) as f64
        );
    }
    Ok(())
}

/// Heights next to holes, over every tile: for each sample with a hole as a 4-neighbour, the profile walking
/// straight away from the hole (k = 0 is the edge sample). Grouped by the height 8 samples in (coast: < 10 m,
/// inland: above), so a "skirt" (edge samples falling towards 0) shows as a jump between k = 0..2 and the rest.
pub fn holes_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    const K: usize = 9;
    #[derive(Default)]
    struct Group {
        n: usize,
        sum: [f64; K],
        /// edge samples whose height is under a quarter of the height 4 samples in
        drop0: usize,
        drop1: usize,
        /// edge samples under 1 m
        low0: usize,
        /// how many leading samples (k = 0, 1, ...) lie far below the profile further in
        skirt_len: [usize; 4],
    }
    let (mut coast, mut inland) = (Group::default(), Group::default());
    let mut per_tile = Vec::new();
    for (x, y, path) in terrain::tiles(ar) {
        let Ok(t) = terrain::load_heights(ar, &path) else {
            println!("{x:02}_{y:02} FAILED");
            continue;
        };
        let (w, d) = (t.width as isize, t.depth as isize);
        let at = |c: isize, r: isize| -> Option<f32> {
            (c >= 0 && r >= 0 && c < w && r < d).then(|| t.height(c as usize, r as usize))
        };
        let (mut edges, mut skirts) = (0usize, 0usize);
        for r in 0..d {
            for c in 0..w {
                let h0 = t.height(c as usize, r as usize);
                if h0 == 0.0 {
                    continue;
                }
                for (dc, dr) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)] {
                    if at(c + dc, r + dr) != Some(0.0) {
                        continue;
                    }
                    // walk away from the hole
                    let prof: Option<Vec<f32>> = (0..K as isize)
                        .map(|k| at(c - dc * k, r - dr * k).filter(|h| *h != 0.0))
                        .collect();
                    let Some(p) = prof else { continue };
                    edges += 1;
                    let g = if p[K - 1] < 10.0 {
                        &mut coast
                    } else {
                        &mut inland
                    };
                    g.n += 1;
                    for (sum, h) in g.sum.iter_mut().zip(&p) {
                        *sum += *h as f64;
                    }
                    let ref4 = p[4].min(p[8]).max(1e-3);
                    g.drop0 += (p[0] < 0.25 * ref4) as usize;
                    g.drop1 += (p[1] < 0.25 * ref4) as usize;
                    g.low0 += (p[0] < 1.0) as usize;
                    let len = (0..4).take_while(|&k| p[k] < 0.5 * ref4).count();
                    g.skirt_len[len.min(3)] += 1;
                    skirts += (p[0] < 0.25 * ref4 && p[8] >= 10.0) as usize;
                }
            }
        }
        per_tile.push(format!(
            "{x:02}_{y:02} edge samples {edges}, inland skirts {skirts}"
        ));
    }
    for l in &per_tile {
        println!("{l}");
    }
    for (name, g) in [("coast", &coast), ("inland", &inland)] {
        let mean: Vec<String> = g
            .sum
            .iter()
            .map(|s| format!("{:.1}", s / g.n.max(1) as f64))
            .collect();
        println!(
            "{name}: {} edge profiles; mean height k=0..8 {mean:?}; edge < 1/4 of inner {} ({:.1}%), \
             next < 1/4 {}; edge < 1 m {}; leading samples below half: 0:{} 1:{} 2:{} 3+:{}",
            g.n,
            g.drop0,
            100.0 * g.drop0 as f64 / g.n.max(1) as f64,
            g.drop1,
            g.low0,
            g.skirt_len[0],
            g.skirt_len[1],
            g.skirt_len[2],
            g.skirt_len[3],
        );
    }
    Ok(())
}

/// One tile ("08_03"): the `_heightmap.bntx` (R16?) against the height file, and cross-sections across the
/// first few inland hole edges (samples of both, walking from the ground into the hole).
pub fn heightmap_check(ar: &mut archive::Archive, tile: &str) -> anyhow::Result<()> {
    let path = format!("field_graphic/terrain/terrain_{tile}/ter_{tile}.trtrr");
    let t = terrain::load_heights(ar, &path)?;
    let hm_path = format!("field_graphic/terrain/terrain_{tile}/ter_{tile}_heightmap.bntx");
    let b = ar.read(&hm_path)?;
    let (format, w, h, raw) = texture::raw_texels(&b, 2)?;
    println!(
        "heightmap {w}x{h} format {format:#06x}; height file {}x{}",
        t.width, t.depth
    );
    let hm: Vec<f32> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c) as f32 / 65535.0)
        .collect();
    // fit heights = a * hm + b over non-hole samples
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0f64, 0f64, 0f64, 0f64, 0f64);
    let (mut zero_hm_in_hole, mut holes) = (0, 0);
    for r in 0..t.depth.min(h as usize) {
        for c in 0..t.width.min(w as usize) {
            let y = t.height(c, r) as f64;
            let x = hm[r * w as usize + c] as f64;
            if y == 0.0 {
                holes += 1;
                zero_hm_in_hole += (x == 0.0) as usize;
                continue;
            }
            n += 1.0;
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
        }
    }
    let a = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let b0 = (sy - a * sx) / n;
    let mut err = 0f64;
    for r in 0..t.depth.min(h as usize) {
        for c in 0..t.width.min(w as usize) {
            let y = t.height(c, r) as f64;
            if y != 0.0 {
                err = err.max((a * hm[r * w as usize + c] as f64 + b0 - y).abs());
            }
        }
    }
    println!(
        "height = {a:.3} * hm + {b0:.3} (max error {err:.3} m over {n} samples); {holes} holes, {zero_hm_in_hole} of them 0 in the heightmap"
    );
    let mut shown = 0;
    'rows: for r in (0..t.depth).step_by(16) {
        for c in 8..t.width - 8 {
            if t.height(c, r) != 0.0 && t.height(c + 1, r) == 0.0 && t.height(c - 8, r) > 30.0 {
                let line: Vec<String> = (c - 8..(c + 6).min(t.width))
                    .map(|k| {
                        format!(
                            "{:.1}/{:.1}",
                            t.height(k, r),
                            a * hm[r * w as usize + k] as f64 + b0
                        )
                    })
                    .collect();
                println!("row {r} cols {}..: {}", c - 8, line.join(" "));
                shown += 1;
                if shown == 6 {
                    break 'rows;
                }
                continue 'rows;
            }
        }
    }
    Ok(())
}

/// All tiles stitched into one grid (rows from z -5500); holes flood-filled from the grid's border are the sea,
/// the rest are inland hole regions. For each region larger than 100 samples: size, place, and the heights of
/// its edge samples (k = 0) and 2, 4, 8 samples further out.
pub fn hole_regions(ar: &mut archive::Archive) -> anyhow::Result<()> {
    const N: usize = 512 * terrain::TILES as usize + 1;
    let mut g = vec![f32::NAN; N * N]; // NAN = no tile
    for (x, y, path) in terrain::tiles(ar) {
        let Ok(t) = terrain::load_heights(ar, &path) else {
            continue;
        };
        let r0 = ((t.origin[2] + 5500.0) / 500.0).round() as usize * 512;
        let c0 = x as usize * 512;
        for r in 0..t.depth {
            for c in 0..t.width {
                if let Some(v) = g.get_mut((r0 + r) * N + c0 + c) {
                    *v = t.height(c, r);
                }
            }
        }
        let _ = y;
    }
    let hole = |v: f32| v == 0.0 || v.is_nan();
    // label: 0 ground, 1 sea, 2.. inland regions
    let mut label = vec![0u32; N * N];
    let fill = |label: &mut Vec<u32>, start: usize, id: u32| -> Vec<usize> {
        let mut stack = vec![start];
        let mut cells = Vec::new();
        label[start] = id;
        while let Some(i) = stack.pop() {
            cells.push(i);
            let (r, c) = (i / N, i % N);
            let mut push = |j: usize| {
                if label[j] == 0 && hole(g[j]) {
                    label[j] = id;
                    stack.push(j);
                }
            };
            if r > 0 {
                push(i - N);
            }
            if r + 1 < N {
                push(i + N);
            }
            if c > 0 {
                push(i - 1);
            }
            if c + 1 < N {
                push(i + 1);
            }
        }
        cells
    };
    for k in 0..N {
        for i in [k, (N - 1) * N + k, k * N, k * N + N - 1] {
            if label[i] == 0 && hole(g[i]) {
                fill(&mut label, i, 1);
            }
        }
    }
    let mut regions = Vec::new();
    let mut next = 2;
    for i in 0..N * N {
        if label[i] == 0 && hole(g[i]) {
            let cells = fill(&mut label, i, next);
            regions.push((next, cells));
            next += 1;
        }
    }
    // edge profiles per label
    let mut stats: std::collections::HashMap<u32, (usize, [f64; 4], usize)> = Default::default();
    for r in 0..N {
        for c in 0..N {
            let i = r * N + c;
            if hole(g[i]) {
                continue;
            }
            for (dc, dr) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)] {
                let (nr, nc) = (r as isize + dr, c as isize + dc);
                if nr < 0 || nc < 0 || nr >= N as isize || nc >= N as isize {
                    continue;
                }
                let j = nr as usize * N + nc as usize;
                if !hole(g[j]) {
                    continue;
                }
                let at = |k: isize| {
                    let (rr, cc) = (r as isize - dr * k, c as isize - dc * k);
                    (rr >= 0 && cc >= 0 && rr < N as isize && cc < N as isize)
                        .then(|| g[rr as usize * N + cc as usize])
                        .filter(|v| !hole(*v))
                };
                let (Some(a), Some(b), Some(d), Some(e)) = (at(0), at(2), at(4), at(8)) else {
                    continue;
                };
                let s = stats.entry(label[j]).or_default();
                s.0 += 1;
                for (k, v) in [a, b, d, e].iter().enumerate() {
                    s.1[k] += *v as f64;
                }
                s.2 += (a < 1.0) as usize;
            }
        }
    }
    let print = |name: String, cells: usize, s: Option<&(usize, [f64; 4], usize)>| {
        let Some((n, sum, low)) = s else {
            println!("{name}: {cells} samples, no full edge profiles");
            return;
        };
        let m: Vec<String> = sum
            .iter()
            .map(|v| format!("{:.1}", v / *n as f64))
            .collect();
        println!(
            "{name}: {cells} samples; {n} edge profiles, mean k=0/2/4/8 {m:?}, edge < 1 m {low} ({:.0}%)",
            100.0 * *low as f64 / *n as f64
        );
    };
    let sea = label.iter().filter(|l| **l == 1).count();
    print("sea".into(), sea, stats.get(&1));
    regions.sort_by_key(|(_, c)| std::cmp::Reverse(c.len()));
    for (id, cells) in regions.iter().filter(|(_, c)| c.len() > 100) {
        let (sr, sc) = cells
            .iter()
            .fold((0usize, 0usize), |(a, b), i| (a + i / N, b + i % N));
        let (r, c) = (sr / cells.len(), sc / cells.len());
        let z = r as f32 * 500.0 / 512.0 - 5500.0;
        let tx = c / 512;
        let ty = if z < -5000.0 {
            0
        } else {
            ((-z - 500.0) / 500.0).floor() as i32 + 1
        };
        print(
            format!(
                "region {id} around x {:.0} z {z:.0} (tile ~{tx:02}_{ty:02})",
                c as f32 * 500.0 / 512.0
            ),
            cells.len(),
            stats.get(id),
        );
    }
    println!(
        "{} inland regions in all, {} larger than 100 samples",
        regions.len(),
        regions.iter().filter(|(_, c)| c.len() > 100).count()
    );
    Ok(())
}

/// The terrain materials of every tile: how often each switch value and texture role occurs, and both rows of the
/// shared `global_splatST_params` (one line per layer).
pub fn terrain_materials(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let mut switches: BTreeMap<String, usize> = BTreeMap::new();
    let mut floats: BTreeMap<String, usize> = BTreeMap::new();
    let (mut tiles, mut failed) = (0, 0);
    let mut param_path = None;
    for (x, y, path) in terrain::tiles(ar) {
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
        let mtr = format!("{dir}/ter_{x:02}_{y:02}.trmtr");
        let mats = match ar.read(&mtr).and_then(|b| model::parse_materials(&b)) {
            Ok(m) => m,
            Err(e) => {
                failed += 1;
                println!("{x:02}_{y:02} {e:#}");
                continue;
            }
        };
        tiles += 1;
        for (name, m) in &mats {
            for (k, v) in &m.switches {
                *switches.entry(format!("{name}: {k}={v}")).or_default() += 1;
            }
            for (k, v) in &m.floats {
                if !(k.starts_with("RoughnessLayer") || k.contains("Threshold")) || *v != 1.0 {
                    *floats.entry(format!("{name}: {k}={v}")).or_default() += 1;
                }
            }
            if let Some(f) = m.texture("SplatParam") {
                param_path.get_or_insert(model::resolve(&dir, f));
            }
        }
    }
    println!("{tiles} tile materials read, {failed} failed");
    for (k, n) in &switches {
        println!("  {n:4} {k}");
    }
    println!("floats (roughness and thresholds only when not 1):");
    for (k, n) in &floats {
        println!("  {n:4} {k}");
    }
    if let Some(p) = param_path {
        let (w, h, px) = texture::decode_rgba32f(&ar.read(&p)?)?;
        println!("{p}: {w} x {h}");
        for k in 0..64.min(w as usize) {
            let row1 = px.get(w as usize + k).copied().unwrap_or_default();
            println!("  layer {k:2}: row0 {:?} row1 {row1:?}", px[k]);
        }
    }
    Ok(())
}

/// One tile ("06_03"): the height file's grass names (slot 12) and per-cell values (slot 13), how often each
/// value occurs, and for each value the share of cells whose dominant splat layer is each layer.
pub fn grass_cells(ar: &mut archive::Archive, tile: &str) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let path = format!("field_graphic/terrain/terrain_{tile}/ter_{tile}.trtrr");
    let header = archive::fnv1a64(&path);
    let mut found = None;
    for h in ar.pack_files(&path)? {
        if h == header {
            continue;
        }
        let Ok(b) = ar.read_hash(h) else { continue };
        if terrain::parse_heights(&b).is_ok() {
            found = Some(b);
        }
    }
    let b = found.ok_or_else(|| anyhow::anyhow!("no height file"))?;
    let root = fb::Table::root(&b, 0)?;
    println!("slot 12 {:?}", root.strings(12)?);
    println!("slot 15 {:?}", root.strings(15)?);
    println!("slots 10/11 {} x {}", root.u32(10, 0), root.u32(11, 0));
    let (start, n) = root.vector(13)?;
    let raw = fb::get(&b, start, n * 4)?;
    let vals: Vec<u32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let m = terrain::tile_material(ar, &path)?;
    let ctl = texture::decode_bntx(&ar.read(&m.splat_alpha)?)?;
    let mut by: BTreeMap<u32, BTreeMap<u8, usize>> = BTreeMap::new();
    for (i, v) in vals.iter().enumerate() {
        let p = ctl.rgba.get(i).copied().unwrap_or_default();
        let dom = if p[2] >= p[3] { p[0] } else { p[1] };
        *by.entry(*v).or_default().entry(dom).or_default() += 1;
    }
    println!("{n} values; value: cells, then dominant layer: cells");
    for (v, layers) in &by {
        let total: usize = layers.values().sum();
        let mut l: Vec<_> = layers.iter().collect();
        l.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
        println!(
            "  {v:#010x}: {total} cells, layers {:?}",
            &l[..l.len().min(6)]
        );
    }
    Ok(())
}

/// Rebuilds a path from a pack name: `prefix` (with its slashes) + `rest` (collapsed), trying every way to put up
/// to three slashes into `rest`. Prints each split the archive contains.
pub fn split_path(ar: &archive::Archive, prefix: &str, rest: &str) {
    let n = rest.len();
    let cuts: Vec<usize> = (1..n).filter(|&i| rest.is_char_boundary(i)).collect();
    let mut found = 0;
    let mut test = |slashes: &[usize]| {
        let mut p = prefix.trim_end_matches('/').to_string();
        p.push('/');
        let mut last = 0;
        for &c in slashes {
            p.push_str(&rest[last..c]);
            p.push('/');
            last = c;
        }
        p.push_str(&rest[last..]);
        if ar.contains(&p) {
            println!("YES {p}");
            found += 1;
        }
    };
    test(&[]);
    for (i, &a) in cuts.iter().enumerate() {
        test(&[a]);
        for (j, &b) in cuts.iter().enumerate().skip(i + 1) {
            test(&[a, b]);
            for &c in cuts.iter().skip(j + 1) {
                test(&[a, b, c]);
            }
        }
    }
    println!("{found} found");
}

/// What a pack holds: files grouped by kind (magic, or a FlatBuffer's first string field), with counts and sizes,
/// and one example hash per kind.
pub fn pack_survey(ar: &mut archive::Archive, path: &str) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let mut kinds: BTreeMap<String, (usize, usize, u64)> = BTreeMap::new();
    for h in ar.pack_files(path)? {
        let Ok(b) = ar.read_hash(h) else {
            *kinds.entry("unreadable".into()).or_default() = (0, 0, h);
            continue;
        };
        let kind = if b.len() >= 4 && b[..4].iter().all(|c| c.is_ascii_uppercase()) {
            String::from_utf8_lossy(&b[..4]).into_owned()
        } else if b.get(4..8) == Some(b"TAG0") {
            "Havok".into()
        } else if let Ok(t) = Table::root(&b, 0) {
            let s = (0..t.slot_count().min(4))
                .find_map(|i| t.string(i).ok().flatten().map(|s| (i, s.to_string())))
                .filter(|(_, s)| !s.is_empty() && s.len() < 60);
            match s {
                Some((i, s)) if s.chars().all(|c| c.is_ascii_graphic()) => {
                    let short: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
                    format!("fb slot{i} str {short:?}… ({} slots)", t.slot_count())
                }
                _ => format!("fb {} slots", t.slot_count()),
            }
        } else {
            "other".into()
        };
        let e = kinds.entry(kind).or_insert((0, 0, h));
        e.0 += 1;
        e.1 += b.len();
    }
    for (k, (n, bytes, ex)) in &kinds {
        println!("{n:6} {:>11} bytes  {k}  e.g. #{ex:016x}", bytes);
    }
    Ok(())
}

/// Every tile's heights, for ground lookups in world x/z (bilinear; None over a hole or off the map).
struct Ground(Vec<terrain::HeightTile>);

impl Ground {
    fn load(ar: &mut archive::Archive) -> Ground {
        Ground(
            terrain::tiles(ar)
                .iter()
                .filter_map(|(_, _, p)| terrain::load_heights(ar, p).ok())
                .collect(),
        )
    }

    /// The viewer's lookup (`HeightTile::height_at`), over every tile.
    fn at(&self, x: f32, z: f32) -> Option<f32> {
        self.0.iter().find_map(|t| t.height_at(x, z))
    }
}

/// Statistics over a set of instance files: how many parse, instance counts, bounds, scales, and how far each
/// instance's y lies from the ground under it.
#[derive(Default)]
struct InstanceStats {
    files: usize,
    failed: usize,
    instances: usize,
    min: [f32; 3],
    max: [f32; 3],
    scale: (f32, f32),
    /// |y - ground| < 0.5 m, < 2 m, >= 2 m, no ground (hole / off the map)
    ground: [usize; 4],
    /// the last row of every matrix is 0, 0, 0, 1
    affine: usize,
}

impl InstanceStats {
    fn add(&mut self, buf: &[u8], ground: &Ground) -> Option<instances::Instances> {
        if self.files == 0 && self.failed == 0 {
            self.min = [f32::MAX; 3];
            self.max = [f32::MIN; 3];
            self.scale = (f32::MAX, f32::MIN);
        }
        match instances::parse(buf) {
            Ok(ins) => {
                self.files += 1;
                for m in &ins.matrices {
                    self.instances += 1;
                    let p = instances::Instances::position(m);
                    self.min = std::array::from_fn(|k| self.min[k].min(p[k]));
                    self.max = std::array::from_fn(|k| self.max[k].max(p[k]));
                    let s = instances::Instances::scale(m);
                    self.scale.0 = self.scale.0.min(s[1]);
                    self.scale.1 = self.scale.1.max(s[1]);
                    self.affine +=
                        (m[3] == 0.0 && m[7] == 0.0 && m[11] == 0.0 && m[15] == 1.0) as usize;
                    let k = match ground.at(p[0], p[2]) {
                        Some(h) if (p[1] - h).abs() < 0.5 => 0,
                        Some(h) if (p[1] - h).abs() < 2.0 => 1,
                        Some(_) => 2,
                        None => 3,
                    };
                    self.ground[k] += 1;
                }
                Some(ins)
            }
            Err(_) => {
                self.failed += 1;
                None
            }
        }
    }

    fn print(&self, name: &str) {
        println!(
            "{name}: {} files parse, {} fail; {} instances ({} with last row 0,0,0,1); positions {:?} .. {:?}; \
             y scale {:.2}..{:.2}; y vs. ground: <0.5 m {}, <2 m {}, further {}, no ground {}",
            self.files,
            self.failed,
            self.instances,
            self.affine,
            self.min,
            self.max,
            self.scale.0,
            self.scale.1,
            self.ground[0],
            self.ground[1],
            self.ground[2],
            self.ground[3]
        );
    }
}

/// Every file of the pack holding `path`, read as instance buffers, against the terrain.
pub fn instances_check(ar: &mut archive::Archive, path: &str) -> anyhow::Result<()> {
    let ground = Ground::load(ar);
    let mut stats = InstanceStats::default();
    let mut by_tile = std::collections::BTreeMap::<(i32, i32), usize>::new();
    for h in ar.pack_files(path)? {
        let Ok(b) = ar.read_hash(h) else { continue };
        if let Some(ins) = stats.add(&b, &ground) {
            for m in &ins.matrices {
                let p = instances::Instances::position(m);
                *by_tile
                    .entry(((p[0] / 500.0).floor() as i32, (p[2] / 500.0).floor() as i32))
                    .or_default() += 1;
            }
        }
    }
    stats.print(path);
    println!("instances per 500 m cell (x / 500, z / 500): {by_tile:?}");
    // does a shift of the instances by whole tiles fit the ground better?
    let mut all = Vec::new();
    for h in ar.pack_files(path)? {
        if let Some(ins) = ar.read_hash(h).ok().and_then(|b| instances::parse(&b).ok()) {
            all.extend(ins.matrices.iter().map(instances::Instances::position));
        }
    }
    // the _00 row's files say z -5500; would it fit at -500 (next to row 01) instead?
    let moved = Ground(
        ground
            .0
            .iter()
            .map(|t| terrain::HeightTile {
                origin: if t.origin[2] < -5250.0 {
                    [t.origin[0], t.origin[1], t.origin[2] + 5000.0]
                } else {
                    t.origin
                },
                scale: t.scale,
                width: t.width,
                depth: t.depth,
                heights: t.heights.clone(),
            })
            .collect(),
    );
    for (name, g) in [
        ("_00 row at z -5500", &ground),
        ("_00 row at z -500", &moved),
    ] {
        let (mut near, mut lo, mut hi) = (0, 0, 0);
        for p in all.iter().filter(|p| p[2] > -500.0 || p[2] < -5000.0) {
            match g.at(p[0], p[2]) {
                Some(h) if (p[1] - h).abs() < 0.5 => near += 1,
                Some(_) => lo += 1,
                None => hi += 1,
            }
        }
        println!(
            "{name}: instances in the outer rows: {near} within 0.5 m, {lo} further, {hi} no ground"
        );
    }
    for dz in [-1000.0, -500.0, 0.0, 500.0] {
        for dx in [-500.0, 0.0, 500.0] {
            let close = all
                .iter()
                .filter(|p| {
                    ground
                        .at(p[0] + dx, p[2] + dz)
                        .is_some_and(|h| (p[1] - h).abs() < 0.5)
                })
                .count();
            println!(
                "shift x {dx:+} z {dz:+}: {close} of {} within 0.5 m of the ground",
                all.len()
            );
        }
    }
    Ok(())
}

/// Recovers names in the pack holding `path`: `<dir>/ins_root_<stem>_<NNN>_X<xx>_Z<zz>.trins` for every folder in
/// `dirs` (comma-separated), every model stem in the file `stems` (one per line), NNN 000..099 and cells 00..07.
/// Prints how many of the pack's files got a name and the names found per stem.
pub fn guess_ins(
    ar: &mut archive::Archive,
    path: &str,
    stems: &str,
    dirs: &str,
) -> anyhow::Result<()> {
    use std::collections::{BTreeMap, HashSet};
    let files: HashSet<u64> = ar.pack_files(path)?.into_iter().collect();
    let stems: Vec<String> = std::fs::read_to_string(stems)?
        .lines()
        .map(str::trim)
        .filter(|s| s.len() > 3)
        .map(String::from)
        .collect();
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut named = 0;
    for dir in dirs.split(',') {
        for stem in &stems {
            for n in 0..100 {
                for x in 0..8 {
                    for z in 0..8 {
                        let p = format!("{dir}/ins_root_{stem}_{n:03}_X{x:02}_Z{z:02}.trins");
                        if files.contains(&archive::fnv1a64(&p)) {
                            named += 1;
                            found
                                .entry(stem.clone())
                                .or_default()
                                .push(format!("{n:03} X{x:02} Z{z:02} in {dir}"));
                        }
                    }
                }
            }
        }
    }
    println!("{named} of {} files named", files.len());
    for (stem, hits) in &found {
        println!("{stem}: {} {:?}", hits.len(), &hits[..hits.len().min(4)]);
    }
    Ok(())
}

/// Every tile's tree instances (trtrr slot 4): lists, instance files, models (exist? load?), ground fit.
pub fn tile_instances_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    let ground = Ground::load(ar);
    let mut stats = InstanceStats::default();
    let (mut lists, mut list_failed, mut missing_files) = (0, 0, 0);
    let mut models = std::collections::BTreeMap::<String, usize>::new();
    for (x, y, path) in terrain::tiles(ar) {
        match terrain::tile_instances(ar, &path) {
            Ok(list) => {
                lists += 1;
                for (model, files) in list {
                    for f in files {
                        match ar.read(&f) {
                            Ok(b) => {
                                if let Some(ins) = stats.add(&b, &ground) {
                                    *models.entry(model.clone()).or_default() += ins.matrices.len();
                                }
                            }
                            Err(_) => {
                                missing_files += 1;
                                println!("{x:02}_{y:02} missing {f}");
                            }
                        }
                    }
                }
            }
            Err(e) => {
                list_failed += 1;
                println!("{x:02}_{y:02} list: {e:#}");
            }
        }
    }
    println!(
        "{lists} tile lists read, {list_failed} failed, {missing_files} instance files missing"
    );
    stats.print("tile instances");
    let (mut load_ok, mut load_failed) = (0, 0);
    for (m, n) in &models {
        match model::load(ar, m, 0) {
            Ok(md) => {
                load_ok += 1;
                println!(
                    "  {n:6} x {m}: {} triangles, {} LODs?, height {:.1} m, min y {:.2}",
                    md.triangle_count(),
                    md.mesh_files,
                    md.max[1] - md.min[1],
                    md.min[1]
                );
            }
            Err(e) => {
                load_failed += 1;
                println!("  {n:6} x {m}: FAIL {e:#}");
            }
        }
    }
    println!(
        "{} models: {load_ok} load, {load_failed} fail",
        models.len()
    );
    Ok(())
}

/// Every town scene: what it places, whether the models load, how its instance files sit on the ground, and how
/// much of the unity_output pack the scenes name.
pub fn town_scenes_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::{BTreeMap, HashSet};
    let ground = Ground::load(ar);
    let scenes = maps::field_scenes(ar);
    println!(
        "{} scenes: {:?}",
        scenes.len(),
        scenes.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>()
    );
    let pack: HashSet<u64> = ar
        .pack_files(&format!(
            "{}/town_t07_field_streaming_s_/ins_root_plants_un_garden_tulip01_yellow_007_X00_Z00.trins",
            scene::UNITY_OUTPUT
        ))?
        .into_iter()
        .collect();
    let mut named = HashSet::new();
    let mut models: BTreeMap<String, usize> = BTreeMap::new();
    let mut stats = InstanceStats::default();
    let (mut placed, mut placed_ground, mut errors) = (0, [0usize; 4], 0);
    for (code, path) in &scenes {
        let s = match ar.read(path).and_then(|b| scene::parse(&b)) {
            Ok(s) => s,
            Err(e) => {
                println!("{code}: FAIL {e:#}");
                continue;
            }
        };
        let one = format!("{}1.trscn", path.trim_end_matches("0.trscn"));
        let same = ar
            .read(&one)
            .ok()
            .map(|b| b == ar.read(path).unwrap_or_default());
        let c = scene::content(&s);
        errors += c.errors.len();
        for p in &c.placements {
            *models.entry(p.model.clone()).or_default() += 1;
            placed += 1;
            let t = p.matrix.w;
            let k = match ground.at(t.x, t.z) {
                Some(h) if (t.y - h).abs() < 0.5 => 0,
                Some(h) if t.y > h => 1,
                Some(_) => 2,
                None => 3,
            };
            placed_ground[k] += 1;
        }
        for scene::Instancer { model, file: f, .. } in &c.instancers {
            *models.entry(model.clone()).or_default() += 1;
            named.insert(archive::fnv1a64(f));
            match ar.read(f) {
                Ok(b) => {
                    stats.add(&b, &ground);
                }
                Err(_) => println!("{code}: missing {f}"),
            }
        }
        if c.placements.is_empty() && c.instancers.is_empty() && same == Some(true) {
            continue;
        }
        let (mut sum, mut n) = ([0f64; 3], 0);
        for p in &c.placements {
            sum = [
                sum[0] + p.matrix.w.x as f64,
                sum[1] + p.matrix.w.y as f64,
                sum[2] + p.matrix.w.z as f64,
            ];
            n += 1;
        }
        let n = n.max(1) as f64;
        print!(
            "[centre {:.0} {:.0} {:.0}] ",
            sum[0] / n,
            sum[1] / n,
            sum[2] / n
        );
        println!(
            "{code}: {} placements, {} instance files, {} errors, _1 identical: {same:?}",
            c.placements.len(),
            c.instancers.len(),
            c.errors.len()
        );
    }
    println!(
        "{placed} placements: on the ground (0.5 m) {}, above {}, below {}, no ground {}; {errors} node errors",
        placed_ground[0], placed_ground[1], placed_ground[2], placed_ground[3]
    );
    stats.print("instance files the scenes name");
    println!(
        "the scenes name {} of the {} files in the unity_output pack",
        named.intersection(&pack).count(),
        pack.len()
    );
    let (mut ok, mut failed) = (0, 0);
    // shader name: (materials, parts, models)
    let mut shaders: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for m in models.keys() {
        match model::load(ar, m, 0) {
            Ok(md) => {
                ok += 1;
                let mut seen = std::collections::BTreeSet::new();
                for mat in md.materials.values() {
                    shaders.entry(mat.shader.clone()).or_default().0 += 1;
                }
                for sm in &md.submeshes {
                    let sh = md
                        .materials
                        .get(&sm.material)
                        .map(|m| m.shader.clone())
                        .unwrap_or_default();
                    shaders.entry(sh.clone()).or_default().1 += 1;
                    if seen.insert(sh.clone()) {
                        shaders.entry(sh).or_default().2 += 1;
                    }
                }
            }
            Err(e) => {
                failed += 1;
                println!("model FAIL {m}: {e:#}");
            }
        }
    }
    println!("{} distinct models: {ok} load, {failed} fail", models.len());
    println!("shaders (materials, parts, models):");
    for (sh, n) in &shaders {
        println!("  {sh:30} {n:?}");
    }
    let mut by_use: Vec<_> = models.iter().collect();
    by_use.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (m, n) in by_use
        .iter()
        .filter(|(m, _)| !m.contains("/plants_"))
        .take(40)
    {
        println!("  {n:4} {m}");
    }
    Ok(())
}

/// Every scene in every pack that holds one of the known field scenes: name, placed models, instance files,
/// and the ones whose models contain `needle`.
pub fn scene_packs(ar: &mut archive::Archive, needle: &str) -> anyhow::Result<()> {
    use std::collections::BTreeSet;
    let mut packs: BTreeSet<Vec<u64>> = BTreeSet::new();
    for (_, p) in maps::field_scenes(ar) {
        let mut files = ar.pack_files(&p)?;
        files.sort();
        packs.insert(files);
    }
    println!("{} packs hold the known field scenes", packs.len());
    for files in &packs {
        println!("== pack of {} files", files.len());
        for h in files {
            let Ok(b) = ar.read_hash(*h) else { continue };
            let Ok(s) = scene::parse(&b) else { continue };
            if s.nodes.is_empty() {
                continue;
            }
            let c = scene::content(&s);
            let hits: BTreeSet<&str> = c
                .placements
                .iter()
                .map(|p| p.model.as_str())
                .chain(c.instancers.iter().map(|i| i.model.as_str()))
                .filter(|m| !needle.is_empty() && m.contains(needle))
                .collect();
            println!(
                "  #{h:016x} {:?}: {} placements, {} instance files{}",
                s.name,
                c.placements.len(),
                c.instancers.len(),
                if hits.is_empty() {
                    String::new()
                } else {
                    format!(", {needle}: {hits:?}")
                }
            );
        }
    }
    Ok(())
}

/// LOD data: every ModelComponent of every field scene, the values of its float slots 5..18 (how often each),
/// and every placed model's mesh-file count and `.trmdl` slot 4 (LOD table?) with slot 4's type string.
pub fn lod_survey(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let mut slots: BTreeMap<(usize, String), usize> = BTreeMap::new();
    let mut models = std::collections::BTreeSet::new();
    let mut components = 0;
    for (_, path) in maps::field_scenes(ar) {
        let Ok(s) = ar.read(&path).and_then(|b| scene::parse(&b)) else {
            continue;
        };
        for n in &s.nodes {
            n.walk(&mut Vec::new(), &mut |n, _| {
                if n.kind != "trinity_ModelComponent" {
                    return;
                }
                let Some(t) = n.table() else { return };
                components += 1;
                if let Ok(Some(p)) = t.string(0) {
                    models.insert(p);
                }
                for k in 5..19 {
                    if t.field(k).is_some() {
                        let raw = t.u32(k, 0);
                        let f = f32::from_bits(raw);
                        let v = if f.abs() > 1e-3 && f.abs() < 1e6 {
                            format!("{f}")
                        } else {
                            format!("u{raw}")
                        };
                        *slots.entry((k, v)).or_default() += 1;
                    }
                }
            });
        }
    }
    println!("{components} ModelComponents, {} models", models.len());
    for ((k, v), n) in &slots {
        println!("  slot {k:2} = {v:>10}: {n}");
    }
    let mut files: BTreeMap<usize, usize> = BTreeMap::new();
    let mut lod_types: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    for m in &models {
        let Ok(b) = ar.read(m) else { continue };
        let Ok(t) = Table::root(&b, 0) else { continue };
        *files
            .entry(t.tables(1).map(|v| v.len()).unwrap_or(0))
            .or_default() += 1;
        if let Ok(v) = t.tables(4) {
            for e in v {
                let ty = e.string(1).ok().flatten().unwrap_or_default();
                *lod_types.entry(ty).or_default() += 1;
                if shown < 3 {
                    shown += 1;
                    println!("{m} slot 4:");
                    dump(&e, 2, 3);
                }
            }
        }
    }
    println!("mesh files per model: {files:?}");
    println!("trmdl slot 4 types: {lod_types:?}");
    Ok(())
}

/// A scene as a tree: objects with their transforms, models and instance files (other kinds by name only).
pub fn scene_tree(ar: &mut archive::Archive, path: &str, limit: usize) -> anyhow::Result<()> {
    let buf = match path.strip_prefix('#') {
        Some(h) => ar.read_hash(u64::from_str_radix(h, 16)?)?,
        None => ar.read(path)?,
    };
    let s = scene::parse(&buf)?;
    let mut shown = 0;
    for n in &s.nodes {
        n.walk(&mut Vec::new(), &mut |n, parents| {
            if shown >= limit {
                return;
            }
            shown += 1;
            let pad = "  ".repeat(parents.len());
            let kind = n.kind.trim_start_matches("trinity_");
            match (n.object(), n.reference()) {
                (Ok(Some((name, t))), _) => println!(
                    "{pad}{kind} {name:?} T {:?} R {:?} S {:?}",
                    t.translation, t.rotation, t.scale
                ),
                (Err(e), _) => println!("{pad}{kind} FAIL {e:#}"),
                (_, Some((what, p))) => println!("{pad}{kind} {what} {p}"),
                _ => println!("{pad}{kind}"),
            }
        });
    }
    Ok(())
}

/// A scene: how many nodes of each kind, then the first `limit` nodes as a tree with their data's slots
/// (only kinds matching `filter`, if given).
pub fn scene_dump(
    ar: &mut archive::Archive,
    path: &str,
    limit: usize,
    filter: Option<&str>,
) -> anyhow::Result<()> {
    let buf = match path.strip_prefix('#') {
        Some(h) => ar.read_hash(u64::from_str_radix(h, 16)?)?,
        None => ar.read(path)?,
    };
    let s = scene::parse(&buf)?;
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for n in &s.nodes {
        n.walk(&mut Vec::new(), &mut |n, _| {
            *kinds.entry(n.kind.clone()).or_default() += 1
        });
    }
    println!(
        "scene {:?}: {} top nodes; kinds {kinds:?}",
        s.name,
        s.nodes.len()
    );
    let mut shown = 0;
    for n in &s.nodes {
        n.walk(&mut Vec::new(), &mut |n, parents| {
            if shown >= limit || filter.is_some_and(|f| !n.kind.contains(f)) {
                return;
            }
            shown += 1;
            let pad = "  ".repeat(parents.len());
            println!("{pad}== {} ({} bytes)", n.kind, n.data.len());
            if let Some(t) = n.table() {
                dump(&t, 2, 3);
            }
            if filter.is_some() {
                for (k, row) in n.data.chunks(32).enumerate() {
                    let words: Vec<String> = row
                        .chunks(4)
                        .map(|w| {
                            let mut b = [0u8; 4];
                            b[..w.len()].copy_from_slice(w);
                            let f = f32::from_le_bytes(b);
                            if f.abs() > 1e-4 && f.abs() < 1e6 {
                                format!("{f:>10.4}")
                            } else {
                                format!("{:>10x}", u32::from_le_bytes(b))
                            }
                        })
                        .collect();
                    println!("{pad}  {:4x}: {}", k * 32, words.join(" "));
                }
            }
        });
    }
    Ok(())
}

pub fn has(ar: &archive::Archive, paths: &[String]) {
    for p in paths {
        println!("{} {p}", if ar.contains(p) { "YES" } else { "no " });
    }
}

pub fn quat_test(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use three_d::{InnerSpace, Quat, Rotation3};
    let models = ar.pokemon_models();
    let variants = [
        anim::QuatCodec {
            quarter_pi: false,
            sign_bit: true,
        },
        anim::QuatCodec {
            quarter_pi: false,
            sign_bit: false,
        },
        anim::QuatCodec {
            quarter_pi: true,
            sign_bit: true,
        },
        anim::QuatCodec {
            quarter_pi: true,
            sign_bit: false,
        },
    ];
    let mut score = [(0.0f64, 0usize, 0usize); 4];
    let (mut tdist, mut tn) = (0.0f64, 0usize);
    let mut checked = 0;
    for m in models.iter().step_by(25) {
        let Ok(md) = model::load(ar, m, 0) else {
            continue;
        };
        let Ok(sk) = skeleton::load(ar, &md.dir, md.skeleton_file.as_deref().unwrap_or("")) else {
            continue;
        };
        let list = anim::list(ar, m);
        let Some((_, path)) = list
            .iter()
            .find(|(l, _)| l.contains("wait"))
            .or(list.first())
        else {
            continue;
        };
        let Ok(buf) = ar.read(path) else { continue };
        checked += 1;
        for (vi, v) in variants.iter().enumerate() {
            let a = anim::parse(&buf, "", *v)?;
            for t in &a.tracks {
                let Some(b) = sk.bones.iter().find(|b| b.name == t.name) else {
                    continue;
                };
                let s = t.sample(0.0);
                if let Some(q) = s.rotation {
                    let r = Quat::from_angle_z(three_d::radians(b.rotation[2]))
                        * Quat::from_angle_y(three_d::radians(b.rotation[1]))
                        * Quat::from_angle_x(three_d::radians(b.rotation[0]));
                    let d = q.dot(r).abs() as f64;
                    score[vi].0 += d;
                    score[vi].1 += 1;
                    if d > 0.999 {
                        score[vi].2 += 1;
                    }
                }
                if vi == 0
                    && let Some(tr) = s.translation
                {
                    let rest =
                        three_d::Vec3::new(b.translation[0], b.translation[1], b.translation[2]);
                    tdist += (tr - rest).magnitude() as f64;
                    tn += 1;
                }
            }
        }
    }
    println!("models checked {checked}");
    for (v, s) in variants.iter().zip(score) {
        println!(
            "{v:?}: mean |dot| {:.4}, {} of {} within 0.999",
            s.0 / s.1.max(1) as f64,
            s.2,
            s.1
        );
    }
    println!(
        "translation frame 0 vs rest: mean distance {:.4} over {tn}",
        tdist / tn.max(1) as f64
    );
    Ok(())
}

/// Brute-force the packed-quaternion layout on fixed rotation tracks (bones that do not move should sit at rest).
pub fn quat_layouts(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use three_d::{InnerSpace, Quat, Rotation3};
    // (raw x, y, z, rest quaternion)
    let mut samples: Vec<([u16; 3], Quat)> = Vec::new();
    for m in ar.pokemon_models().iter().step_by(20) {
        let Ok(md) = model::load(ar, m, 0) else {
            continue;
        };
        let Ok(sk) = skeleton::load(ar, &md.dir, md.skeleton_file.as_deref().unwrap_or("")) else {
            continue;
        };
        let list = anim::list(ar, m);
        let Some((_, path)) = list
            .iter()
            .find(|(l, _)| l.contains("wait"))
            .or(list.first())
        else {
            continue;
        };
        let Ok(buf) = ar.read(path) else { continue };
        let root = fb::Table::root(&buf, 0)?;
        let Some(bones) = root.table(1)? else {
            continue;
        };
        for t in bones.tables(0)? {
            if t.u8(3, 0) != 1 {
                continue;
            }
            let (Some(name), Some(v)) = (t.string(0)?, t.table(4)?) else {
                continue;
            };
            let Some(at) = v.field(0) else { continue };
            let raw = [
                fb::u16_at(&buf, at)?,
                fb::u16_at(&buf, at + 2)?,
                fb::u16_at(&buf, at + 4)?,
            ];
            let Some(b) = sk.bones.iter().find(|b| b.name == name) else {
                continue;
            };
            let r = Quat::from_angle_z(three_d::radians(b.rotation[2]))
                * Quat::from_angle_y(three_d::radians(b.rotation[1]))
                * Quat::from_angle_x(three_d::radians(b.rotation[0]));
            samples.push((raw, r));
        }
    }
    println!("fixed rotation samples: {}", samples.len());
    let identity = samples.iter().filter(|(_, r)| r.s.abs() > 0.9999).count();
    println!("  of which rest rotation is identity: {identity}");
    for (k, (raw, r)) in samples
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| r.s.abs() < 0.99)
        .take(4)
    {
        println!(
            "  sample {k}: raw {:04x} {:04x} {:04x}  rest (w {:.4}, x {:.4}, y {:.4}, z {:.4})",
            raw[0], raw[1], raw[2], r.s, r.v.x, r.v.y, r.v.z
        );
    }
    let mut results = Vec::new();
    for layout in 0..2 {
        for scale in 0..3 {
            for order in 0..2 {
                for sign in 0..2 {
                    let mut good = 0;
                    for (raw, r) in &samples {
                        let pack =
                            ((raw[2] as u64) << 32) | ((raw[1] as u64) << 16) | raw[0] as u64;
                        let (idx, neg, c) = if layout == 0 {
                            (
                                (pack & 3) as usize,
                                pack & 4 != 0,
                                [
                                    (pack >> 3) & 0x7fff,
                                    (pack >> 18) & 0x7fff,
                                    (pack >> 33) & 0x7fff,
                                ],
                            )
                        } else {
                            (
                                ((pack >> 45) & 3) as usize,
                                pack & (1 << 47) != 0,
                                [pack & 0x7fff, (pack >> 15) & 0x7fff, (pack >> 30) & 0x7fff],
                            )
                        };
                        let e = |i: u64| {
                            let t = i as f32 / 32767.0;
                            match scale {
                                0 => (t - 0.5) * std::f32::consts::SQRT_2,
                                1 => t * std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_4,
                                _ => (t * std::f32::consts::FRAC_PI_2
                                    - std::f32::consts::FRAC_PI_4)
                                    .sin(),
                            }
                        };
                        let q = [e(c[0]), e(c[1]), e(c[2])];
                        let mut big = (1.0 - q.iter().map(|v| v * v).sum::<f32>()).max(0.0).sqrt();
                        if sign == 1 && neg {
                            big = -big;
                        }
                        let mut v = [0.0f32; 4];
                        let mut kk = 0;
                        for (i, slot) in v.iter_mut().enumerate() {
                            if i == idx {
                                *slot = big
                            } else {
                                *slot = q[kk];
                                kk += 1
                            }
                        }
                        let quat = if order == 0 {
                            Quat::new(v[3], v[0], v[1], v[2])
                        } else {
                            Quat::new(v[0], v[1], v[2], v[3])
                        };
                        if quat.normalize().dot(*r).abs() > 0.9995 {
                            good += 1;
                        }
                    }
                    results.push((
                        good,
                        format!(
                            "layout {layout} scale {scale} order {} sign {sign}",
                            if order == 0 { "xyzw" } else { "wxyz" }
                        ),
                    ));
                }
            }
        }
    }
    results.sort_by_key(|r| std::cmp::Reverse(r.0));
    for (g, d) in results.iter().take(6) {
        println!("{g:5} / {}  {d}", samples.len());
    }
    Ok(())
}

pub fn skin_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    let (mut models, mut no_skin, mut bad_sum, mut verts, mut over) = (0, 0, 0usize, 0usize, 0);
    let mut worst_ratio = 0.0f32;
    for m in ar.pokemon_models() {
        let Ok(md) = model::load(ar, &m, 0) else {
            continue;
        };
        let Ok(sk) = skeleton::load(ar, &md.dir, md.skeleton_file.as_deref().unwrap_or("")) else {
            continue;
        };
        models += 1;
        let jc = sk.inverse_bind.len();
        let mut max_idx = 0usize;
        for s in &md.submeshes {
            let (Some(j), Some(w)) = (&s.joints, &s.weights) else {
                no_skin += 1;
                continue;
            };
            for (jj, ww) in j.iter().zip(w) {
                verts += 1;
                let sum: f32 = ww.iter().sum();
                if (sum - 1.0).abs() > 0.02 {
                    bad_sum += 1;
                }
                for k in 0..4 {
                    if ww[k] > 0.0 {
                        max_idx = max_idx.max(jj[k] as usize);
                    }
                }
            }
        }
        if max_idx >= jc {
            over += 1;
            if over <= 3 {
                println!("{m}: index {max_idx} >= {jc} joints");
            }
        }
        worst_ratio = worst_ratio.max(max_idx as f32 / jc.max(1) as f32);
    }
    println!(
        "models {models}; parts without skin data {no_skin}; vertices {verts}, weight sum off by >0.02: {bad_sum}"
    );
    println!(
        "models using a joint index beyond the joint list: {over}; highest index / joint count: {worst_ratio:.2}"
    );
    Ok(())
}

pub fn anim_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let (mut files, mut ok, mut unmatched, mut tracks) = (0, 0, 0usize, 0usize);
    let mut errs: BTreeMap<String, usize> = BTreeMap::new();
    let mut no_list = 0;
    for m in ar.pokemon_models().iter().step_by(10) {
        let list = anim::list(ar, m);
        if list.is_empty() {
            no_list += 1;
            continue;
        }
        let md = model::load(ar, m, 0)?;
        let sk = skeleton::load(ar, &md.dir, md.skeleton_file.as_deref().unwrap_or(""))?;
        for (label, path) in &list {
            files += 1;
            match anim::load(ar, path, label) {
                Ok(a) => {
                    ok += 1;
                    for t in &a.tracks {
                        tracks += 1;
                        if !sk.bones.iter().any(|b| b.name == t.name) {
                            unmatched += 1;
                        }
                    }
                }
                Err(e) => {
                    let k = format!("{e:#}");
                    if !errs.contains_key(&k) {
                        println!("{path}: {k}");
                    }
                    *errs.entry(k).or_default() += 1;
                }
            }
        }
    }
    println!("animation files {files}, parsed {ok}; models without a list {no_list}");
    println!("tracks {tracks}, naming no bone of the skeleton: {unmatched}");
    println!("errors: {errs:?}");
    Ok(())
}

pub fn anim_list(ar: &mut archive::Archive, m: &str) {
    let l = anim::list(ar, m);
    println!("{} found", l.len());
    for (a, p) in l.iter().take(5) {
        println!("  {a}  {p}");
    }
    // every .tranm string in the tracr, found or not
    let dir = &m[..m.rfind('/').unwrap() + 1];
    let stem = m.rsplit('/').next().unwrap().trim_end_matches(".trmdl");
    let buf = ar
        .read(&format!("{dir}{stem}_base.tracr"))
        .unwrap_or_default();
    let text = String::from_utf8_lossy(&buf);
    let names: Vec<&str> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '/'))
        .filter(|s| s.ends_with(".tranm"))
        .collect();
    let found = names
        .iter()
        .filter(|n| ar.contains(&model::resolve(dir, n)))
        .count();
    println!(
        "tranm strings in tracr: {}, in archive: {found}",
        names.len()
    );
    for n in names
        .iter()
        .filter(|n| !ar.contains(&model::resolve(dir, n)))
        .take(5)
    {
        println!("  missing {n}");
    }
}

/// For every human model (chara/model_tr, model_uq, model_vr): its own clip list, extra clips, the body type its
/// .trmdt names, and how many tracks of the idle clip it would play land on its bones. For models without their own
/// list, the matched tracks of every body type's idle clip.
fn trainer_anims(ar: &mut archive::Archive) -> anyhow::Result<()> {
    let folders = motion::folders(ar);
    let bodies = motion::body_types(&folders);
    println!(
        "{} motion folders, {} body types: {bodies:?}",
        folders.len(),
        bodies.len()
    );
    let models: Vec<String> = ar
        .character_models()
        .into_iter()
        .filter(|p| !p.starts_with("chara/model_ob/"))
        .collect();
    let (mut own, mut named, mut extra, mut no_skel, mut bad, mut vr_bad) = (0, 0, 0, 0, 0, 0);
    for path in &models {
        let skel = model::load(ar, path, 0).and_then(|m| {
            let f = m
                .skeleton_file
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no skeleton"))?;
            skeleton::load(ar, &m.dir, &f)
        });
        let skel = match skel {
            Ok(s) => s,
            Err(e) if path.starts_with("chara/model_vr/") => {
                // NPC parts: put the part on the shared base skeleton and count vertices left without a bone
                match player::assemble_on(ar, motion::VR_BASE_SKELETON, "vr", &[path.as_str()], 0) {
                    Ok(a) => {
                        let (mut bad, mut total) = (0, 0);
                        for s in &a.model.submeshes {
                            let (Some(j), Some(w)) = (&s.joints, &s.weights) else {
                                continue;
                            };
                            for (j, w) in j.iter().zip(w) {
                                total += 1;
                                bad += (0..4).any(|k| {
                                    w[k] > 0.0 && a.skeleton.joint_bone(j[k] as usize).is_none()
                                }) as usize;
                            }
                        }
                        println!(
                            "{path}: ({e:#}) on the base skeleton: {} bones, {bad} of {total} vertices off a bone, warnings {:?}",
                            a.skeleton.bones.len(),
                            a.warnings
                        );
                        vr_bad += (bad > 0 || !a.warnings.is_empty()) as usize;
                        a.skeleton
                    }
                    Err(e2) => {
                        no_skel += 1;
                        println!("{path}: {e:#}; on the base skeleton: {e2:#}");
                        continue;
                    }
                }
            }
            Err(e) => {
                no_skel += 1;
                println!("{path}: {e:#}");
                continue;
            }
        };
        let found = motion::find(ar, &folders, path);
        own += found.own.is_some() as usize;
        named += found.named_body.is_some() as usize;
        extra += !found.extra.is_empty() as usize;
        let clips = motion::clips(ar, &found, None);
        let idle = clips
            .iter()
            .find(|(l, _)| l.ends_with("defaultwait01_loop"))
            .and_then(|(l, p)| anim::load(ar, p, l).ok())
            .map(|c| {
                let m = c
                    .tracks
                    .iter()
                    .filter(|t| skel.bones.iter().any(|b| b.name == t.name))
                    .count();
                format!("{m}/{}", c.tracks.len())
            });
        // every clip must load
        let failed = clips
            .iter()
            .filter(|(l, p)| anim::load(ar, p, l).is_err())
            .count();
        bad += failed;
        println!(
            "{path}: {} bones; own {:?}, {} clips ({failed} fail), extra {:?}; named {:?}; idle on bones {idle:?}",
            skel.bones.len(),
            found
                .own
                .as_deref()
                .map(|p| p.rsplit('/').next().unwrap_or(p)),
            clips.len(),
            found
                .extra
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>(),
            found.named_body,
        );
        if found.own.is_none() {
            let fits: Vec<String> = bodies
                .iter()
                .filter_map(|b| {
                    body_fit(ar, &skel, b)
                        .map(|f| format!("{}:{}/{} {:.4}", &b[7..], f.matched, f.tracks, f.error))
                })
                .collect();
            println!("    fits {}", fits.join("  "));
        }
    }
    println!(
        "{} models: {own} own list, {extra} with extra clips, {named} name a body type, {no_skel} without a skeleton, {vr_bad} vr parts off the base skeleton, {bad} clips failed to load",
        models.len()
    );
    Ok(())
}

/// How well a body type's idle clip fits a skeleton.
#[derive(Clone, Copy, Debug)]
struct Fit {
    /// Tracks of the clip that name a bone of the skeleton, and all its tracks.
    matched: usize,
    tracks: usize,
    /// Mean distance (metres) between the clip's first-frame bone offsets and the skeleton's rest offsets, over the
    /// matched tracks that move bones.
    error: f32,
}

fn body_fit(ar: &mut archive::Archive, skel: &skeleton::Skeleton, body: &str) -> Option<Fit> {
    let path = format!(
        "chara/motion_cc_base/{body}/{body}_{}.tranm",
        motion::IDLE_CLIP
    );
    let clip = anim::load(ar, &path, body).ok()?;
    let (mut matched, mut sum, mut n) = (0usize, 0.0f32, 0usize);
    for t in &clip.tracks {
        let Some(b) = skel.bones.iter().find(|b| b.name == t.name) else {
            continue;
        };
        matched += 1;
        if let Some(v) = t.sample(0.0).translation {
            let r = b.translation;
            sum += ((v.x - r[0]).powi(2) + (v.y - r[1]).powi(2) + (v.z - r[2]).powi(2)).sqrt();
            n += 1;
        }
    }
    Some(Fit {
        matched,
        tracks: clip.tracks.len(),
        error: if n > 0 { sum / n as f32 } else { f32::INFINITY },
    })
}

/// (shader, render type, alpha test) -> (count, example names, discard values)
type MaterialGroups =
    std::collections::BTreeMap<(String, String, String), (usize, Vec<String>, Vec<String>)>;

/// Every material of every model and player part, grouped by shader, render type (trmtr slot 15) and the shader's
/// EnableAlphaTest switch, with counts, example names, and DiscardValue values seen.
fn transparency(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let mut paths = ar.pokemon_models();
    paths.extend(ar.character_models());
    paths.extend(ar.player_parts());
    // TRANSP_FILTER=chara/model_ narrows the survey to paths containing it
    if let Ok(f) = std::env::var("TRANSP_FILTER") {
        paths.retain(|p| p.contains(&f));
    }
    // (shader, render type, alpha test) -> (count, example names, discard values)
    let mut groups: MaterialGroups = BTreeMap::new();
    let mut switch_names: BTreeMap<String, usize> = BTreeMap::new();
    // materials without a LayerMaskMap whose BaseColor is not white: (shader, has BaseColorMap,
    // EnableBaseColorMap) -> (count, examples)
    let mut tinted: BTreeMap<(String, bool, String), (usize, Vec<String>)> = BTreeMap::new();
    for p in &paths {
        let Ok(m) = model::load(ar, p, 0) else {
            continue;
        };
        let kind = p.split('/').nth(1).unwrap_or("");
        for (name, mat) in &m.materials {
            for k in mat.switches.keys() {
                *switch_names.entry(k.clone()).or_default() += 1;
            }
            if mat.texture("LayerMaskMap").is_none()
                && let Some(c) = mat.params.get("BaseColor")
                && c[..3].iter().any(|v| (v - 1.0).abs() > 0.01)
            {
                let key = (
                    mat.shader.clone(),
                    mat.texture("BaseColorMap").is_some(),
                    mat.switches
                        .get("EnableBaseColorMap")
                        .cloned()
                        .unwrap_or("-".into()),
                );
                let t = tinted.entry(key).or_default();
                t.0 += 1;
                if t.1.len() < 10 {
                    t.1.push(format!(
                        "{}:{name} {:.2?}",
                        p.split('/').nth(1).unwrap_or(""),
                        &c[..3]
                    ));
                }
            }
            let key = (
                mat.shader.clone(),
                mat.render_type.clone(),
                mat.switches
                    .get("EnableAlphaTest")
                    .cloned()
                    .unwrap_or("-".into()),
            );
            let g = groups.entry(key).or_default();
            g.0 += 1;
            let ex = format!("{kind}:{name}");
            if g.1.len() < 40 && !g.1.contains(&ex) {
                g.1.push(ex);
            }
            if let Some(d) = mat.floats.get("DiscardValue") {
                let d = format!("{d}");
                if g.2.len() < 6 && !g.2.contains(&d) {
                    g.2.push(d);
                }
            }
        }
    }
    for ((shader, rt, at), (n, ex, disc)) in &groups {
        println!("{shader:>14} {rt:>12} alphatest={at:<5} {n:>6}  discard {disc:?}  e.g. {ex:?}");
    }
    println!("switches: {switch_names:?}");
    println!(
        "BaseColor not white, no LayerMaskMap: (shader, has BaseColorMap, EnableBaseColorMap)"
    );
    for ((sh, has, en), (n, ex)) in &tinted {
        println!("{sh:>14} map={has:<5} enable={en:<5} {n:>5}  e.g. {ex:?}");
    }
    Ok(())
}

/// Every Pokémon eye material ("Eye" and "EyeClearCoat" shaders): how the highlight is set up.
fn eyes(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut total = 0;
    for p in ar.pokemon_models() {
        let Ok(m) = model::load(ar, &p, 0) else {
            continue;
        };
        for mat in m.materials.values().filter(|m| m.shader.starts_with("Eye")) {
            total += 1;
            let key = format!(
                "{} highlight={} normal1={} map1={}",
                mat.shader,
                mat.switches
                    .get("EnableHighlight")
                    .map_or("-", String::as_str),
                mat.texture("NormalMap1").is_some(),
                mat.switches
                    .get("EnableNormalMap1")
                    .map_or("-", String::as_str),
            );
            *keys.entry(key).or_default() += 1;
            let mut note = |k: &str, v: String| {
                *values
                    .entry(k.to_string())
                    .or_default()
                    .entry(v)
                    .or_default() += 1;
            };
            for f in [
                "RoughnessHighlight",
                "MetallicHighlight",
                "EmissionIntensityLayer5",
                "RoughnessClearCoat",
            ] {
                note(f, mat.floats.get(f).map_or("-".into(), |v| format!("{v}")));
            }
            for c in ["EmissionColorLayer5", "BaseColorClearCoat"] {
                note(
                    c,
                    mat.params
                        .get(c)
                        .map_or("-".into(), |v| format!("{:.2?}", v)),
                );
            }
            let mut sw: Vec<_> = mat.switches.keys().cloned().collect();
            sw.sort();
            note("switch set", sw.join(","));
        }
    }
    println!("{total} eye materials");
    for (k, n) in keys {
        println!("{n:>6}  {k}");
    }
    for (k, vs) in values {
        let mut v: Vec<_> = vs.into_iter().collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.1));
        v.truncate(8);
        println!("{k}: {v:?}");
    }
    Ok(())
}

/// Paldea's resident scenes' models (`maps::overworlds`): per part, its material, size, vertex colours, and how its
/// surface lies against the terrain under it (water y - ground y, over the vertices that have ground).
pub fn water_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    use three_d::Transform;
    let ground = Ground::load(ar);
    println!("{} tiles of ground", ground.0.len());
    for label in maps::overworlds().remove(0).resident {
        let c = match scene::load_content(ar, &label) {
            Ok(c) => c,
            Err(e) => {
                println!("== {label}: FAILED {e:#}");
                continue;
            }
        };
        println!(
            "== {label}: {} placements, {} instance files",
            c.placements.len(),
            c.instancers.len()
        );
        for p in &c.placements {
            let m = match model::load(ar, &p.model, 0) {
                Ok(m) => m,
                Err(e) => {
                    println!("  {}: FAILED {e:#}", p.model);
                    continue;
                }
            };
            println!("  {} ({} parts)", p.model, m.submeshes.len());
            for s in &m.submeshes {
                let shader = m
                    .materials
                    .get(&s.material)
                    .map_or("?", |x| x.shader.as_str());
                let mut used = vec![false; s.positions.len()];
                for i in &s.indices {
                    used[*i as usize] = true;
                }
                let (mut ylo, mut yhi) = (f32::MAX, f32::MIN);
                let mut dys = Vec::new();
                let mut holes = 0;
                let mut col = [0.0f64; 4];
                let mut amin = 1.0f32;
                let mut n = 0;
                for (v, q) in s.positions.iter().enumerate() {
                    if !used[v] {
                        continue;
                    }
                    n += 1;
                    let w = p
                        .matrix
                        .transform_point(three_d::Point3::new(q[0], q[1], q[2]));
                    ylo = ylo.min(w.y);
                    yhi = yhi.max(w.y);
                    match ground.at(w.x, w.z) {
                        Some(g) => dys.push(w.y - g),
                        None => holes += 1,
                    }
                    if let Some(cs) = &s.colors {
                        for k in 0..4 {
                            col[k] += cs[v][k] as f64;
                        }
                        amin = amin.min(cs[v][3]);
                    }
                }
                dys.sort_by(f32::total_cmp);
                let q = |f: f32| {
                    dys.get(((dys.len() as f32 - 1.0) * f) as usize)
                        .copied()
                        .unwrap_or(f32::NAN)
                };
                let above = dys.iter().filter(|d| **d > -0.25).count();
                let nn = n.max(1) as f64;
                println!(
                    "    {:40} {:38} {shader:8} {:5} tris  y {ylo:8.2}..{yhi:8.2}  ground {}/{n} (holes {holes}):                      dy p10 {:7.2} p50 {:7.2} p90 {:7.2}, {:.0}% above  colour {}",
                    s.shape,
                    s.material,
                    s.indices.len() / 3,
                    dys.len(),
                    q(0.1),
                    q(0.5),
                    q(0.9),
                    100.0 * above as f32 / dys.len().max(1) as f32,
                    if s.colors.is_some() {
                        format!(
                            "mean {:.2} {:.2} {:.2} {:.2}, min a {amin:.2}",
                            col[0] / nn,
                            col[1] / nn,
                            col[2] / nn,
                            col[3] / nn
                        )
                    } else {
                        "none".into()
                    }
                );
            }
        }
    }
    Ok(())
}

/// Every `terrain_XX_YY` tile of any grid (XX, YY up to 99): origin, sample grid, height range, holes.
pub fn map_tiles(ar: &mut archive::Archive) -> anyhow::Result<()> {
    for x in 0..100 {
        for y in 0..100 {
            let path = terrain::tile_path(x, y);
            if !ar.contains(&path) {
                continue;
            }
            match terrain::load_heights(ar, &path) {
                Ok(t) => {
                    let solid: Vec<f32> = t.heights.iter().copied().filter(|v| *v != 0.0).collect();
                    let (lo, hi) = solid
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
                    println!(
                        "{x:02}_{y:02}: origin {:?} cell {:?} {}x{} heights {lo:.1}..{hi:.1} holes {}",
                        t.origin,
                        t.scale,
                        t.width,
                        t.depth,
                        t.heights.len() - solid.len()
                    );
                }
                Err(e) => println!("{x:02}_{y:02}: FAILED {e:#}"),
            }
        }
    }
    // each map's grid formulas against the files: origin, tile under the tile's centre
    for map in maps::overworlds() {
        let Some(g) = map.grid else { continue };
        let (mut ok, mut bad) = (0, 0);
        for (x, y, path) in g.tiles(ar) {
            let Ok(t) = terrain::load_heights(ar, &path) else {
                continue;
            };
            let (ox, oz) = g.origin(x, y);
            let fits = (ox - t.origin[0]).abs() < 0.01
                && (oz - t.origin[2]).abs() < 0.01
                && g.tile_at(ox + 250.0, oz + 250.0) == Some((x, y));
            if fits {
                ok += 1;
            } else {
                bad += 1;
                println!(
                    "{}: {x:02}_{y:02} grid says ({ox}, {oz}), file {:?}",
                    map.name, t.origin
                );
            }
        }
        println!("{}: {ok} tiles fit the grid, {bad} do not", map.name);
    }
    Ok(())
}

/// Every room `maps::rooms` finds: its content with sub-scenes, the models placed and whether they load.
pub fn rooms_check(ar: &mut archive::Archive) -> anyhow::Result<()> {
    let rooms = maps::rooms(ar);
    let (mut empty, mut failed) = (0, 0);
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for (code, path) in &rooms {
        match scene::load_content(ar, path) {
            Ok(c) => {
                let bad = c
                    .placements
                    .iter()
                    .filter(|p| model::load(ar, &p.model, 0).is_err())
                    .count();
                failed += bad + c.errors.len();
                if c.placements.is_empty() && c.instancers.is_empty() {
                    empty += 1;
                }
                println!(
                    "{code:40} {} models ({bad} fail), {} instance files, {} errors  {path}",
                    c.placements.len(),
                    c.instancers.len(),
                    c.errors.len()
                );
                for e in &c.errors {
                    // "sub-scene <path>: <error>": by the referenced file's extension and the error's last words
                    let ext = e
                        .split(':')
                        .next()
                        .and_then(|p| p.rsplit('.').next())
                        .unwrap_or("?");
                    let why = e.rsplit(':').next().unwrap_or("").trim();
                    *kinds.entry(format!(".{ext}: {why}")).or_default() += 1;
                }
            }
            Err(e) => {
                failed += 1;
                println!("{code:40} FAILED {e:#}");
            }
        }
    }
    for (k, n) in &kinds {
        println!("{n:6} {k}");
    }
    println!(
        "{} rooms, {empty} place nothing, {failed} failures",
        rooms.len()
    );
    Ok(())
}

/// The Textures tab's catalogue over the whole dump (see `textures.rs`): folders and files per group, pack names
/// whose path was not rebuilt; with `expand`, every folder's other textures; with `decode`, also decodes every
/// texture listed and counts failures by format.
fn texture_catalogue(ar: &mut archive::Archive, mode: Option<&str>) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let t = std::time::Instant::now();
    let (mut folders, lost) = textures::catalogue(ar);
    println!(
        "catalogue: {} folders, {} named textures, {lost} pack names not rebuilt, {:.1} s",
        folders.len(),
        folders.iter().map(|f| f.files.len()).sum::<usize>(),
        t.elapsed().as_secs_f32()
    );
    let expand = matches!(mode, Some("expand" | "decode"));
    if expand {
        let t = std::time::Instant::now();
        let mut skipped = 0;
        for f in folders.iter_mut() {
            let (more, s) = textures::expand(ar, f);
            skipped += s;
            f.files.extend(more);
            f.expanded = true;
        }
        println!(
            "expanded in {:.1} s, {skipped} files unreadable",
            t.elapsed().as_secs_f32()
        );
    }
    for g in textures::Group::ALL {
        let fs: Vec<&textures::Folder> = folders.iter().filter(|f| f.group == g).collect();
        let files: usize = fs.iter().map(|f| f.files.len()).sum();
        let named = fs
            .iter()
            .flat_map(|f| &f.files)
            .filter(|f| f.path.is_some())
            .count();
        println!(
            "{:12} {:5} folders {:6} textures ({named} with a path)",
            g.label(),
            fs.len(),
            files
        );
        let mut big: Vec<&&textures::Folder> = fs.iter().collect();
        big.sort_by_key(|f| std::cmp::Reverse(f.files.len()));
        for f in big.iter().take(6) {
            println!("    {:5}  {}", f.files.len(), f.label());
        }
    }
    for f in folders.iter().filter(|f| f.path.is_empty()) {
        for &pi in f.packs.iter().take(5) {
            println!("not rebuilt: {}", ar.pack_names[pi as usize]);
        }
    }
    if mode == Some("decode") {
        let t = std::time::Instant::now();
        let mut ok = 0;
        let mut fails: BTreeMap<String, (usize, String)> = BTreeMap::new();
        let mut formats: BTreeMap<String, usize> = BTreeMap::new();
        for f in folders.iter().flat_map(|f| &f.files) {
            let r = ar.read_hash(f.hash).and_then(|b| texture::decode_bntx(&b));
            match r {
                Ok(img) => {
                    ok += 1;
                    *formats.entry(img.format).or_default() += 1;
                }
                Err(e) => {
                    let e = format!("{e:#}");
                    let key = e.split(':').next().unwrap_or(&e).to_string();
                    let entry = fails.entry(key).or_insert((0, f.name.clone()));
                    entry.0 += 1;
                }
            }
        }
        println!(
            "decoded {ok} in {:.0} s; formats {formats:?}",
            t.elapsed().as_secs_f32()
        );
        for (e, (n, example)) in fails {
            println!("  failed {n}: {e} (e.g. {example})");
        }
    }
    Ok(())
}

/// The Wwise sounds (see `wwise.rs`): counts per group and codec, total length; with `decode`, decodes every
/// `step`th sound and counts failures per codec (with an example).
fn audio_survey(
    romfs: &std::path::Path,
    mode: Option<&str>,
    step: Option<usize>,
) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let t = std::time::Instant::now();
    let (sounds, notes) = wwise::catalogue(romfs)?;
    println!(
        "{} sounds in {:.1} s",
        sounds.len(),
        t.elapsed().as_secs_f32()
    );
    for n in &notes {
        println!("note: {n}");
    }
    for g in wwise::Group::ALL {
        let gs: Vec<&wwise::Sound> = sounds.iter().filter(|s| s.group == g).collect();
        if gs.is_empty() {
            continue;
        }
        let mut codecs: BTreeMap<String, usize> = BTreeMap::new();
        let mut secs = 0.0;
        let mut loops = 0;
        for s in &gs {
            match &s.header {
                Ok(h) => {
                    *codecs.entry(h.codec.label()).or_default() += 1;
                    secs += h.seconds();
                    loops += h.looping.is_some() as usize;
                }
                Err(e) => *codecs.entry(format!("bad header: {e}")).or_default() += 1,
            }
        }
        println!(
            "{:15} {:6} sounds {:7.0} min, {loops} looped, {codecs:?}",
            g.label(),
            gs.len(),
            secs / 60.0
        );
        let titled = gs.iter().filter(|s| s.title.is_some()).count();
        let dev = gs.iter().filter(|s| s.dev_name.is_some()).count();
        println!("                titles {titled}, developer names {dev}");
        let named: Vec<&&wwise::Sound> = gs
            .iter()
            .filter(|s| s.title.is_some() || s.dev_name.is_some())
            .collect();
        for s in named.iter().step_by((named.len() / 5).max(1)).take(5) {
            println!(
                "                  {:>10}  {:30} {}",
                s.id,
                s.title.as_deref().unwrap_or(""),
                s.dev_name.as_deref().unwrap_or("")
            );
        }
    }
    if mode == Some("names") {
        for s in &sounds {
            if s.title.is_some() || s.dev_name.is_some() {
                println!(
                    "{}	{}	{}	{}",
                    s.group.label(),
                    s.id,
                    s.title.as_deref().unwrap_or(""),
                    s.dev_name.as_deref().unwrap_or("")
                );
            }
        }
    }
    if mode != Some("decode") {
        return Ok(());
    }
    let step = step.unwrap_or(1).max(1);
    let t = std::time::Instant::now();
    let mut ok: BTreeMap<String, usize> = BTreeMap::new();
    let mut fail: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut short = 0;
    let mut secs: BTreeMap<String, (f32, f32)> = BTreeMap::new(); // codec: (decode time, audio length)
    for s in sounds.iter().step_by(step) {
        let t0 = std::time::Instant::now();
        let codec = s
            .header
            .as_ref()
            .map(|h| h.codec.label())
            .unwrap_or_else(|_| "?".into());
        match wwise::read(romfs, s).and_then(|b| wem::decode(&b)) {
            Ok(pcm) => {
                let e = secs.entry(codec.clone()).or_default();
                e.0 += t0.elapsed().as_secs_f32();
                e.1 += pcm.samples.len() as f32 / pcm.channels as f32 / pcm.rate as f32;
                *ok.entry(codec).or_default() += 1;
                if let Ok(h) = &s.header {
                    let got = pcm.samples.len() / pcm.channels as usize;
                    let want = (h.samples as u64 * pcm.rate as u64 / h.rate.max(1) as u64) as usize;
                    if got + 2048 < want {
                        short += 1;
                    }
                }
            }
            Err(e) => {
                let e = format!("{e:#}");
                let key = format!("{codec}: {}", e.chars().take(60).collect::<String>());
                fail.entry(key)
                    .or_insert((0, format!("{} {:08x}", s.file, s.id)))
                    .0 += 1;
            }
        }
    }
    for (c, (t, len)) in &secs {
        println!(
            "  {c}: {t:.1} s to decode {len:.0} s of sound ({:.0}x real time)",
            len / t.max(1e-6)
        );
    }
    println!(
        "decoded {ok:?} in {:.0} s; {short} came out shorter than their header says",
        t.elapsed().as_secs_f32()
    );
    for (e, (n, ex)) in fail {
        println!("  failed {n}: {e} (e.g. {ex})");
    }
    Ok(())
}

/// Every file in the packs whose name contains `filter` that holds `text` (as ASCII or UTF-16 LE): pack, file
/// hash, size, offsets of the first matches. Packs are read whole, so a broad filter takes minutes.
fn grep_archive(ar: &mut archive::Archive, text: &str, filter: &str) -> anyhow::Result<()> {
    let ascii = text.as_bytes().to_vec();
    let wide: Vec<u8> = text.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let packs: Vec<(u32, String)> = ar
        .pack_names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.contains(filter))
        .map(|(i, n)| (i as u32, n.clone()))
        .collect();
    println!("{} packs to search", packs.len());
    let find = |hay: &[u8], needle: &[u8]| -> Vec<usize> {
        hay.windows(needle.len())
            .enumerate()
            .filter(|(_, w)| *w == needle)
            .map(|(i, _)| i)
            .take(4)
            .collect()
    };
    let mut hits = 0;
    for (pi, name) in packs {
        let Ok(hashes) = ar.pack_files_at(pi) else {
            continue;
        };
        for h in hashes {
            let Ok(b) = ar.read_hash(h) else { continue };
            for (kind, needle) in [("ascii", &ascii), ("utf16", &wide)] {
                let at = find(&b, needle);
                if !at.is_empty() {
                    hits += 1;
                    println!("{name}  #{h:016x}  {} bytes  {kind} at {at:x?}", b.len());
                }
            }
        }
    }
    println!("{hits} files");
    Ok(())
}

/// The effect catalogue (`effects::catalogue`): counts by kind and group, and every name whose path was not rebuilt.
pub fn effect_catalogue(ar: &archive::Archive) -> anyhow::Result<()> {
    let t = std::time::Instant::now();
    let (list, lost) = effects::catalogue(ar);
    let mut counts: std::collections::BTreeMap<(effects::Kind, effects::Group), usize> =
        Default::default();
    for e in &list {
        *counts.entry((e.kind, e.group)).or_default() += 1;
    }
    for ((k, g), n) in &counts {
        println!("{:<12} {:<11} {n}", k.label(), g.label());
    }
    for e in list.iter().filter(|e| e.path.is_none()) {
        println!("not rebuilt: {}", ar.pack_names[e.pack as usize]);
    }
    println!(
        "{} effects, {lost} paths not rebuilt, {:.2} s",
        list.len(),
        t.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Parses every `.ptcl` the effect list names (or the one given) and counts what the emitter fields hold, so the
/// offsets in `vfxb.rs` can be checked over the whole dump.
pub fn vfxb_check(ar: &mut archive::Archive, one: Option<&str>) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let paths: Vec<String> = match one {
        Some(p) => vec![p.to_string()],
        None => effects::catalogue(ar)
            .0
            .into_iter()
            .filter(|e| e.kind == effects::Kind::Particles)
            .filter_map(|e| e.path)
            .collect(),
    };
    let mut hist: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut add =
        |k: &'static str, v: String| *hist.entry(k).or_default().entry(v).or_default() += 1;
    let (mut files, mut failed, mut emitters, mut tex_ok, mut tex_missing) = (0, 0, 0, 0, 0);
    for p in &paths {
        files += 1;
        let parsed = ar.read(p).and_then(|b| {
            let f = vfxb::parse(&b)?;
            let names: Vec<String> = match &f.bntx {
                Some(r) => texture::texture_names(&b[r.clone()])?,
                None => Vec::new(),
            };
            Ok((f, names))
        });
        let (f, names) = match parsed {
            Ok(x) => x,
            Err(e) => {
                failed += 1;
                println!("FAILED {p}: {e:#}");
                continue;
            }
        };
        let mut stack: Vec<(&vfxb::Emitter, usize)> = f
            .sets
            .iter()
            .flat_map(|s| s.emitters.iter().map(|e| (e, 0)))
            .collect();
        while let Some((e, depth)) = stack.pop() {
            emitters += 1;
            stack.extend(e.children.iter().map(|c| (c, depth + 1)));
            if one.is_some() {
                println!(
                    "{:indent$}{} life {}+{} rate {} every {} start {} dur {} once {} vol {} r {:?} blend {} type {} scale {:?}+{}% speed out {} dir {} {:?} grav {} {:?} colour {:?} {:?} alpha {} {:?} keys {:?}",
                    "",
                    e.name,
                    e.life,
                    e.life_random,
                    e.rate,
                    e.interval,
                    e.start,
                    e.duration,
                    e.one_time,
                    e.volume,
                    e.volume_radius,
                    e.blend,
                    e.particle_type,
                    e.scale,
                    e.scale_random,
                    e.speed_out,
                    e.speed_dir,
                    e.direction,
                    e.gravity_scale,
                    e.gravity_dir,
                    e.color0_type,
                    e.color0,
                    e.alpha0,
                    e.alpha0_type,
                    e.key_counts,
                    indent = depth * 2
                );
                for s in e.samplers.iter().filter(|s| s.has_texture()) {
                    println!(
                        "{:indent$}  texture {} pattern {} x{} every {} div {:?} uv {:?}",
                        "",
                        f.texture_names.get(&s.texture).map_or("?", String::as_str),
                        s.pattern_type,
                        s.frames,
                        s.frames_per_step,
                        s.div,
                        s.uv_scale,
                        indent = depth * 2
                    );
                }
            }
            add("blend", e.blend.to_string());
            add("particle type", e.particle_type.to_string());
            add("volume", e.volume.to_string());
            add("colour0 type", format!("{:?}", e.color0_type));
            add("alpha0 type", format!("{:?}", e.alpha0_type));
            add("one time", e.one_time.to_string());
            add("depth", depth.to_string());
            add(
                "life",
                match e.life {
                    i32::MIN..=0 => "<=0",
                    1..=30 => "1-30",
                    31..=120 => "31-120",
                    121..=1000 => "121-1000",
                    _ => ">1000",
                }
                .into(),
            );
            add(
                "key counts ok",
                e.key_counts.iter().all(|&k| k <= 8).to_string(),
            );
            let keys_sorted = [&e.color0_keys, &e.alpha0_keys, &e.scale_keys]
                .iter()
                .all(|k| {
                    k.0.windows(2).all(|w| w[0].time <= w[1].time)
                        && k.0.iter().all(|k| (0.0..=1.0).contains(&k.time))
                });
            add("key times sorted in 0..1", keys_sorted.to_string());
            add(
                "scale random %",
                match e.scale_random {
                    0.0 => "0",
                    x if x <= 100.0 => "0-100",
                    _ => ">100",
                }
                .into(),
            );
            add(
                "speed random %",
                match e.speed_random {
                    0.0 => "0",
                    x if x <= 100.0 => "0-100",
                    _ => ">100",
                }
                .into(),
            );
            add("colour scale", format!("{:.1}", e.color_scale));
            add(
                "gravity dir len",
                format!(
                    "{:.1}",
                    e.gravity_dir.iter().map(|v| v * v).sum::<f32>().sqrt()
                ),
            );
            add(
                "air resistance",
                match e.air_resistance {
                    0.0 => "0",
                    x if x < 0.5 => "<0.5",
                    x if x <= 1.0 => "0.5-1",
                    _ => ">1",
                }
                .into(),
            );
            for (i, s) in e.samplers.iter().enumerate() {
                if !s.has_texture() {
                    continue;
                }
                add(
                    ["pattern type 0", "pattern type 1", "pattern type 2"][i],
                    s.pattern_type.to_string(),
                );
                add("sheet div", format!("{}x{}", s.div[0], s.div[1]));
                match f.texture_names.get(&s.texture) {
                    Some(n) if names.contains(n) => tex_ok += 1,
                    _ => tex_missing += 1,
                }
            }
        }
    }
    for (k, v) in &hist {
        println!("{k}: {v:?}");
    }
    println!(
        "{files} files, {failed} failed, {emitters} emitters; sampler textures found in the BNTX {tex_ok}, not found {tex_missing}"
    );
    Ok(())
}
