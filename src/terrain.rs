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
//!   Slot 13 is a bit mask per cell: bit k = grass model k of slot 12 grows there (`probe --grass-cells 06_03`:
//!   the masks sit on the green layers, e.g. bits 5/6 on layer 3, bits 3/4 on layer 0).
//!   tile header `.trtrr` { 0: {3: 0.7}, 1: {0: 500, 1: 500, 2: 1000, 3: 32, 4: 1024}, 2: {0: 1024, 1: 512, 2: 513},
//!                 3: 10, 4: [{0: model path relative to the tile folder ("../../terrain_model/<m>/<m>.trmdl"),
//!                 1: [string] its instance files in the tile folder ("ter_XX_YY_<m>_X0Z0_d0.trins")}] (trees;
//!                 see `instances.rs`), 5: [u8] 13312, 6: [{0: grass model path, 1..18: floats (scales, 15, 3,
//!                 a colour?, 0.9 / 1.1 = random scale range?, 10.5), 19: u32}] (grass) }
//!   Height in metres = sample * scale.y; position = origin + (column * scale.x, height, row * scale.z).
//!   Tiles are 500 m with 513 x 513 samples (cell 500/512 m); tile XX_YY has origin (XX*500, 0, -YY*500 - 500).
//!   Samples of 0 are holes. `ter_XX_YY_heightmap.bntx` (R16 unorm, format 0x0a01, 513²) holds the same samples:
//!   height = 1000 * value, within 0.011 m on every non-hole sample, holes 0 in both (`probe --heightmap`).
//! Checked with `probe --terrain`: all 100 tiles of the 10 x 10 grid parse (see the module's `TILES`).
//!
//! Holes and the "skirts" around them (`probe --hole-regions`, all tiles stitched, holes flood-filled from the
//! map's border): the sea (10.3M samples; 94% of its edge samples are under 1 m, mean 1.6 / 5.1 / 12.2 / 21.5 m
//! at 0 / 2 / 4 / 8 samples in; the sea surface is at 27 m (`water.rs`), so this is sea bed below it), **Area Zero's crater** (one region of
//! 1.04M samples, ~1 km², around terrain_04_04; part of its rim is cut at ~200 m, 64% of its edge falls to ~0:
//! walls going deeper than the unorm range), and 26 small pits (two over 100 samples, in 08_08). **Towns are
//! not holes**: their models sit on the ground. The steep strips at coasts and the crater are therefore real
//! samples the game renders too (sea cliffs, crater walls), not an artefact; the viewer keeps them.

use crate::archive::Archive;
use crate::fb::Table;
use anyhow::{Result, anyhow, bail};

/// The models a tile places from its own instance files (trtrr slot 4): (model path, instance file paths).
pub fn tile_instances(ar: &mut Archive, trtrr_path: &str) -> Result<Vec<(String, Vec<String>)>> {
    let dir = trtrr_path.rsplit_once('/').map_or("", |(d, _)| d);
    let buf = ar.read(trtrr_path)?;
    let root = Table::root(&buf, 0)?;
    let mut out = Vec::new();
    for e in root.tables(4)? {
        let Some(model) = e.string(0)? else { continue };
        let files = e
            .strings(1)?
            .iter()
            .map(|f| crate::model::resolve(dir, f))
            .collect();
        out.push((crate::model::resolve(dir, &model), files));
    }
    Ok(out)
}

/// Paldea's grid: tiles terrain_00_00 .. terrain_09_09 (the probe's Paldea checks; the viewer uses `maps.rs`).
#[allow(dead_code)]
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

    /// Ground height at world x/z, bilinear between the four samples around it. `None` outside the tile or
    /// when one of them is a hole (no ground there to stand on).
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        let c = (x - self.origin[0]) / self.scale[0];
        let r = (z - self.origin[2]) / self.scale[2];
        if !(c >= 0.0 && r >= 0.0 && c <= (self.width - 1) as f32 && r <= (self.depth - 1) as f32) {
            return None;
        }
        let (c0, r0) = (c.floor() as usize, r.floor() as usize);
        let (c1, r1) = ((c0 + 1).min(self.width - 1), (r0 + 1).min(self.depth - 1));
        let (fc, fr) = (c - c0 as f32, r - r0 as f32);
        let h = [
            self.height(c0, r0),
            self.height(c1, r0),
            self.height(c0, r1),
            self.height(c1, r1),
        ];
        if h.contains(&0.0) {
            return None;
        }
        let top = h[0] + (h[1] - h[0]) * fc;
        let bottom = h[2] + (h[3] - h[2]) * fc;
        Some(self.origin[1] + top + (bottom - top) * fr)
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
#[allow(dead_code)] // the probe's Paldea checks
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
    /// edge texels. The names do not always describe the picture: layers 2 and 4 (`grass_deep01`/`03`) are orange
    /// gravel (mean sRGB 202,98,35), the ground of the orange east (terrain_06_03 is mostly them).
    pub splat_alpha: String,
    /// BC7 sRGB 256 x 256 x 60 layers, 9 mips: the ground textures (`terrain_splat_tex/global_color_layer_array`).
    /// Saturated as stored (grass01 mean sRGB 45,123,59); the game softens it with grass models on top.
    pub splat_color: String,
    /// RGBA8 (not sRGB) 256 x 256 x 60 layers: the layers' tangent-space normal maps, RGB = xyz (mean x/y 127.5,
    /// z near 255), A = 255 on cliff layers and ~0 on grass and desert (a height or mask; not used).
    pub splat_normal: String,
    /// RGBA32F 256 x 2 (`probe --terrain-materials`): row 0 x/y = a layer's tiling size in metres (5, layer 59:
    /// 6.25; entries 60..255 are 1). Row 1: x = 2 on the sand and snow layers (16-18 desert, 30 snow02, 31 sandy
    /// beach), else 0: not triplanar, which every tile material switches off (`EnableArrayTriplanarLayer`
    /// False, `TriplanarLayerCount` 0, 100 of 100); maybe footprints or sparkle. y = 2 / 1 on the grass layers
    /// (0/1, 5/6, 9/10, 11/12: the `_01` / `_02` of each pair). z = layer / 255. w = 0.7 on the cliff layers 35-43,
    /// snow01 and mud02, 0.9 on mud01, else 1 (roughness or specular?).
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
        splat_normal: tex("SplatNormal")?,
        splat_param: tex("SplatParam")?,
    })
}

/// The ground textures shared by every tile, decoded: one colour and one normal image per layer and its tiling
/// size in metres. `normals` is empty if the normal array failed to load (the ground is then drawn flat).
pub struct SplatLayers {
    pub colors: Vec<crate::texture::Image>,
    pub normals: Vec<crate::texture::Image>,
    pub tiling: Vec<f32>,
}

pub fn load_layers(ar: &mut Archive, m: &TileMaterial) -> Result<SplatLayers> {
    let b = ar.read(&m.splat_color)?;
    let n = crate::texture::layer_count(&b)?;
    let colors = (0..n)
        .map(|k| crate::texture::decode_bntx_layer(&b, k))
        .collect::<Result<Vec<_>>>()?;
    let normals = ar
        .read(&m.splat_normal)
        .and_then(|b| {
            (0..crate::texture::layer_count(&b)?.min(n))
                .map(|k| crate::texture::decode_bntx_layer(&b, k))
                .collect::<Result<Vec<_>>>()
        })
        .unwrap_or_default();
    let (w, _, params) = crate::texture::decode_rgba32f(&ar.read(&m.splat_param)?)?;
    let tiling = (0..n as usize)
        .map(|k| {
            let t = if k < w as usize { params[k][0] } else { 0.0 };
            if t.is_finite() && t > 0.01 { t } else { 5.0 }
        })
        .collect();
    Ok(SplatLayers {
        colors,
        normals,
        tiling,
    })
}
