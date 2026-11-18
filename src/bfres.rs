//! Switch BFRES (version 0.9), as far as the effect primitives in `.ptcl` files need it: models, their shapes'
//! LOD 0 meshes and the vertex attributes. Offsets found on the user's files (2026-11-18); `probe --vfxb-prims`
//! checks them over every effect file.
//!
//! All pointers are u64 offsets from the BFRES start.
//!
//! File: "FRES    ", u32 0x08 version (0x00090100), 0x1C u32 file size, 0x28 models, 0x30 model dictionary
//! (u32 0x04 = count), 0xB0 buffer info (+0x04 u32 size, +0x08 u64 offset of the vertex / index data).
//!
//! Model (`FMDL`, 0x78 bytes, in an array): 0x08 name, 0x20 vertex buffers, 0x28 shapes, 0x68 u16 vertex buffer
//! count, 0x6A u16 shape count.
//! Vertex buffer (`FVTX`, 0x58 bytes): 0x08 attributes, 0x30 buffer sizes (0x10 each: u32 size), 0x38 strides
//! (0x10 each: u32 stride), 0x48 u32 offset of its buffers in the data, 0x4C u8 attribute count, 0x4D u8 buffer
//! count, 0x50 u32 vertex count. Buffers follow each other, each start aligned to 8.
//! Attribute (0x10 bytes): u64 name, u16 format (layout << 8 | type), u16 offset in the vertex, u8 buffer.
//! Names: `_p0` position, `_n0` normal, `_t0` tangent, `_c0` colour, `_u0` / `_u1` UVs.
//! Shape (`FSHP`, 0x60 bytes): 0x08 name, 0x10 its vertex buffer, 0x18 meshes (LODs), 0x5B u8 mesh count.
//! Mesh (0x38 bytes): 0x20 u32 index data offset, 0x24 u32 primitive type (3 triangles), 0x28 u32 index format
//! (0 u8, 1 u16, 2 u32), 0x2C u32 index count, 0x30 u32 first vertex.
//! Strings: u16 length, then the bytes.

use anyhow::{Result, bail};

fn u8_at(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}
fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2)
        .map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn ptr(b: &[u8], at: usize) -> usize {
    (u32_at(b, at) as u64 | (u32_at(b, at + 4) as u64) << 32).min(usize::MAX as u64) as usize
}
fn name(b: &[u8], at: usize) -> String {
    let p = ptr(b, at);
    let len = u16_at(b, p) as usize;
    b.get(p + 2..p + 2 + len)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .unwrap_or_default()
}

/// One attribute of a vertex buffer.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub format: u16,
}

/// One model of the file: its shapes' first LOD, merged.
#[derive(Clone, Debug, Default)]
pub struct Model {
    #[allow(dead_code)] // printed by the probe
    pub name: String,
    /// The attributes of the first vertex buffer, in file order (the `G3NT` table indexes them).
    pub attributes: Vec<Attribute>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// The components (up to 4) of attribute format `format` at `at`, and its size in bytes.
fn decode(b: &[u8], at: usize, format: u16) -> Result<[f32; 4]> {
    let unorm8 = |i: usize| u8_at(b, at + i) as f32 / 255.0;
    let snorm8 = |i: usize| (u8_at(b, at + i) as i8 as f32 / 127.0).max(-1.0);
    let unorm16 = |i: usize| u16_at(b, at + 2 * i) as f32 / 65535.0;
    let snorm16 = |i: usize| (u16_at(b, at + 2 * i) as i16 as f32 / 32767.0).max(-1.0);
    let half = |i: usize| crate::model::f16_to_f32(u16_at(b, at + 2 * i));
    let float = |i: usize| {
        let f = f32::from_bits(u32_at(b, at + 4 * i));
        if f.is_finite() { f } else { 0.0 }
    };
    if b.len() < at + 4 {
        bail!("vertex data runs past the end");
    }
    Ok(match format {
        0x0201 => [unorm8(0), 0.0, 0.0, 1.0],
        0x0901 => [unorm8(0), unorm8(1), 0.0, 1.0],
        0x0902 => [snorm8(0), snorm8(1), 0.0, 1.0],
        0x0B01 => [unorm8(0), unorm8(1), unorm8(2), unorm8(3)],
        0x0B02 => [snorm8(0), snorm8(1), snorm8(2), snorm8(3)],
        0x0E02 => {
            let v = u32_at(b, at);
            let c = |s: u32| {
                let x = ((v >> s) & 0x3FF) as i32;
                let x = if x >= 512 { x - 1024 } else { x };
                (x as f32 / 511.0).max(-1.0)
            };
            [c(0), c(10), c(20), 1.0]
        }
        0x1201 => [unorm16(0), unorm16(1), 0.0, 1.0],
        0x1202 => [snorm16(0), snorm16(1), 0.0, 1.0],
        0x1205 => [half(0), half(1), 0.0, 1.0],
        0x1501 => [unorm16(0), unorm16(1), unorm16(2), unorm16(3)],
        0x1502 => [snorm16(0), snorm16(1), snorm16(2), snorm16(3)],
        0x1505 => {
            if b.len() < at + 8 {
                bail!("vertex data runs past the end");
            }
            [half(0), half(1), half(2), half(3)]
        }
        0x1705 | 0x1805 => {
            let n = if format == 0x1705 { 2 } else { 3 };
            if b.len() < at + 4 * n {
                bail!("vertex data runs past the end");
            }
            [float(0), float(1), if n == 3 { float(2) } else { 0.0 }, 1.0]
        }
        _ => bail!("vertex format {format:#06x} not known"),
    })
}

pub fn parse(b: &[u8]) -> Result<Vec<Model>> {
    if !b.starts_with(b"FRES    ") {
        bail!("not a BFRES");
    }
    let version = u32_at(b, 8);
    if version >> 16 != 9 {
        bail!("BFRES version {version:#x}; only 0.9 is known");
    }
    let data = ptr(b, ptr(b, 0xB0) + 8);
    let count = u32_at(b, ptr(b, 0x30) + 4) as usize;
    let models_at = ptr(b, 0x28);
    let mut out = Vec::new();
    for m in 0..count.min(256) {
        let at = models_at + 0x78 * m;
        if b.get(at..at + 4) != Some(b"FMDL") {
            bail!("model {m} at {at:#x} is not an FMDL");
        }
        let mut model = Model {
            name: name(b, at + 8),
            ..Default::default()
        };
        let shapes_at = ptr(b, at + 0x28);
        let shapes = u16_at(b, at + 0x6A) as usize;
        for s in 0..shapes.min(256) {
            let sh = shapes_at + 0x60 * s;
            if b.get(sh..sh + 4) != Some(b"FSHP") {
                bail!("shape {s} of model {m} is not an FSHP");
            }
            let vb = ptr(b, sh + 0x10);
            if b.get(vb..vb + 4) != Some(b"FVTX") {
                bail!("shape {s} of model {m}: vertex buffer is not an FVTX");
            }
            if u8_at(b, sh + 0x5B) == 0 {
                continue;
            }
            let base = model.positions.len() as u32;
            read_vertices(b, data, vb, &mut model, s == 0)?;
            let mesh = ptr(b, sh + 0x18);
            let kind = u32_at(b, mesh + 0x24);
            if kind != 3 {
                bail!("shape {s} of model {m}: primitive type {kind} (only triangles are known)");
            }
            let first = u32_at(b, mesh + 0x30);
            let n = u32_at(b, mesh + 0x2C) as usize;
            let ia = data + u32_at(b, mesh + 0x20) as usize;
            let size = match u32_at(b, mesh + 0x28) {
                0 => 1,
                1 => 2,
                2 => 4,
                f => bail!("index format {f} not known"),
            };
            if ia + n * size > b.len() {
                bail!("shape {s} of model {m}: indices run past the end");
            }
            let verts = model.positions.len() as u32 - base;
            for i in 0..n {
                let v = match size {
                    1 => u8_at(b, ia + i) as u32,
                    2 => u16_at(b, ia + 2 * i) as u32,
                    _ => u32_at(b, ia + 4 * i),
                } + first;
                if v >= verts {
                    bail!("shape {s} of model {m}: index {v} past {verts} vertices");
                }
                model.indices.push(base + v);
            }
        }
        out.push(model);
    }
    Ok(out)
}

/// Appends the vertices of vertex buffer `vb` to `model` (missing attributes get defaults).
fn read_vertices(b: &[u8], data: usize, vb: usize, model: &mut Model, first: bool) -> Result<()> {
    let attrs_at = ptr(b, vb + 0x08);
    let sizes = ptr(b, vb + 0x30);
    let strides = ptr(b, vb + 0x38);
    let start = data + u32_at(b, vb + 0x48) as usize;
    let attr_count = u8_at(b, vb + 0x4C) as usize;
    let buffers = u8_at(b, vb + 0x4D) as usize;
    let verts = u32_at(b, vb + 0x50) as usize;
    let mut offsets = Vec::new();
    let mut o = start;
    for i in 0..buffers {
        offsets.push((o, u32_at(b, strides + 0x10 * i) as usize));
        o += (u32_at(b, sizes + 0x10 * i) as usize).next_multiple_of(8);
    }
    if o > b.len() + 8 {
        bail!("vertex buffers run past the end");
    }
    let n0 = model.positions.len();
    model.positions.resize(n0 + verts, [0.0; 3]);
    model.normals.resize(n0 + verts, [0.0, 1.0, 0.0]);
    model.uvs.resize(n0 + verts, [0.0; 2]);
    model.colors.resize(n0 + verts, [1.0; 4]);
    for a in 0..attr_count {
        let at = attrs_at + 0x10 * a;
        let attr = Attribute {
            name: name(b, at),
            format: u16_at(b, at + 8),
        };
        let offset = u16_at(b, at + 0x0C) as usize;
        let Some(&(buf, stride)) = offsets.get(u8_at(b, at + 0x0E) as usize) else {
            bail!("attribute {} names buffer past {buffers}", attr.name);
        };
        for i in 0..verts {
            let v = decode(b, buf + stride * i + offset, attr.format)?;
            let k = n0 + i;
            match attr.name.as_str() {
                "_p0" => model.positions[k] = [v[0], v[1], v[2]],
                "_n0" => model.normals[k] = [v[0], v[1], v[2]],
                "_u0" => model.uvs[k] = [v[0], v[1]],
                "_c0" => model.colors[k] = v,
                _ => {}
            }
        }
        if first {
            model.attributes.push(attr);
        }
    }
    Ok(())
}
