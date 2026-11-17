//! Instance buffers (`.trins`): many copies of one model, each with its own transform. The file names its kind
//! but not its model; the model comes from whoever lists the file (a terrain tile's `.trtrr`, see `terrain.rs`).
//!
//! Field slots, checked against the game's files:
//!   SrtInstanceBuffer { 0: "SrtInstanceBuffer", 1: 1 (version?), 2: instance count, 4: { 0: [u8] count * 64 } }
//!   Each instance is a column-major 4 x 4 f32 matrix (16 floats): columns 0-2 = the rotated, scaled axes,
//!   column 3 = translation in world metres (same frame as the terrain tiles), last row 0, 0, 0, 1.
//! Checked over the whole dump:
//!   - `probe --tile-instances`: the 100 tiles' `.trtrr` lists name 311 tree files, all parse: 3509 instances of
//!     49 models (all load), last row 0,0,0,1 in every matrix, 3478 of them within 0.5 m of the ground under them.
//!   - `probe --instances <pack>`: the pack of `world/scene/parts/field/unity_output/town_t07_field_streaming_s_/
//!     ins_root_..._X00_Z00.trins` holds 2405 instance buffers (all parse, 25000 instances over the whole island,
//!     7101 within 0.5 m of the ground, 4219 over no ground). Names follow `town_<code>_field_streaming_s_/
//!     ins_root_<model stem>_<NNN>_X00_Z00.trins` (NNN 000..053, a per-folder index); `probe --guess-ins` names
//!     396 of them in 12 town folders (t01-t09, c02, c03_01-03): pots, lamps, garden plants, cars. The rest
//!     belong to folders not found yet.

use crate::fb::Table;
use anyhow::{Result, bail};

pub struct Instances {
    /// One column-major matrix per instance.
    pub matrices: Vec<[f32; 16]>,
}

#[allow(dead_code)] // used by the probe
impl Instances {
    pub fn position(m: &[f32; 16]) -> [f32; 3] {
        [m[12], m[13], m[14]]
    }

    /// Length of each axis column: the instance's scale along x, y, z.
    pub fn scale(m: &[f32; 16]) -> [f32; 3] {
        std::array::from_fn(|c| {
            (m[4 * c] * m[4 * c] + m[4 * c + 1] * m[4 * c + 1] + m[4 * c + 2] * m[4 * c + 2]).sqrt()
        })
    }
}

/// Parses an `SrtInstanceBuffer`, or says why `buf` is not one.
pub fn parse(buf: &[u8]) -> Result<Instances> {
    if buf.len() < 32 || buf.starts_with(b"BNTX") {
        bail!("not a FlatBuffer");
    }
    let root = Table::root(buf, 0)?;
    if root.string(0)?.as_deref() != Some("SrtInstanceBuffer") {
        bail!("not an SrtInstanceBuffer");
    }
    let count = root.u32(2, 0) as usize;
    let Some(data) = root.table(4)? else {
        bail!("no instance data (slot 4)");
    };
    let (start, n) = data.vector(0)?;
    if n != count * 64 {
        bail!("{n} bytes of instance data for {count} instances");
    }
    let raw = crate::fb::get(buf, start, n)?;
    let matrices: Vec<[f32; 16]> = raw
        .as_chunks::<64>()
        .0
        .iter()
        .map(|m| {
            std::array::from_fn(|k| {
                f32::from_le_bytes([m[4 * k], m[4 * k + 1], m[4 * k + 2], m[4 * k + 3]])
            })
        })
        .collect();
    if matrices.iter().flatten().any(|v| !v.is_finite()) {
        bail!("a matrix has a value that is not finite");
    }
    Ok(Instances { matrices })
}
