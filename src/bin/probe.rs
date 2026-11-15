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
#[path = "../anim.rs"]
mod anim;
#[path = "../archive.rs"]
mod archive;
#[path = "../config.rs"]
mod config;
#[path = "../fb.rs"]
mod fb;
#[path = "../model.rs"]
mod model;
#[path = "../skeleton.rs"]
mod skeleton;
#[path = "../texture.rs"]
mod texture;

use fb::{Table, u32_at};

fn dump(t: &Table, depth: usize, max_depth: usize) {
    let pad = "  ".repeat(depth);
    for i in 0..t.slot_count() {
        let Some(a) = t.field(i) else { continue };
        let raw = u32_at(t.buf, a).unwrap_or(0);
        let mut line = format!("{pad}[{i}] u32={raw} f32={:.4}", f32::from_bits(raw));
        if let Ok(Some(s)) = t.string(i) {
            if !s.is_empty() && s.len() < 100 && s.chars().all(|c| c.is_ascii_graphic()) {
                line += &format!(" str={s:?}");
            }
        }
        if let Ok((_, n)) = t.vector(i) {
            if n < 100000 {
                line += &format!(" vec_len={n}");
            }
        }
        println!("{line}");
        if depth / 2 < max_depth {
            if let Ok(v) = t.tables(i) {
                if !v.is_empty() && v.len() < 200 && v[0].slot_count() < 24 {
                    for (k, e) in v.iter().enumerate().take(if depth == 0 { 3 } else { 40 }) {
                        println!("{pad}  [{i}][{k}]:");
                        dump(e, depth + 2, max_depth);
                    }
                    continue;
                }
            }
            if let Ok(Some(sub)) = t.table(i) {
                if sub.slot_count() < 24 {
                    println!("{pad}  (table)");
                    dump(&sub, depth + 2, max_depth);
                }
            }
        }
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
    let b = ar.read(&args[0])?;
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
                if vi == 0 {
                    if let Some(tr) = s.translation {
                        let rest = three_d::Vec3::new(
                            b.translation[0],
                            b.translation[1],
                            b.translation[2],
                        );
                        tdist += (tr - rest).magnitude() as f64;
                        tn += 1;
                    }
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
                        for i in 0..4 {
                            if i == idx {
                                v[i] = big
                            } else {
                                v[i] = q[kk];
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
    results.sort_by(|a, b| b.0.cmp(&a.0));
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
