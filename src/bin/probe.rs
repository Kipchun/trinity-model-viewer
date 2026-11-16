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
//!   cargo run --release --bin probe -- --texture <bntx> <layer> <out>  decoded RGBA8 of one layer (raw, for study)
//!   cargo run --release --bin probe -- --splat                every tile's splat names vs. the layers its control map uses
//!   cargo run --release --bin probe -- --splat-seams          do neighbouring control maps agree along shared edges?
//!   cargo run --release --bin probe -- --characters            list the character models discovery finds
#[path = "../anim.rs"]
mod anim;
#[path = "../archive.rs"]
mod archive;
#[path = "../config.rs"]
mod config;
#[path = "../fb.rs"]
mod fb;
#[path = "../materials.rs"]
mod materials;
#[path = "../model.rs"]
mod model;
#[path = "../player.rs"]
mod player;
#[path = "../skeleton.rs"]
mod skeleton;
#[path = "../terrain.rs"]
mod terrain;
#[path = "../texture.rs"]
mod texture;

use fb::{Table, u32_at};

fn dump(t: &Table, depth: usize, max_depth: usize) {
    let pad = "  ".repeat(depth);
    for i in 0..t.slot_count() {
        let Some(a) = t.field(i) else { continue };
        let raw = u32_at(t.buf, a).unwrap_or(0);
        let mut line = format!("{pad}[{i}] u32={raw} f32={:.4}", f32::from_bits(raw));
        if let Ok(Some(s)) = t.string(i)
            && !s.is_empty()
            && s.len() < 100
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
    let m = match model::load(ar, path, 0) {
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
    for w in &warnings {
        println!("  warn {w}");
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
    if args[0] == "--terrain" {
        return terrain_check(&mut ar);
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
