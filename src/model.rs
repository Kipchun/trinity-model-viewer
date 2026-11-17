//! Trinity model loading: .trmdl (model) -> .trmsh (mesh shapes and vertex layouts) + .trmbf (vertex/index buffers).
//! Field slots, checked against the game's files:
//!   trmdl  { 1: meshes [{0: filename}], 2: skeleton {0: filename}, 3: materials [string], 4: lods [...] }
//!   trmsh  { 1: shapes [MeshShape], 2: buffer file name }
//!   MeshShape { 0: name, 1: bounds, 2: index type (0 u8, 1 u16, 2 u32), 3: [VertexAccessors],
//!               4: [MaterialInfo {0: index count, 1: index offset, 3: material name}], 12: mesh name }
//!   VertexAccessors { 0: [Attribute {1: semantic, 2: layer, 3: format, 4: byte offset}], 1: [{0: stride}] }
//!   Semantics: 1 position, 2 normal, 3 tangent, 5 colour, 6 texcoord, 7 joints, 8 weights. Formats: 20 RGBA8
//!   unorm, 22 RGBA8 uint, 39 RGBA16 unorm, 43 RGBA16 float, 48 RG32F, 51 RGB32F, 54 RGBA32F.
//!   trmbf  { 1: [Buffer {0: [{0: index bytes}], 1: [{0: vertex bytes}] per accessor stream}] }
//!   trmtr  { 1: [Material {0: name, 1: [Shader {0: name, 1: [{0: switch, 1: value string}]}], 2: [Texture {0: role, 1: .bntx file, 2: slot}],
//!               4: [{0: name, 1: float}] (e.g. LayerMaskScale1), 7: [{0: name, 1: float4}] (e.g. UVScaleOffset,
//!               BaseColorLayer1), 15: render type}] }
//!   A parameter stored without a value has the FlatBuffers default, 0.

use crate::archive::Archive;
use crate::fb::Table;
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;

// Vertex attribute semantics
const POSITION: u32 = 1;
const NORMAL: u32 = 2;
const COLOR: u32 = 5;
const TEXCOORD: u32 = 6;
const BLEND_INDICES: u32 = 7;
const BLEND_WEIGHTS: u32 = 8;

pub struct SubMesh {
    pub shape: String,
    pub material: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    /// Vertex colour, layer 0 (RGBA8 in the meshes seen). Water uses it for edge alpha and foam.
    pub colors: Option<Vec<[f32; 4]>>,
    /// Skinning: up to four joints per vertex (indices into the skeleton's joint list) and their weights.
    pub joints: Option<Vec<[u8; 4]>>,
    pub weights: Option<Vec<[f32; 4]>>,
    pub indices: Vec<u32>,
}

/// What the viewer uses of a material: texture file names by role, and its float4 parameters.
#[derive(Default, Clone)]
pub struct Material {
    pub textures: Vec<(String, String)>,
    pub params: HashMap<String, [f32; 4]>,
    pub floats: HashMap<String, f32>,
    /// trmtr slot 15: "Opaque", "Transparent", ...
    pub render_type: String,
    /// The first shader's name ("Standard", "SSS", "Eye", ...) and its switches ("EnableAlphaTest" -> "True").
    pub shader: String,
    pub switches: HashMap<String, String>,
}

impl Material {
    pub fn texture(&self, role: &str) -> Option<&str> {
        self.textures
            .iter()
            .find(|(r, _)| r == role)
            .map(|(_, f)| f.as_str())
    }
}

pub struct Model {
    pub path: String,
    pub dir: String,
    pub materials: HashMap<String, Material>,
    pub mesh_file: String,
    pub mesh_files: usize,
    /// The .trskl file the model names (trmdl slot 2), relative to `dir`.
    pub skeleton_file: Option<String>,
    pub submeshes: Vec<SubMesh>,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Model {
    pub fn triangle_count(&self) -> usize {
        self.submeshes.iter().map(|s| s.indices.len() / 3).sum()
    }
}

/// `dir` + a file name that may contain "./" and "../" parts, as an archive path.
pub fn resolve(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    parts.join("/")
}

fn dir_of(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..=i]).unwrap_or("")
}

/// Load a model; `lod` picks which of the model's mesh files to use (0 = most detailed).
pub fn load(ar: &mut Archive, path: &str, lod: usize) -> Result<Model> {
    let dir = dir_of(path);
    let mdl_buf = ar.read(path)?;
    let mdl = Table::root(&mdl_buf, 0)?;
    let mesh_names: Vec<String> = mdl
        .tables(1)?
        .iter()
        .filter_map(|t| t.string(0).ok().flatten())
        .collect();
    if mesh_names.is_empty() {
        bail!("model lists no mesh files");
    }
    let mesh_file = mesh_names[lod.min(mesh_names.len() - 1)]
        .trim_start_matches("./")
        .to_string();
    let msh_buf = ar.read(&resolve(dir, &mesh_file))?;
    let msh = Table::root(&msh_buf, 0)?;
    let buf_name = msh
        .string(2)?
        .ok_or_else(|| anyhow!("{mesh_file}: no buffer file name"))?;
    let mbf_buf = ar.read(&resolve(dir, &buf_name))?;
    let mbf = Table::root(&mbf_buf, 0)?;
    let buffers = mbf.tables(1)?;

    let mut submeshes = Vec::new();
    for (si, shape) in msh.tables(1)?.iter().enumerate() {
        let name = shape.string(0)?.unwrap_or_default();
        let buffer = buffers
            .get(si)
            .ok_or_else(|| anyhow!("{name}: no buffer {si}"))?;
        let parts = load_shape(shape, buffer).with_context(|| format!("shape {name}"))?;
        submeshes.extend(parts);
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for s in &submeshes {
        for p in &s.positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    if submeshes.is_empty() {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    let mut materials = HashMap::new();
    for mtr in mdl.strings(3)? {
        // A missing or unreadable material file only costs the textures, not the model.
        if let Ok(buf) = ar.read(&resolve(dir, &mtr))
            && let Ok(m) = parse_materials(&buf)
        {
            materials.extend(m);
        }
    }
    let skeleton_file = mdl
        .table(2)?
        .and_then(|t| t.string(0).ok().flatten())
        .filter(|s| !s.is_empty());
    Ok(Model {
        path: path.to_string(),
        dir: dir.to_string(),
        materials,
        mesh_file,
        mesh_files: mesh_names.len(),
        skeleton_file,
        submeshes,
        min,
        max,
    })
}

pub fn parse_materials(buf: &[u8]) -> Result<HashMap<String, Material>> {
    let root = Table::root(buf, 0)?;
    let mut out = HashMap::new();
    for m in root.tables(1)? {
        let name = m.string(0)?.unwrap_or_default();
        let mut mat = Material {
            render_type: m.string(15)?.unwrap_or_default(),
            ..Default::default()
        };
        if let Some(sh) = m.tables(1)?.first() {
            mat.shader = sh.string(0)?.unwrap_or_default();
            for sw in sh.tables(1)? {
                if let (Some(k), Some(v)) = (sw.string(0)?, sw.string(1)?) {
                    mat.switches.insert(k, v);
                }
            }
        }
        for t in m.tables(2)? {
            if let (Some(role), Some(file)) = (t.string(0)?, t.string(1)?) {
                mat.textures.push((role, file));
            }
        }
        for p in m.tables(4)? {
            if let Some(pname) = p.string(0)? {
                mat.floats.insert(pname, p.f32(1, 0.0));
            }
        }
        for p in m.tables(7)? {
            let Some(pname) = p.string(0)? else { continue };
            let mut v = [0.0f32; 4];
            if let Some(at) = p.field(1) {
                for (k, slot) in v.iter_mut().enumerate() {
                    *slot = crate::fb::u32_at(buf, at + 4 * k)
                        .map(f32::from_bits)
                        .unwrap_or(0.0);
                }
            }
            mat.params.insert(pname, v);
        }
        out.insert(name, mat);
    }
    Ok(out)
}

fn load_shape(shape: &Table, buffer: &Table) -> Result<Vec<SubMesh>> {
    let shape_name = shape.string(0)?.unwrap_or_default();
    let accessors = shape.tables(3)?;
    let vbufs = buffer.tables(1)?;
    let mut positions: Option<Vec<[f32; 3]>> = None;
    let mut normals = None;
    let mut uvs = None;
    let mut colors: Option<Vec<[f32; 4]>> = None;
    let mut joints: Option<Vec<[u8; 4]>> = None;
    let mut weights: Option<Vec<[f32; 4]>> = None;

    for (ai, acc) in accessors.iter().enumerate() {
        let stride = acc.tables(1)?.first().map(|t| t.u32(0, 0)).unwrap_or(0) as usize;
        let data = match vbufs.get(ai) {
            Some(vb) => vb.bytes(0)?,
            None => continue,
        };
        if stride == 0 {
            continue;
        }
        let count = data.len() / stride;
        for attr in acc.tables(0)? {
            let (sem, layer, fmt, off) = (
                attr.u32(1, 0),
                attr.u32(2, 0),
                attr.u32(3, 0),
                attr.u32(4, 0) as usize,
            );
            match sem {
                POSITION if positions.is_none() => {
                    positions = Some(
                        read_vec(data, stride, off, count, fmt)?
                            .into_iter()
                            .map(|v| [v[0], v[1], v[2]])
                            .collect(),
                    );
                }
                NORMAL if normals.is_none() => {
                    normals = read_vec(data, stride, off, count, fmt)
                        .ok()
                        .map(|v| v.into_iter().map(|v| [v[0], v[1], v[2]]).collect());
                }
                TEXCOORD if layer == 0 && uvs.is_none() => {
                    uvs = read_vec(data, stride, off, count, fmt)
                        .ok()
                        .map(|v| v.into_iter().map(|v| [v[0], v[1]]).collect());
                }
                COLOR if layer == 0 && colors.is_none() => {
                    colors = read_vec(data, stride, off, count, fmt).ok();
                }
                // Joint indices: 4 x u8 (format 22), indices into the skeleton's joint list.
                BLEND_INDICES if layer == 0 && joints.is_none() && fmt == 22 => {
                    joints = (0..count)
                        .map(|v| {
                            data.get(v * stride + off..v * stride + off + 4)
                                .map(|b| [b[0], b[1], b[2], b[3]])
                        })
                        .collect::<Option<Vec<_>>>();
                }
                BLEND_WEIGHTS if layer == 0 && weights.is_none() => {
                    weights = read_vec(data, stride, off, count, fmt).ok();
                }
                _ => {}
            }
        }
    }
    let positions = positions.ok_or_else(|| anyhow!("no position attribute"))?;

    let ib = buffer
        .tables(0)?
        .first()
        .map(|t| t.bytes(0))
        .transpose()?
        .ok_or_else(|| anyhow!("no index buffer"))?;
    let all: Vec<u32> = match shape.u32(2, 0) {
        0 => ib.iter().map(|b| *b as u32).collect(),
        1 => ib
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]) as u32)
            .collect(),
        2 => ib
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        t => bail!("index type {t} not handled"),
    };
    if let Some(bad) = all.iter().find(|i| **i as usize >= positions.len()) {
        bail!("index {bad} is past the {} vertices", positions.len());
    }

    // One submesh per material range; each keeps the full vertex arrays (simple, and these meshes are small).
    let mut out = Vec::new();
    let mats = shape.tables(4)?;
    let ranges: Vec<(usize, usize, String)> = if mats.is_empty() {
        vec![(0, all.len(), String::new())]
    } else {
        mats.iter()
            .map(|m| {
                (
                    m.u32(1, 0) as usize,
                    m.u32(0, 0) as usize,
                    m.string(3).ok().flatten().unwrap_or_default(),
                )
            })
            .collect()
    };
    for (start, count, material) in ranges {
        let end = (start + count).min(all.len());
        if start >= end {
            continue;
        }
        let indices = all[start..end - (end - start) % 3].to_vec();
        out.push(SubMesh {
            shape: shape_name.clone(),
            material,
            positions: positions.clone(),
            normals: normals.clone(),
            uvs: uvs.clone(),
            colors: colors.clone(),
            joints: joints.clone(),
            weights: weights.clone(),
            indices,
        });
    }
    Ok(out)
}

/// Reads one attribute of `count` vertices as up to 4 floats each.
fn read_vec(
    data: &[u8],
    stride: usize,
    off: usize,
    count: usize,
    fmt: u32,
) -> Result<Vec<[f32; 4]>> {
    let mut out = Vec::with_capacity(count);
    for v in 0..count {
        let at = v * stride + off;
        let f32_at = |k: usize| -> f32 {
            f32::from_le_bytes(data[at + 4 * k..at + 4 * k + 4].try_into().unwrap())
        };
        let f16_at = |k: usize| -> f32 {
            f16_to_f32(u16::from_le_bytes([data[at + 2 * k], data[at + 2 * k + 1]]))
        };
        let need = match fmt {
            51 => 12,
            54 => 16,
            48 => 8,
            43 => 8,
            39 => 8,
            20 => 4,
            _ => bail!("vertex format {fmt} not handled"),
        };
        if at + need > data.len() {
            bail!("vertex data too short");
        }
        out.push(match fmt {
            51 => [f32_at(0), f32_at(1), f32_at(2), 1.0], // RGB 32 float
            54 => [f32_at(0), f32_at(1), f32_at(2), f32_at(3)], // RGBA 32 float
            48 => [f32_at(0), f32_at(1), 0.0, 0.0],       // RG 32 float
            43 => [f16_at(0), f16_at(1), f16_at(2), f16_at(3)], // RGBA 16 float
            39 => {
                // RGBA 16 unorm
                let u = |k: usize| {
                    u16::from_le_bytes([data[at + 2 * k], data[at + 2 * k + 1]]) as f32 / 65535.0
                };
                [u(0), u(1), u(2), u(3)]
            }
            _ => {
                // 20: RGBA 8 unorm
                let u = |k: usize| data[at + k] as f32 / 255.0;
                [u(0), u(1), u(2), u(3)]
            }
        });
    }
    Ok(out)
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1f) as u32;
    let man = (h & 0x3ff) as u32;
    let bits = if exp == 0 {
        if man == 0 {
            sign
        } else {
            // subnormal
            let mut e = 127 - 15 + 1;
            let mut m = man;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x3ff) << 13)
        }
    } else if exp == 31 {
        sign | 0x7f80_0000 | (man << 13)
    } else {
        sign | ((exp + 127 - 15) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}
