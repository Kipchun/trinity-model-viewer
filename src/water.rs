//! Water in the World view: the sea, rivers, lakes, ponds and waterfalls: every "Water" part of the models the
//! map's resident scenes place (`maps::Map::resident`; a room's pools too). The other parts of those models are
//! drawn as placed objects (`plants.rs` skips "Water").
//!
//! Paldea (checked with `probe --water`):
//! - `outdoor_resident` places `objects_un_sea01` (the sea: one sheet 100 km across at **y = 27 m**, coast wave
//!   strips at 27.1 m, a 27-32 m skirt over the sea holes) and `objects_un_water01` (every river, lake, pond
//!   and waterfall of Paldea in one model of 21 parts, 6822 triangles), both at the identity transform.
//! - `town_c01_field_resident` places `objects_un_c01water01`: the academy's pools, canals and falls, plus a
//!   second sea sheet at 27 m, left out here (it would z-fight with `objects_un_sea01`). Its cliffs and skybox
//!   are not water.
//! - Kitakami (`dlc1_resident`): `objects_un_dlc1_water01` (rivers, falls). The Terarium (`dlc2_resident`):
//!   `objects_un_dlc2_sea01_n/e` (sea), `objects_un_dlc2_water01_s/e/w` (lakes), `objects_un_dlc2_water_outside01`,
//!   the dome's water. Sea parts are those of models named `*_sea01*`. Whole-map numbers below are Paldea's.
//! - Sea level is 27 m: the sea's vertices that lie over ground sit at the shoreline (median 0.3 m above it), so
//!   the terrain next to the sea holes is sea bed, not beach. Rivers and lakes meet their banks a metre or so
//!   below the ground (median -0.7 to -1.3 m: their edges tuck under the terrain); waterfalls stand above it.
//!
//! Material (`.trmtr` shader "Water", render type "Blend", no colour texture): `BaseColor` (rgb, a = opacity),
//! `NormalMap` / `NormalMap1` (BC5, two layers with their own `UVScaleOffset` / `UVScaleOffset1` and scrolling
//! `ScrollUVSpeed` / `ScrollUVSpeed1` in UV per second), `FoamMaskMap` (BC4, tiled by `FoamMaskUVScale`),
//! `FoamColor`, `FoamMaskIntensity`, `FoamSharpness`, `Roughness`, switches `EnableVertexAlpha` /
//! `EnableVertexFoamMask`. Vertex colour (semantic 5, RGBA8): a = opacity (river edges fade out), r = foam
//! (waterfalls ~0.75, rivers ~0.27). `FlowMap` is a 1 x 1 constant on the rivers (flow along v) and is not used;
//! nor are the wave animation, depth fade, refraction and fog switches.
//! **Approximation** (`shaders/water.frag`): two scrolling normal maps, the viewer's lights plus a Fresnel
//! reflection of a fixed sky colour, foam = (mask x vertex r x intensity) ^ sharpness. The game's formula is not
//! known.

use crate::archive::Archive;
use crate::model::Material as TrMaterial;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use three_d::*;

/// One water material: its parameters and textures. The time is shared by all of them.
pub struct WaterMaterial {
    normal0: Rc<Texture2D>,
    normal1: Rc<Texture2D>,
    foam: Rc<Texture2D>,
    base: Vec4,
    foam_color: Vec4,
    uv0: Vec4,
    uv1: Vec4,
    scroll0: Vec2,
    scroll1: Vec2,
    foam_scale: f32,
    foam_intensity: f32,
    foam_sharpness: f32,
    roughness: f32,
    vertex_alpha: bool,
    vertex_foam: bool,
    time: Rc<Cell<f32>>,
}

impl Material for WaterMaterial {
    fn fragment_shader_source(&self, lights: &[&dyn Light]) -> String {
        let mut s = lights_shader_source(lights);
        s.push_str(ToneMapping::fragment_shader_source());
        s.push_str(ColorMapping::fragment_shader_source());
        s.push_str(include_str!("shaders/water.frag"));
        s
    }

    fn id(&self) -> EffectMaterialId {
        // public-use range (0x0000..0x4FFF); terrain 0x0100, eye 0x0101, model materials 0x02xx
        EffectMaterialId(0x0102)
    }

    fn use_uniforms(&self, program: &Program, viewer: &dyn Viewer, lights: &[&dyn Light]) {
        viewer.tone_mapping().use_uniforms(program);
        viewer.color_mapping().use_uniforms(program);
        program.use_uniform_if_required("cameraPosition", viewer.position());
        for (i, light) in lights.iter().enumerate() {
            light.use_uniforms(program, i as u32);
        }
        program.use_texture("normal0", &self.normal0);
        program.use_texture("normal1", &self.normal1);
        program.use_texture("foamMask", &self.foam);
        program.use_uniform_if_required("baseColor", self.base);
        program.use_uniform_if_required("foamColor", self.foam_color);
        program.use_uniform_if_required("uvScaleOffset0", self.uv0);
        program.use_uniform_if_required("uvScaleOffset1", self.uv1);
        program.use_uniform_if_required("scroll0", self.scroll0);
        program.use_uniform_if_required("scroll1", self.scroll1);
        program.use_uniform_if_required("foamScale", self.foam_scale);
        program.use_uniform_if_required("foamIntensity", self.foam_intensity);
        program.use_uniform_if_required("foamSharpness", self.foam_sharpness);
        program.use_uniform_if_required("roughness", self.roughness);
        program.use_uniform_if_required("vertexAlpha", self.vertex_alpha as i32);
        program.use_uniform_if_required("vertexFoam", self.vertex_foam as i32);
        program.use_uniform_if_required("time", self.time.get());
    }

    fn render_states(&self) -> RenderStates {
        RenderStates {
            write_mask: WriteMask::COLOR,
            blend: Blend::TRANSPARENCY,
            cull: Cull::None,
            ..Default::default()
        }
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Transparent
    }
}

struct Part {
    gm: Gm<Mesh, WaterMaterial>,
    /// Part of the sea (`objects_un_sea01_*` materials), shown with the "Sea" toggle; else inland water.
    sea: bool,
}

/// The overworld's water, loaded once.
pub struct Water {
    parts: Vec<Part>,
    pub sea_triangles: usize,
    pub inland_triangles: usize,
    pub warnings: Vec<String>,
    time: Rc<Cell<f32>>,
    start: std::time::Instant,
}

/// A flat 1 x 1 texture: the stand-in for a normal or foam map that is missing or does not decode.
fn flat(context: &Context, rgba: [u8; 4]) -> Rc<Texture2D> {
    Rc::new(Texture2D::new(
        context,
        &CpuTexture {
            data: TextureData::RgbaU8(vec![rgba]),
            width: 1,
            height: 1,
            ..Default::default()
        },
    ))
}

/// Textures by path, decoded and uploaded once; failures become warnings and the stand-in.
struct Textures<'a> {
    context: &'a Context,
    cache: HashMap<String, Rc<Texture2D>>,
    warnings: &'a mut Vec<String>,
}

impl Textures<'_> {
    fn get(&mut self, ar: &mut Archive, path: Option<String>, fallback: [u8; 4]) -> Rc<Texture2D> {
        let Some(path) = path else {
            return flat(self.context, fallback);
        };
        if let Some(t) = self.cache.get(&path) {
            return t.clone();
        }
        let t = match ar.read(&path).and_then(|b| crate::texture::decode_bntx(&b)) {
            Ok(img) => Rc::new(Texture2D::new(
                self.context,
                &CpuTexture {
                    data: TextureData::RgbaU8(img.rgba),
                    width: img.width,
                    height: img.height,
                    wrap_s: Wrapping::Repeat,
                    wrap_t: Wrapping::Repeat,
                    ..Default::default()
                },
            )),
            Err(e) => {
                self.warnings.push(format!("{path}: {e:#}"));
                flat(self.context, fallback)
            }
        };
        self.cache.insert(path, t.clone());
        t
    }
}

fn material(
    ar: &mut Archive,
    textures: &mut Textures,
    dir: &str,
    m: &TrMaterial,
    time: &Rc<Cell<f32>>,
) -> WaterMaterial {
    let tex = |role: &str| m.texture(role).map(|f| crate::model::resolve(dir, f));
    let p = |name: &str, def: [f32; 4]| m.params.get(name).copied().unwrap_or(def);
    let f = |name: &str, def: f32| m.floats.get(name).copied().unwrap_or(def);
    let on = |name: &str| m.switches.get(name).is_some_and(|v| v == "True");
    let [r, g, b, a] = p("BaseColor", [0.2, 0.5, 0.6, 0.6]);
    let [s0, s1, ..] = p("ScrollUVSpeed", [0.0; 4]);
    let [t0, t1, ..] = p("ScrollUVSpeed1", [0.0; 4]);
    let flat_normal = [128, 128, 255, 255];
    WaterMaterial {
        normal0: textures.get(ar, tex("NormalMap"), flat_normal),
        normal1: textures.get(ar, tex("NormalMap1"), flat_normal),
        foam: textures.get(ar, tex("FoamMaskMap"), [0, 0, 0, 255]),
        base: vec4(r, g, b, a),
        foam_color: p("FoamColor", [1.0; 4]).into(),
        uv0: p("UVScaleOffset", [1.0, 1.0, 0.0, 0.0]).into(),
        uv1: p("UVScaleOffset1", [1.0, 1.0, 0.0, 0.0]).into(),
        scroll0: vec2(s0, s1),
        scroll1: vec2(t0, t1),
        foam_scale: f("FoamMaskUVScale", 1.0),
        foam_intensity: f("FoamMaskIntensity", 1.0),
        foam_sharpness: f("FoamSharpness", 1.0).clamp(0.25, 8.0),
        roughness: f("Roughness", 0.1).clamp(0.02, 1.0),
        vertex_alpha: on("EnableVertexAlpha"),
        vertex_foam: on("EnableVertexFoamMask"),
        time: time.clone(),
    }
}

/// A vertex colour channel as an sRGB byte, so that three-d's sRGB-to-linear conversion on upload gives the
/// stored value back in the shader.
fn encode(v: f32) -> u8 {
    (crate::materials::linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8
}

impl Water {
    /// Loads the water scenes' models and keeps their "Water" parts. Problems become warnings.
    /// `scenes`: the map's resident scenes (`maps::Map::resident`), read with their sub-scenes.
    pub fn load(context: &Context, ar: &mut Archive, scenes: &[String]) -> Water {
        let time = Rc::new(Cell::new(0.0));
        let mut warnings = Vec::new();
        let mut parts = Vec::new();
        let (mut sea_triangles, mut inland_triangles) = (0, 0);
        let mut cache = HashMap::new();
        for path in scenes {
            let content = match crate::scene::load_content(ar, path) {
                Ok(c) => c,
                Err(e) => {
                    warnings.push(format!("{path}: {e:#}"));
                    continue;
                }
            };
            for p in content.placements {
                // the sea models: `objects_un_sea01` (Paldea), `objects_un_dlc2_sea01_*` (the Terarium)
                let sea_model = p.model.contains("_sea01");
                let model = match crate::model::load(ar, &p.model, 0) {
                    Ok(m) => m,
                    Err(e) => {
                        warnings.push(format!("{}: {e:#}", p.model));
                        continue;
                    }
                };
                let mut textures = Textures {
                    context,
                    cache: std::mem::take(&mut cache),
                    warnings: &mut warnings,
                };
                let mut materials: HashMap<String, WaterMaterial> = HashMap::new();
                for s in &model.submeshes {
                    let Some(m) = model.materials.get(&s.material) else {
                        continue;
                    };
                    if m.shader != "Water" {
                        continue;
                    }
                    // a copy of Paldea's sea in another model (the central city's water) is left out
                    if !sea_model && s.material.starts_with("objects_un_sea01_") {
                        continue;
                    }
                    let sea = sea_model;
                    let wm = materials
                        .entry(s.material.clone())
                        .or_insert_with(|| material(ar, &mut textures, &model.dir, m, &time));
                    let gm = Gm::new(
                        Mesh::new(context, &cpu_mesh(s, p.matrix)),
                        clone_material(wm),
                    );
                    let n = s.indices.len() / 3;
                    if sea {
                        sea_triangles += n;
                    } else {
                        inland_triangles += n;
                    }
                    parts.push(Part { gm, sea });
                }
                cache = textures.cache;
            }
        }
        Water {
            parts,
            sea_triangles,
            inland_triangles,
            warnings,
            time,
            start: std::time::Instant::now(),
        }
    }

    /// Draws the sea and / or the inland water, after everything opaque (they blend over it).
    pub fn render(
        &self,
        target: &RenderTarget,
        camera: &Camera,
        lights: &[&dyn Light],
        sea: bool,
        inland: bool,
    ) {
        self.time.set(self.start.elapsed().as_secs_f32());
        let objects = self
            .parts
            .iter()
            .filter(|p| if p.sea { sea } else { inland })
            .map(|p| &p.gm);
        target.render(camera, objects, lights);
    }
}

/// `Gm` owns its material; the parts of one material share textures and time through `Rc`s.
fn clone_material(m: &WaterMaterial) -> WaterMaterial {
    WaterMaterial {
        normal0: m.normal0.clone(),
        normal1: m.normal1.clone(),
        foam: m.foam.clone(),
        time: m.time.clone(),
        ..*m
    }
}

/// A submesh in world space (its placement's matrix applied), with raw UVs and vertex colours for the shader.
fn cpu_mesh(s: &crate::model::SubMesh, matrix: Mat4) -> CpuMesh {
    let normal_matrix = matrix.invert().unwrap_or(Mat4::identity()).transpose();
    let mut cpu = CpuMesh {
        positions: Positions::F32(
            s.positions
                .iter()
                .map(|p| (matrix * vec4(p[0], p[1], p[2], 1.0)).truncate())
                .collect(),
        ),
        indices: Indices::U32(s.indices.clone()),
        // three-d flips v on upload; this hands the shader Trinity's own v, as the placed models' path does
        uvs: s
            .uvs
            .as_ref()
            .map(|uv| uv.iter().map(|t| vec2(t[0], 1.0 - t[1])).collect()),
        colors: s.colors.as_ref().map(|c| {
            c.iter()
                .map(|c| {
                    Srgba::new(
                        encode(c[0]),
                        encode(c[1]),
                        encode(c[2]),
                        (c[3].clamp(0.0, 1.0) * 255.0).round() as u8,
                    )
                })
                .collect()
        }),
        ..Default::default()
    };
    match &s.normals {
        Some(n) => {
            cpu.normals = Some(
                n.iter()
                    .map(|n| {
                        (normal_matrix * vec4(n[0], n[1], n[2], 0.0))
                            .truncate()
                            .normalize()
                    })
                    .collect(),
            )
        }
        None => cpu.compute_normals(),
    }
    cpu
}
