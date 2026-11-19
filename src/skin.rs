//! Skinned models on the GPU: a model's parts as three-d meshes with their materials (`build`), CPU linear blend
//! skinning of their vertices (`apply_skin`), and a playing clip's pose (`Clip`). Shared by the model view
//! (`main.rs`) and cutscenes (`cutscene_play.rs`), which show several characters at once.

use crate::{anim, archive, eye, materials, model, skeleton, texture};
use std::collections::HashMap;
use three_d::*;

/// A three-d material for a Trinity alpha mode: blended ones are drawn after the opaque ones, back to front, and do
/// not write depth (three-d's transparent pass).
pub(crate) fn transparent_material(
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
pub(crate) fn material_color(name: &str) -> Srgba {
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

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Shading {
    Textured,
    MaterialColours,
    Plain,
}

pub(crate) struct Loaded {
    pub(crate) model: model::Model,
    pub(crate) baked: HashMap<String, materials::Baked>,
    pub(crate) warnings: Vec<String>,
    pub(crate) meshes: Vec<Gm<Mesh, eye::ModelMaterial>>,
    /// Bind-pose vertex data of each mesh in `meshes`, for skinning; `None` for parts without skin weights.
    pub(crate) skin: Vec<Option<SkinPart>>,
    pub(crate) floor: Gm<Mesh, PhysicalMaterial>,
}

pub(crate) struct SkinPart {
    /// Parts of the same shape share their vertices, so their positions and normals are skinned once per frame.
    pub(crate) shape: String,
    pub(crate) positions: Vec<Vec3>,
    pub(crate) normals: Option<Vec<Vec3>>,
    /// Tangents are this part's own: they are computed from its triangles only (NaN at the shape's other
    /// vertices), and the handedness can differ between parts of one shape, so they are never shared.
    pub(crate) tangents: Option<Vec<Vec4>>,
    pub(crate) joints: Vec<[u8; 4]>,
    pub(crate) weights: Vec<[f32; 4]>,
}

impl SkinPart {
    /// Linear blend skinning: vertex v's matrix is the weighted average of its joints' skin matrices, or `None`
    /// when it has no usable joint (it then stays where it is).
    pub(crate) fn matrix(&self, v: usize, mats: &[Mat4]) -> Option<Mat4> {
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

pub(crate) type Skinned = (Vec<Vec3>, Option<Vec<Vec3>>);

/// Skinned positions and normals of a part (shared by every part of its shape).
pub(crate) fn skin_part(part: &SkinPart, mats: &[Mat4]) -> Skinned {
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
pub(crate) fn skin_tangents(part: &SkinPart, mats: &[Mat4]) -> Option<Vec<Vec4>> {
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
pub(crate) fn apply_skin(loaded: &mut Loaded, mats: &[Mat4]) {
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

pub(crate) fn cpu_texture(img: &texture::Image) -> CpuTexture {
    CpuTexture {
        data: TextureData::RgbaU8(img.rgba.clone()),
        width: img.width,
        height: img.height,
        ..Default::default()
    }
}

pub(crate) fn build(
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

/// One playing animation: the clip, its clock (in frames), and for each bone the track that drives it.
pub(crate) struct Clip {
    pub(crate) anim: anim::Animation,
    pub(crate) frame: f32,
    pub(crate) track_of_bone: Vec<Option<usize>>,
}

impl Clip {
    pub(crate) fn new(anim: anim::Animation, skel: Option<&skeleton::Skeleton>) -> Clip {
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
    pub(crate) fn last(&self) -> f32 {
        (self.anim.frames.max(2) - 1) as f32
    }

    /// How far through the clip the clock is, 0 to 1.
    pub(crate) fn phase(&self) -> f32 {
        self.frame / self.last()
    }

    /// Moves the clock on by `seconds`; false when it reached the end without looping.
    pub(crate) fn advance(&mut self, seconds: f32, looping: bool) -> bool {
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
    pub(crate) fn locals(&self, s: &skeleton::Skeleton) -> Vec<skeleton::Local> {
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
