//! BNTX (Nintendo Switch texture container) to RGBA8. Only the first texture and mip level are read; any array
//! layer can be (`decode_bntx_layer`).
//!
//! BNTX header: "BNTX" ... at 0x20 the "NX  " section: u32 texture count at 0x24, u64 offset of the BRTI pointer
//! array at 0x28. BRTI (texture info), offsets from its start:
//!   0x16 u16 mip count   0x1C u32 format (high byte type, low byte 1 unorm / 6 sRGB / ...)
//!   0x24 u32 width       0x28 u32 height    0x30 u32 array layers  0x34 u32 log2 of the block height in GOBs
//!   0x50 u32 size of all layers' data (every layer takes the same share, mips included)
//!   0x70 u64 offset of the array of u64 mip-level data offsets
//! Data is in the Tegra X1 "block linear" layout: 64-byte x 8-row GOBs stacked into blocks of 2^n GOBs.

use crate::fb::{u16_at, u32_at, u64_at};
use anyhow::{Result, anyhow, bail};

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<[u8; 4]>,
    pub format: String,
}

impl Image {
    pub fn has_alpha(&self) -> bool {
        self.rgba.iter().any(|p| p[3] < 250)
    }
}

enum Codec {
    Raw,    // uncompressed, bpp bytes per pixel
    Bc(u8), // BC1..BC7
    Astc(usize, usize),
}

pub fn decode_bntx(b: &[u8]) -> Result<Image> {
    decode_bntx_layer(b, 0)
}

/// Offset of the BRTI block of the first texture.
fn brti_at(b: &[u8]) -> Result<usize> {
    if !b.starts_with(b"BNTX") {
        bail!("not a BNTX file");
    }
    let ptrs = u64_at(b, 0x28)? as usize;
    let brti = u64_at(b, ptrs)? as usize;
    if b.get(brti..brti + 4) != Some(b"BRTI") {
        bail!("BRTI block not found");
    }
    Ok(brti)
}

/// Number of array layers of the first texture (1 for a plain 2D texture).
pub fn layer_count(b: &[u8]) -> Result<u32> {
    Ok(u32_at(b, brti_at(b)? + 0x30)?.max(1))
}

/// Decodes array layer `layer` of the first texture, first mip level.
pub fn decode_bntx_layer(b: &[u8], layer: u32) -> Result<Image> {
    if !b.starts_with(b"BNTX") {
        bail!("not a BNTX file");
    }
    let ptrs = u64_at(b, 0x28)? as usize;
    let brti = u64_at(b, ptrs)? as usize;
    if b.get(brti..brti + 4) != Some(b"BRTI") {
        bail!("BRTI block not found");
    }
    let format = u32_at(b, brti + 0x1C)?;
    let width = u32_at(b, brti + 0x24)? as usize;
    let height = u32_at(b, brti + 0x28)? as usize;
    let blh_log2 = u32_at(b, brti + 0x34)? & 7;
    let _mips = u16_at(b, brti + 0x16)?;
    let mip_ptrs = u64_at(b, brti + 0x70)? as usize;
    let layers = u32_at(b, brti + 0x30)?.max(1);
    if layer >= layers {
        bail!("layer {layer} of {layers}");
    }
    let stride = u32_at(b, brti + 0x50)? as usize / layers as usize;
    let data_off = u64_at(b, mip_ptrs)? as usize + layer as usize * stride;
    let data = b
        .get(data_off..)
        .ok_or_else(|| anyhow!("texture data offset past the end"))?;
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        bail!("bad size {width}x{height}");
    }

    let ty = format >> 8;
    let (codec, bw, bh, bpp, name) = match ty {
        0x02 => (Codec::Raw, 1, 1, 1, "R8"),
        0x09 => (Codec::Raw, 1, 1, 2, "R8G8"),
        0x0b => (Codec::Raw, 1, 1, 4, "R8G8B8A8"),
        0x0c => (Codec::Raw, 1, 1, 4, "B8G8R8A8"),
        0x1a => (Codec::Bc(1), 4, 4, 8, "BC1"),
        0x1b => (Codec::Bc(2), 4, 4, 16, "BC2"),
        0x1c => (Codec::Bc(3), 4, 4, 16, "BC3"),
        0x1d => (Codec::Bc(4), 4, 4, 8, "BC4"),
        0x1e => (Codec::Bc(5), 4, 4, 16, "BC5"),
        0x1f => (Codec::Bc(6), 4, 4, 16, "BC6H"),
        0x20 => (Codec::Bc(7), 4, 4, 16, "BC7"),
        0x2d..=0x3a => {
            const ASTC: [(usize, usize); 14] = [
                (4, 4),
                (5, 4),
                (5, 5),
                (6, 5),
                (6, 6),
                (8, 5),
                (8, 6),
                (8, 8),
                (10, 5),
                (10, 6),
                (10, 8),
                (10, 10),
                (12, 10),
                (12, 12),
            ];
            let (w, h) = ASTC[(ty - 0x2d) as usize];
            (Codec::Astc(w, h), w, h, 16, "ASTC")
        }
        _ => bail!("texture format {format:#06x} not handled"),
    };
    let srgb = format & 0xff == 6;
    let fmt_name = match codec {
        Codec::Astc(w, h) => format!("ASTC {w}x{h}{}", if srgb { " sRGB" } else { "" }),
        _ => format!("{name}{}", if srgb { " sRGB" } else { "" }),
    };

    let (wb, hb) = (width.div_ceil(bw), height.div_ceil(bh));
    let linear = deswizzle(data, wb, hb, bpp, blh_log2);

    let mut px = vec![0u32; wb * bw * hb * bh];
    let (pw, ph) = (wb * bw, hb * bh);
    let err = |e: &str| anyhow!("{fmt_name}: {e}");
    let rgba: Vec<[u8; 4]> = match codec {
        Codec::Raw => linear
            .chunks_exact(bpp)
            .map(|c| match (ty, bpp) {
                (0x02, _) => [c[0], c[0], c[0], 255],
                (0x09, _) => [c[0], c[1], 0, 255],
                (0x0c, _) => [c[2], c[1], c[0], c[3]],
                _ => [c[0], c[1], c[2], c[3]],
            })
            .collect(),
        Codec::Bc(n) => {
            use texture2ddecoder as t;
            match n {
                1 => t::decode_bc1(&linear, pw, ph, &mut px),
                2 => t::decode_bc2(&linear, pw, ph, &mut px),
                3 => t::decode_bc3(&linear, pw, ph, &mut px),
                4 => t::decode_bc4(&linear, pw, ph, &mut px),
                5 => t::decode_bc5(&linear, pw, ph, &mut px),
                6 => t::decode_bc6(&linear, pw, ph, &mut px, false),
                _ => t::decode_bc7(&linear, pw, ph, &mut px),
            }
            .map_err(err)?;
            px.iter().map(|p| bgra(*p)).collect()
        }
        Codec::Astc(w, h) => {
            texture2ddecoder::decode_astc(&linear, pw, ph, w, h, &mut px).map_err(err)?;
            px.iter().map(|p| bgra(*p)).collect()
        }
    };

    // Crop the block padding (block-compressed images are decoded in whole blocks).
    let full_w = if matches!(codec, Codec::Raw) {
        width
    } else {
        pw
    };
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        out.extend_from_slice(&rgba[y * full_w..y * full_w + width]);
    }
    Ok(Image {
        width: width as u32,
        height: height as u32,
        rgba: out,
        format: fmt_name,
    })
}

fn bgra(p: u32) -> [u8; 4] {
    let [b, g, r, a] = p.to_le_bytes();
    [r, g, b, a]
}

/// Tegra X1 block-linear to linear, in units of `bpp`-byte elements (pixels or compressed blocks).
fn deswizzle(data: &[u8], w: usize, h: usize, bpp: usize, block_height_log2: u32) -> Vec<u8> {
    let gobs = 1usize << block_height_log2; // GOBs per block, stacked vertically
    let width_gobs = (w * bpp).div_ceil(64);
    let mut out = vec![0u8; w * h * bpp];
    for y in 0..h {
        for x in 0..w {
            let xb = x * bpp;
            let gob = (y / (8 * gobs)) * 512 * gobs * width_gobs
                + (xb / 64) * 512 * gobs
                + (y % (8 * gobs) / 8) * 512;
            let addr = gob
                + ((xb % 64) / 32) * 256
                + ((y % 8) / 2) * 64
                + ((xb % 32) / 16) * 32
                + (y % 2) * 16
                + (xb % 16);
            let dst = (y * w + x) * bpp;
            if let Some(src) = data.get(addr..addr + bpp) {
                out[dst..dst + bpp].copy_from_slice(src);
            }
        }
    }
    out
}

/// The first texture as 32-bit float RGBA (format type 0x19; e.g. the terrain's `global_splatST_params.bntx`,
/// 256 x 2, whose values are plausible only read this way). Returns width, height and row-major texels.
pub fn decode_rgba32f(b: &[u8]) -> Result<(u32, u32, Vec<[f32; 4]>)> {
    let brti = brti_at(b)?;
    let format = u32_at(b, brti + 0x1C)?;
    if format >> 8 != 0x19 {
        bail!("texture format {format:#06x} is not RGBA32F");
    }
    let width = u32_at(b, brti + 0x24)? as usize;
    let height = u32_at(b, brti + 0x28)? as usize;
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        bail!("bad size {width}x{height}");
    }
    let blh_log2 = u32_at(b, brti + 0x34)? & 7;
    let mip_ptrs = u64_at(b, brti + 0x70)? as usize;
    let data_off = u64_at(b, mip_ptrs)? as usize;
    let data = b
        .get(data_off..)
        .ok_or_else(|| anyhow!("texture data offset past the end"))?;
    let linear = deswizzle(data, width, height, 16, blh_log2);
    let texels = linear
        .as_chunks::<16>()
        .0
        .iter()
        .map(|c| {
            std::array::from_fn(|k| {
                f32::from_le_bytes([c[4 * k], c[4 * k + 1], c[4 * k + 2], c[4 * k + 3]])
            })
        })
        .collect();
    Ok((width as u32, height as u32, texels))
}
