//! Overworld terrain: the heightfield of each tile of field_graphic/terrain/terrain_XX_YY/.
//!
//! A tile's pack holds `ter_XX_YY.trtrr` (a small header: material, detail map, grass), `ter_XX_YY.trmtr`,
//! `ter_XX_YY.trcol` (Havok collision), `ter_XX_YY_heightmap.bntx`, `.trtbf` detail maps, more `.bntx` and one
//! FlatBuffer whose name is not known yet, which holds the heights. It is found by its shape (slots 3/4 = grid
//! size, slot 5 a vector of that many floats). Field slots, checked against the game's files:
//!   height file { 0: origin {0: x, 2: z} (min corner; y absent = 0), 1: scale {0: cell x, 1: height, 2: cell z},
//!                 2: size {0: x, 1: height range, 2: z}, 3: samples x, 4: samples z, 5: [f32] heights 0..1,
//!                 row-major, 6/7: 256 x 256 (a coarser grid; 8: [{1: [u32] 65536}]), 10/11: 512 x 512 cells,
//!                 12: [string] grass model names, 13: [u32] 512 x 512 per-cell values (grass?),
//!                 15: [string] splat material names (`splat000_ground_grass01`, ...), 17: "ter_XX_YY.trcol" }
//!   Height in metres = sample * scale.y; position = origin + (column * scale.x, height, row * scale.z).
//!   Tiles are 500 m with 513 x 513 samples (cell 500/512 m); tile XX_YY has origin (XX*500, 0, -YY*500 - 500).
//!   Samples of 0 are holes (towns and other places with their own models, e.g. most of terrain_04_04).
//! Checked with `probe --terrain`: all 100 tiles of the 10 x 10 grid parse (see the module's `TILES`).

use crate::archive::Archive;
use crate::fb::Table;
use anyhow::{Result, anyhow, bail};

/// The overworld grid: tiles terrain_00_00 .. terrain_09_09.
pub const TILES: i32 = 10;

pub struct HeightTile {
    pub origin: [f32; 3],
    /// Cell size along x and z, and the factor that turns a sample into metres.
    pub scale: [f32; 3],
    /// Samples along x (columns) and z (rows).
    pub width: usize,
    pub depth: usize,
    /// Heights in metres, row-major (`row * width + column`).
    pub heights: Vec<f32>,
}

impl HeightTile {
    pub fn height(&self, column: usize, row: usize) -> f32 {
        self.heights
            .get(row * self.width + column)
            .copied()
            .unwrap_or(0.0)
    }

    /// World position of a sample.
    pub fn position(&self, column: usize, row: usize) -> [f32; 3] {
        [
            self.origin[0] + column as f32 * self.scale[0],
            self.origin[1] + self.height(column, row),
            self.origin[2] + row as f32 * self.scale[2],
        ]
    }
}

pub fn tile_path(x: i32, y: i32) -> String {
    format!("field_graphic/terrain/terrain_{x:02}_{y:02}/ter_{x:02}_{y:02}.trtrr")
}

/// The grid tiles present in the archive, as (x, y, path to the tile's .trtrr).
pub fn tiles(ar: &Archive) -> Vec<(i32, i32, String)> {
    let mut out = Vec::new();
    for x in 0..TILES {
        for y in 0..TILES {
            let p = tile_path(x, y);
            if ar.contains(&p) {
                out.push((x, y, p));
            }
        }
    }
    out
}

/// Reads one float3-like table (fields 0..2, absent = 0).
fn vec3(t: &Table, slot: usize) -> Result<[f32; 3]> {
    let v = t
        .table(slot)?
        .ok_or_else(|| anyhow!("slot {slot} is missing"))?;
    Ok([v.f32(0, 0.0), v.f32(1, 0.0), v.f32(2, 0.0)])
}

/// Parses `buf` as a height file, or says why it is not one.
pub fn parse_heights(buf: &[u8]) -> Result<HeightTile> {
    if buf.len() < 64 || buf.starts_with(b"BNTX") {
        bail!("not a FlatBuffer");
    }
    let root = Table::root(buf, 0)?;
    if root.slot_count() < 6 {
        bail!("too few slots");
    }
    let (width, depth) = (root.u32(3, 0) as usize, root.u32(4, 0) as usize);
    if !(2..=4097).contains(&width) || !(2..=4097).contains(&depth) {
        bail!("grid size {width} x {depth} is not plausible");
    }
    let (start, n) = root.vector(5)?;
    if n != width * depth {
        bail!("{n} heights for a {width} x {depth} grid");
    }
    let raw = crate::fb::get(buf, start, n * 4)?;
    let origin = vec3(&root, 0)?;
    let scale = vec3(&root, 1)?;
    if scale.iter().any(|s| !s.is_finite() || *s <= 0.0) {
        bail!("bad scale {scale:?}");
    }
    let heights = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c) * scale[1])
        .map(|h| if h.is_finite() { h } else { 0.0 })
        .collect();
    Ok(HeightTile {
        origin,
        scale,
        width,
        depth,
        heights,
    })
}

/// Loads the heightfield of the tile whose header is `trtrr_path`, by trying each unnamed file in its pack.
pub fn load_heights(ar: &mut Archive, trtrr_path: &str) -> Result<HeightTile> {
    let header = crate::archive::fnv1a64(trtrr_path);
    for h in ar.pack_files(trtrr_path)? {
        if h == header {
            continue;
        }
        let Ok(buf) = ar.read_hash(h) else { continue };
        if let Ok(tile) = parse_heights(&buf) {
            return Ok(tile);
        }
    }
    bail!("{trtrr_path}: no height file in its pack")
}

/// The textures a tile's material (`ter_XX_YY.trmtr`, material "TerrainMaterial", shader "Terrain") names, as
/// archive paths. Roles: HeightMap (R16, 513²), HeightNormalMap (R10G10B10A2, 513²), SplatAlpha, SplatColor,
/// SplatNormal (RGBA8 256² x 60 layers, the layers' normal maps; not used yet), SplatParam. Float params:
/// RoughnessLayerN, SplatSTNThreshold; float4: SplatSTNFar (N = 0..5). The pack also holds `_mid`/`_low` copies of
/// the height, normal and splat maps (257²/129², 256²/128²), a second material with 11 layers (LOD?) and a few
/// `SrtInstanceBuffer` FlatBuffers (instances; not read).
pub struct TileMaterial {
    /// 512 x 512 RGBA8 control map, one texel per height cell (texel x = column, y = row): R and G are two
    /// layers of the colour array, B and A their weights (B + A = 255). Checked on terrain_02_05: its nine
    /// distinct indices match the nine splat names of its height file in order, and cliff layers sit on slopes
    /// four times steeper than the rest (transposed or flipped, they do not). `probe --splat`: the indices are
    /// global over all 100 tiles (layer k = the k-th splat name in sorted order, e.g. 42 = splat062, 59 =
    /// splat102, the sea fill). `probe --splat-seams`: dominant layers agree on 98.8% / 98.4% of shared x / z
    /// edge texels.
    pub splat_alpha: String,
    /// BC7 sRGB 256 x 256 x 60 layers, 9 mips: the ground textures (`terrain_splat_tex/global_color_layer_array`).
    pub splat_color: String,
    /// RGBA32F 256 x 2: row 0 x/y = a layer's tiling size in metres (5, layer 59: 6.25); row 1 = per-layer
    /// flags (x 2 on some desert and cliff layers), z = layer / 255, w = 0.7..1 (roughness?).
    pub splat_param: String,
}

pub fn tile_material(ar: &mut Archive, trtrr_path: &str) -> Result<TileMaterial> {
    let dir = trtrr_path.rsplit_once('/').map_or("", |(d, _)| d);
    let stem = trtrr_path
        .rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".trtrr"))
        .unwrap_or_default();
    let mats = crate::model::parse_materials(&ar.read(&format!("{dir}/{stem}.trmtr"))?)?;
    let m = mats
        .values()
        .find(|m| m.texture("SplatAlpha").is_some())
        .ok_or_else(|| anyhow!("{stem}.trmtr has no SplatAlpha texture"))?;
    let tex = |role: &str| -> Result<String> {
        m.texture(role)
            .map(|f| crate::model::resolve(dir, f))
            .ok_or_else(|| anyhow!("{stem}.trmtr has no {role} texture"))
    };
    Ok(TileMaterial {
        splat_alpha: tex("SplatAlpha")?,
        splat_color: tex("SplatColor")?,
        splat_param: tex("SplatParam")?,
    })
}

/// The ground textures shared by every tile, decoded: one image per layer and its tiling size in metres.
pub struct SplatLayers {
    pub colors: Vec<crate::texture::Image>,
    pub tiling: Vec<f32>,
}

pub fn load_layers(ar: &mut Archive, m: &TileMaterial) -> Result<SplatLayers> {
    let b = ar.read(&m.splat_color)?;
    let n = crate::texture::layer_count(&b)?;
    let colors = (0..n)
        .map(|k| crate::texture::decode_bntx_layer(&b, k))
        .collect::<Result<Vec<_>>>()?;
    let (w, _, params) = crate::texture::decode_rgba32f(&ar.read(&m.splat_param)?)?;
    let tiling = (0..n as usize)
        .map(|k| {
            let t = if k < w as usize { params[k][0] } else { 0.0 };
            if t.is_finite() && t > 0.01 { t } else { 5.0 }
        })
        .collect();
    Ok(SplatLayers { colors, tiling })
}
