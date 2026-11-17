//! Placed field models in the World view, drawn instanced: each terrain tile's trees (the models and `.trins`
//! files its `.trtrr` lists, see `terrain::tile_instances` and `instances.rs`), and what the streamed scenes
//! place: a tile's objects and the towns' buildings and props (`scene.rs`). A model is loaded with all its LODs
//! and its textures baked once, then drawn per group as one `InstancedMesh` per LOD and part. Groups further than
//! the draw distance from the camera are skipped.
//!
//! LODs: a model's mesh files are its LODs (1393 of the 1497 placed models have three, `probe --lod-survey`).
//! Each placement carries the game's switch distances (`scene::Node::lod_distances`; tile trees use the common
//! 20 / 40 m). In `LodMode::Auto` every instance takes the LOD for its distance from the camera, times a scale;
//! `LodMode::Fixed` forces one (or the model's last). Instances are re-sorted when the camera has moved.

use crate::archive::Archive;
use crate::eye::ModelMaterial;
use crate::materials::{self, AlphaMode};
use std::collections::HashMap;
use std::rc::Rc;
use three_d::*;

/// One model, ready to be instanced: per LOD, geometry and material per part.
pub struct CpuModel {
    lods: Vec<Vec<(CpuMesh, ModelMaterial)>>,
    /// Triangles per LOD.
    pub triangles: Vec<usize>,
    /// Per LOD: a camera-facing card (a material with `BillboardType` "AxisY": trees' last LOD, two triangles
    /// with shader "Grass"), turned about its y axis towards the camera.
    billboard: Vec<bool>,
    /// LOD 0's box in model space (`Model::min` / `max`).
    bounds: (Vec3, Vec3),
}

/// Whether a LOD is a card that turns to face the camera.
fn is_billboard(model: &crate::model::Model) -> bool {
    model.submeshes.iter().any(|s| {
        model
            .materials
            .get(&s.material)
            .and_then(|m| m.switches.get("BillboardType"))
            .is_some_and(|t| t == "AxisY")
    })
}

/// One LOD's parts. `gpu` shares materials between the LODs (they use the same material names).
fn lod_parts(
    context: &Context,
    model: &crate::model::Model,
    baked: &HashMap<String, materials::Baked>,
    gpu: &mut HashMap<String, ModelMaterial>,
) -> (Vec<(CpuMesh, ModelMaterial)>, usize) {
    let mut parts = Vec::new();
    let mut triangles = 0;
    for s in &model.submeshes {
        // "Water" parts are drawn by water.rs; "BGDistant" is a sky dome (the academy's, 4 km high) that would wall
        // off the view. "Shadow" parts are hulls that only cast shadows (1094 of the 1497 placed models have one,
        // `probe --town-scenes`); drawn, they poke through the real surface as flat grey facets
        if model
            .materials
            .get(&s.material)
            .is_some_and(|m| matches!(m.shader.as_str(), "Shadow" | "Water" | "BGDistant"))
        {
            continue;
        }
        let [su, sv, ou, ov] = model
            .materials
            .get(&s.material)
            .and_then(|m| m.params.get("UVScaleOffset"))
            .copied()
            .unwrap_or([1.0, 1.0, 0.0, 0.0]);
        let mut cpu = CpuMesh {
            positions: Positions::F32(s.positions.iter().map(|p| vec3(p[0], p[1], p[2])).collect()),
            indices: Indices::U32(s.indices.clone()),
            // as in main.rs: Trinity V runs down the image
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
        triangles += s.indices.len() / 3;
        let has_uvs = cpu.uvs.is_some();
        let material = gpu
            .entry(s.material.clone())
            .or_insert_with(|| {
                let b = baked.get(&s.material);
                let mut m = CpuMaterial {
                    albedo: Srgba::new(120, 140, 100, 255),
                    roughness: 0.9,
                    metallic: 0.0,
                    ..Default::default()
                };
                let mut cutout = false;
                if let Some(b) = b {
                    if let (Some(a), true) = (&b.albedo, has_uvs) {
                        m.albedo = Srgba::WHITE;
                        m.albedo_texture = Some(CpuTexture {
                            data: TextureData::RgbaU8(a.rgba.clone()),
                            width: a.width,
                            height: a.height,
                            ..Default::default()
                        });
                    } else if let Some(c) = b.color {
                        let to8 = |v: f32| (materials::linear_to_srgb(v) * 255.0).round() as u8;
                        m.albedo = Srgba::new(to8(c[0]), to8(c[1]), to8(c[2]), 255);
                    }
                    if let AlphaMode::Cutout(t) = b.alpha {
                        m.alpha_cutout = Some(t);
                        cutout = true;
                    }
                    // "Grass" (grass and the trees' billboard cards) is cut out by the texture's alpha
                    // without an alpha-test switch
                    let grass = model
                        .materials
                        .get(&s.material)
                        .is_some_and(|x| x.shader == "Grass");
                    if grass && b.albedo.as_ref().is_some_and(|a| a.has_alpha()) {
                        m.alpha_cutout = Some(0.5);
                        cutout = true;
                    }
                }
                let mut pm = PhysicalMaterial::new_opaque(context, &m);
                if cutout {
                    pm.render_states.cull = Cull::None;
                }
                ModelMaterial::physical(pm, m.alpha_cutout)
            })
            .clone();
        parts.push((cpu, material));
    }
    (parts, triangles)
}

/// Loads a model with every LOD it lists and its baked colour textures (baked once per material; a LOD that uses
/// materials LOD 0 does not is baked too). Cut-out materials are drawn two-sided, as leaves are single sheets. Parts with the
/// "Shadow" shader are left out. "BGLayer" / "BGLayerDepthFade" materials (two colour layers mixed by vertex
/// colour) show their first layer only. A LOD that fails to load falls back to the one before it.
pub fn load_model(context: &Context, ar: &mut Archive, path: &str) -> anyhow::Result<CpuModel> {
    let first = crate::model::load(ar, path, 0)?;
    let count = first.mesh_files.max(1);
    let (mut baked, _) = materials::bake(ar, &first);
    let mut gpu: HashMap<String, ModelMaterial> = HashMap::new();
    let (parts, tris) = lod_parts(context, &first, &baked, &mut gpu);
    let mut lods = vec![parts];
    let mut triangles = vec![tris];
    let mut billboard = vec![is_billboard(&first)];
    for lod in 1..count {
        match crate::model::load(ar, path, lod) {
            Ok(m) => {
                // `bake` covers the materials a model's parts use; a lower LOD can use ones LOD 0 does not
                if m.submeshes.iter().any(|s| !baked.contains_key(&s.material)) {
                    for (k, v) in materials::bake(ar, &m).0 {
                        baked.entry(k).or_insert(v);
                    }
                }
                let (parts, tris) = lod_parts(context, &m, &baked, &mut gpu);
                lods.push(parts);
                triangles.push(tris);
                billboard.push(is_billboard(&m));
            }
            Err(_) => break,
        }
    }
    Ok(CpuModel {
        lods,
        triangles,
        billboard,
        bounds: (first.min.into(), first.max.into()),
    })
}

/// A column-major matrix from an instance file as three-d's (also column-major) Mat4.
fn mat4(m: &[f32; 16]) -> Mat4 {
    Mat4::new(
        m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11], m[12], m[13],
        m[14], m[15],
    )
}

/// One model to draw at some places, with the distances where it switches LOD.
pub struct Item {
    pub model: String,
    pub lods: [f32; 2],
    pub matrices: Vec<Mat4>,
}

pub type Items = Vec<Item>;

/// How placed models pick their LOD.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LodMode {
    /// By each instance's distance from the camera and the game's switch distances.
    Auto,
    /// Always this LOD (or the model's last, if it has fewer).
    Fixed(usize),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LodSettings {
    pub mode: LodMode,
    /// Multiplies the switch distances (1 = as in the game).
    pub scale: f32,
}

impl Default for LodSettings {
    fn default() -> Self {
        LodSettings {
            mode: LodMode::Auto,
            scale: 1.0,
        }
    }
}

/// A tile's trees: the models and instance files its `.trtrr` lists.
pub fn tile_trees(ar: &mut Archive, trtrr_path: &str) -> (Items, Vec<String>) {
    let mut warnings = Vec::new();
    let list = match crate::terrain::tile_instances(ar, trtrr_path) {
        Ok(l) => l,
        Err(e) => return (Vec::new(), vec![format!("tree list: {e:#}")]),
    };
    let mut items = Vec::new();
    for (model, files) in list {
        let mut matrices = Vec::new();
        for f in &files {
            match ar.read(f).and_then(|b| crate::instances::parse(&b)) {
                Ok(ins) => matrices.extend(ins.matrices.iter().map(mat4)),
                Err(e) => warnings.push(format!("{f}: {e:#}")),
            }
        }
        items.push(Item {
            model,
            lods: crate::scene::DEFAULT_LOD_DISTANCES,
            matrices,
        });
    }
    (items, warnings)
}

/// What scenes place, with their sub-scenes (`scene::load_content`): single objects and instance files, grouped by model and LOD distances.
pub fn scene_items(ar: &mut Archive, scene_paths: &[String]) -> (Items, Vec<String>) {
    let mut warnings = Vec::new();
    // key: model and the distances' bits
    let mut by_model: HashMap<(String, [u32; 2]), Vec<Mat4>> = HashMap::new();
    let key = |model: String, l: [f32; 2]| (model, [l[0].to_bits(), l[1].to_bits()]);
    for path in scene_paths {
        let c = match crate::scene::load_content(ar, path) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("{path}: {e:#}"));
                continue;
            }
        };
        warnings.extend(c.errors.iter().map(|e| format!("{path}: {e}")));
        for p in c.placements {
            by_model
                .entry(key(p.model, p.lods))
                .or_default()
                .push(p.matrix);
        }
        for i in c.instancers {
            match ar.read(&i.file).and_then(|b| crate::instances::parse(&b)) {
                Ok(ins) => by_model
                    .entry(key(i.model, i.lods))
                    .or_default()
                    .extend(ins.matrices.iter().map(mat4)),
                Err(e) => warnings.push(format!("{}: {e:#}", i.file)),
            }
        }
    }
    let mut items: Items = by_model
        .into_iter()
        .map(|((model, l), matrices)| Item {
            model,
            lods: [f32::from_bits(l[0]), f32::from_bits(l[1])],
            matrices,
        })
        .collect();
    items.sort_by(|a, b| a.model.cmp(&b.model));
    (items, warnings)
}

/// Centre and radius (x/z) of the items' positions; `None` when there are none.
pub fn extent(items: &Items) -> Option<(Vec2, f32)> {
    let mut lo = vec2(f32::MAX, f32::MAX);
    let mut hi = vec2(f32::MIN, f32::MIN);
    for m in items.iter().flat_map(|i| &i.matrices) {
        lo = vec2(lo.x.min(m.w.x), lo.y.min(m.w.z));
        hi = vec2(hi.x.max(m.w.x), hi.y.max(m.w.z));
    }
    (lo.x <= hi.x).then(|| ((lo + hi) * 0.5, (hi - lo).magnitude() * 0.5))
}

/// One item on the GPU: per LOD the parts' instanced meshes, and which instances each LOD draws now.
struct Placed {
    matrices: Vec<Mat4>,
    lods: [f32; 2],
    meshes: Vec<Vec<Gm<InstancedMesh, ModelMaterial>>>,
    triangles: Vec<usize>,
    /// Per LOD: turned to face the camera (`CpuModel::billboard`).
    billboard: Vec<bool>,
    /// The LOD each instance is in.
    assigned: Vec<u8>,
    /// Instances per LOD.
    counts: Vec<usize>,
}

impl Placed {
    fn lod_for(&self, m: &Mat4, eye: Vec3, s: LodSettings) -> u8 {
        let last = self.meshes.len().saturating_sub(1);
        let lod = match s.mode {
            LodMode::Fixed(k) => k,
            LodMode::Auto => {
                let d = vec3(m.w.x, m.w.y, m.w.z).distance(eye) / s.scale.max(0.01);
                if d < self.lods[0] {
                    0
                } else if d < self.lods[1] {
                    1
                } else {
                    2
                }
            }
        };
        lod.min(last) as u8
    }

    /// Re-sorts the instances; uploads the LODs whose set changed, and billboard LODs (turned to the camera)
    /// every time.
    fn update(&mut self, eye: Vec3, s: LodSettings) {
        let assigned: Vec<u8> = self
            .matrices
            .iter()
            .map(|m| self.lod_for(m, eye, s))
            .collect();
        if assigned == self.assigned && !self.billboard.iter().any(|b| *b) {
            return;
        }
        for (l, meshes) in self.meshes.iter_mut().enumerate() {
            let billboard = self.billboard.get(l).copied().unwrap_or(false);
            let changed = assigned
                .iter()
                .zip(&self.assigned)
                .any(|(a, b)| (*a as usize == l) != (*b as usize == l));
            let transformations: Vec<Mat4> = self
                .matrices
                .iter()
                .zip(&assigned)
                .filter(|(_, a)| **a as usize == l)
                .map(|(m, _)| if billboard { facing(m, eye) } else { *m })
                .collect();
            self.counts[l] = transformations.len();
            if (changed || billboard) && !transformations.is_empty() {
                let instances = Instances {
                    transformations,
                    ..Default::default()
                };
                for gm in meshes.iter_mut() {
                    gm.geometry.set_instances(&instances);
                }
            }
        }
        self.assigned = assigned;
    }
}

/// An instance's matrix turned about y to face `eye`: its position and scale, the card's normal (+z) towards the
/// camera in x/z. Its own rotation is dropped (a card keeps only the yaw it is given here).
fn facing(m: &Mat4, eye: Vec3) -> Mat4 {
    let pos = vec3(m.w.x, m.w.y, m.w.z);
    let len = |c: Vec4| vec3(c.x, c.y, c.z).magnitude();
    let to = eye - pos;
    Mat4::from_translation(pos)
        * Mat4::from_angle_y(Rad(to.x.atan2(to.z)))
        * Mat4::from_nonuniform_scale(len(m.x), len(m.y), len(m.z))
}

/// A set of placed models (a tile's trees, a tile's objects, a town).
pub struct Group {
    placed: Vec<Placed>,
    /// Centre and radius in x/z of everything placed, for the draw distance.
    pub center: Vec2,
    pub radius: f32,
    pub instances: usize,
    /// Triangles if every instance were drawn at LOD 0.
    pub triangles: usize,
    /// Models or instance files that failed, as messages.
    pub warnings: Vec<String>,
    /// World-space box around every instance's model, `None` when nothing is placed.
    pub aabb: Option<(Vec3, Vec3)>,
    /// Camera position and settings of the last LOD sort.
    last: Option<(Vec3, LodSettings)>,
}

/// Puts the items on the GPU, every instance at LOD 0 until the first `update_lods`. `cache` keeps each
/// model's CPU parts, shared between groups.
pub fn build(
    context: &Context,
    ar: &mut Archive,
    items: Items,
    mut warnings: Vec<String>,
    cache: &mut HashMap<String, Result<Rc<CpuModel>, String>>,
) -> Group {
    let (center, radius) = extent(&items).unwrap_or((vec2(0.0, 0.0), 0.0));
    let mut out = Group {
        placed: Vec::new(),
        center,
        radius,
        instances: 0,
        triangles: 0,
        warnings: Vec::new(),
        aabb: None,
        last: None,
    };
    for item in items {
        if item.matrices.is_empty() {
            continue;
        }
        let model = cache
            .entry(item.model.clone())
            .or_insert_with(|| {
                load_model(context, ar, &item.model)
                    .map(Rc::new)
                    .map_err(|e| format!("{e:#}"))
            })
            .clone();
        let model = match model {
            Ok(m) => m,
            Err(e) => {
                warnings.push(format!("{}: {e}", item.model));
                continue;
            }
        };
        let n = item.matrices.len();
        let (lo, hi) = model.bounds;
        for m in &item.matrices {
            for k in 0..8 {
                let c = vec3(
                    if k & 1 == 0 { lo.x } else { hi.x },
                    if k & 2 == 0 { lo.y } else { hi.y },
                    if k & 4 == 0 { lo.z } else { hi.z },
                );
                let w = (*m * c.extend(1.0)).truncate();
                out.aabb = Some(match out.aabb {
                    None => (w, w),
                    Some((a, b)) => (
                        vec3(a.x.min(w.x), a.y.min(w.y), a.z.min(w.z)),
                        vec3(b.x.max(w.x), b.y.max(w.y), b.z.max(w.z)),
                    ),
                });
            }
        }
        out.instances += n;
        out.triangles += model.triangles.first().copied().unwrap_or(0) * n;
        let instances = Instances {
            transformations: item.matrices.clone(),
            ..Default::default()
        };
        let meshes: Vec<Vec<Gm<InstancedMesh, ModelMaterial>>> = model
            .lods
            .iter()
            .map(|parts| {
                parts
                    .iter()
                    .map(|(cpu, material)| {
                        Gm::new(
                            InstancedMesh::new(context, &instances, cpu),
                            material.clone(),
                        )
                    })
                    .collect()
            })
            .collect();
        let mut counts = vec![0; meshes.len()];
        counts[0] = n;
        out.placed.push(Placed {
            matrices: item.matrices,
            lods: item.lods,
            meshes,
            triangles: model.triangles.clone(),
            billboard: model.billboard.clone(),
            assigned: vec![0; n],
            counts,
        });
    }
    out.warnings = warnings;
    out
}

impl Group {
    /// The meshes to draw: each LOD that has instances now.
    pub fn objects(&self) -> impl Iterator<Item = &dyn Object> {
        self.placed.iter().flat_map(|p| {
            p.meshes
                .iter()
                .zip(&p.counts)
                .filter(|(_, n)| **n > 0)
                .flat_map(|(meshes, _)| meshes.iter().map(|m| m as &dyn Object))
        })
    }

    /// Whether any part of the group lies within `distance` of `eye` (in x/z).
    pub fn near(&self, eye: Vec3, distance: f32) -> bool {
        vec2(eye.x, eye.z).distance(self.center) < distance + self.radius
    }

    /// Picks every instance's LOD for a camera at `eye`; does nothing while the camera stays within a metre of
    /// the last sort and the settings are the same.
    pub fn update_lods(&mut self, eye: Vec3, s: LodSettings) {
        if let Some((e, last)) = self.last
            && last == s
            && e.distance(eye) < 1.0
        {
            return;
        }
        for p in &mut self.placed {
            p.update(eye, s);
        }
        self.last = Some((eye, s));
    }

    /// Triangles drawn now (by LOD), and instances per LOD (0, 1, 2+).
    pub fn drawn(&self) -> (usize, [usize; 3]) {
        let mut tris = 0;
        let mut per = [0; 3];
        for p in &self.placed {
            for (l, n) in p.counts.iter().enumerate() {
                tris += n * p.triangles.get(l).copied().unwrap_or(0);
                per[l.min(2)] += n;
            }
        }
        (tris, per)
    }
}
