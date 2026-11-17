//! `.ptcl` particle effects: Nintendo's VFXB binary (NintendoWare / Eft2), version 0x16 in this game.
//!
//! Field order follows KillzXGaming's EffectLibrary (EFT2 `EmitterData`, MIT); the byte offsets below were found
//! by checking the user's files (2026-11-16). Run `probe --vfxb` over the whole dump to check them again.
//!
//! File header: "VFXB    ", u16 0x08 graphics API version, u16 0x0A version (0x16), u16 0x0C BOM (FEFF),
//! u16 0x16 offset of the first section (0x40), u32 0x1C file size.
//!
//! Section header (0x20 bytes; offsets from the section's start, 0xFFFFFFFF = none):
//!   0x00 magic, 0x04 u32 size, 0x08 u32 first child, 0x0C u32 next sibling, 0x10 u32 attribute section,
//!   0x14 u32 binary data, 0x1C u16 child count.
//! Tree: `ESTA` (emitter set list) > `ESET` (one effect, name at data +0x10) > `EMTR` (emitters; an emitter's
//! children are child emitters, spawned from its particles). An emitter's attribute chain holds extra blocks
//! (`CSDP`, `CUDP`, `CADP`, `EAES`, `FCOL`, `FRND`, ...; not read). `GRTF`: data is a BNTX with all the textures;
//! child `GTNT` lists them: entries {u64 id, u32 offset to the next entry (0 = last), u32 name length, name}.
//! `PRMA`/`G3PR`: primitive meshes (G3PR data is a BFRES); `GRSN`/`GRSC`: compiled shaders (not used).
//!
//! Emitter data: 0xAC0 bytes from the EMTR's binary offset (E). Offsets from E:
//!   0x010 name (64 bytes)
//!   0x060 u32 x6 key counts: colour 0, alpha 0, colour 1, alpha 1, scale, "param"
//!   0x0C0 f32 air resistance (velocity multiplier per frame)
//!   0x110 + 0x90 n  texture pattern of sampler n: f32 frame count, f32 frames per step, ..., +0x10 i32[32] table
//!   0x2C0 + 0x50 n  texture UV of sampler n (20 f32): [0,1] scroll per frame, [2,3] scroll, [4,5] scroll random,
//!                   [6,7] scale per frame, [8,9] UV scale (repeat), [10,11] scale random, [12] rotation per
//!                   frame, [13] rotation, [14] rotation random, [18,19] sheet columns, rows
//!   0x3A8 f32 colour scale
//!   0x3C0 / 0x440 / 0x4C0 / 0x540  key tables colour 0, alpha 0, colour 1, alpha 1: 8 keys {x, y, z, time 0..1}
//!   0x600 key table scale (x, y multipliers), 0x680 "param" table
//!   0x700 f32x3 rotation, 0x710 rotation random, 0x720 rotation per frame, 0x730 its random (radians)
//!   0x770 emitter translation, 0x77C its random, 0x788 rotation, 0x794 its random, 0x7A0 scale,
//!   0x7AC emitter colour 0 RGBA, 0x7BC colour 1
//!   0x7F0 u8 one-time (stops after the duration), 0x7F4 u32 start frame, 0x7FC u32 duration, 0x800 f32 particles
//!   per emission, 0x804 its random, 0x808 i32 frames between emissions, 0x80C its random,
//!   0x814 f32 gravity scale, 0x818 f32x3 gravity direction
//!   0x838 u8 volume type, 0x840 f32 arc (longitude), 0x844 latitude, 0x85C f32x3 volume radius, 0x868 form scale
//!   0x898 render state: u8 blend on, depth test, depth func, depth write, alpha test, alpha func, 0x89E blend type
//!   (0 alpha, 1 add, 2 subtract, 3 screen, 4 multiply), 0x89F display side
//!   0x8A8 u8 infinite life, 0x8AA u8 particle type (0 billboard, 2 Y billboard, 3 polygon XY, 4 polygon XZ,
//!   5/6 along the velocity), 0x8B8 i32 life (frames), 0x8BC its random
//!   0x964 f32 speed outwards from the volume, 0x968 speed along the direction, 0x96C f32x3 direction,
//!   0x978 spread angle (degrees), 0x98C speed random (%), 0x990 emitter velocity inherited
//!   0x99C u8 x4 colour 0, colour 1, alpha 0, alpha 1 types (0 fixed, 1 random key, 2 keyed over life)
//!   0x9A0 colour 0 RGB + alpha 0, 0x9B0 colour 1 RGB + alpha 1
//!   0x9C0 f32x3 particle scale, 0x9CC its random (%)
//!   0x9F0 + 0x20 n  sampler n: u64 texture id (GTNT), u8 wrap U, wrap V (0 mirror, 1 repeat, 2 clamp; mirrored
//!                   textures are a quarter of the image, shown over UV 0..2), filter, f32 max LOD, LOD bias
//!   0xA50 + 0x10 n  texture animation of sampler n: u8 pattern type (0 none, 1 fit to life, 2 clamp, 3 loop,
//!   4 random frame), ...

use anyhow::{Result, bail};
use std::collections::HashMap;

const NONE: u32 = 0xFFFF_FFFF;
const EMITTER_SIZE: usize = 0xAC0;

fn u8_at(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn i32_at(b: &[u8], at: usize) -> i32 {
    u32_at(b, at) as i32
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u32_at(b, at) as u64 | (u32_at(b, at + 4) as u64) << 32
}
fn f32_at(b: &[u8], at: usize) -> f32 {
    let f = f32::from_bits(u32_at(b, at));
    if f.is_finite() { f } else { 0.0 }
}
fn vec3_at(b: &[u8], at: usize) -> [f32; 3] {
    [f32_at(b, at), f32_at(b, at + 4), f32_at(b, at + 8)]
}
fn cstr(b: &[u8], at: usize, max: usize) -> String {
    let s = b.get(at..(at + max).min(b.len())).unwrap_or(&[]);
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Key {
    pub value: [f32; 3],
    /// 0..1 of the particle's life.
    pub time: f32,
}

/// Up to 8 keys over the particle's life.
#[derive(Clone, Debug, Default)]
pub struct Keys(pub Vec<Key>);

impl Keys {
    fn read(b: &[u8], at: usize, count: u32) -> Keys {
        Keys(
            (0..count.min(8) as usize)
                .map(|i| Key {
                    value: vec3_at(b, at + 16 * i),
                    time: f32_at(b, at + 16 * i + 12),
                })
                .collect(),
        )
    }

    /// The value at `t` (0..1), linear between keys.
    pub fn at(&self, t: f32) -> Option<[f32; 3]> {
        let k = &self.0;
        let first = k.first()?;
        if t <= first.time || k.len() == 1 {
            return Some(first.value);
        }
        for w in k.windows(2) {
            if t <= w[1].time {
                let span = (w[1].time - w[0].time).max(1e-6);
                let f = ((t - w[0].time) / span).clamp(0.0, 1.0);
                return Some(std::array::from_fn(|i| {
                    w[0].value[i] + (w[1].value[i] - w[0].value[i]) * f
                }));
            }
        }
        k.last().map(|l| l.value)
    }
}

/// How a colour or alpha is set: fixed, a random key per particle, or keyed over life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorType {
    Fixed,
    Random,
    Keyed,
}

impl ColorType {
    fn from(v: u8) -> ColorType {
        match v {
            1 => ColorType::Random,
            2 => ColorType::Keyed,
            _ => ColorType::Fixed,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sampler {
    /// Id in the texture table (`GTNT`); 0 or all ones = no texture.
    pub texture: u64,
    /// Wrap U, V: 0 mirror (the texture is a quarter, mirrored over UV 0..2), 1 repeat, 2 clamp.
    pub wrap: [u8; 2],
    /// Pattern animation: 0 none, 1 fit to life, 2 clamp, 3 loop, 4 random frame.
    pub pattern_type: u8,
    pub frames: u32,
    pub frames_per_step: f32,
    pub table: Vec<i32>,
    /// Sprite sheet columns and rows.
    pub div: [f32; 2],
    pub uv_scale: [f32; 2],
    pub scroll: [f32; 2],
    pub scroll_add: [f32; 2],
    pub scroll_random: [f32; 2],
}

impl Sampler {
    pub fn has_texture(&self) -> bool {
        self.texture != 0 && self.texture != u64::MAX
    }
}

#[derive(Clone, Debug)]
pub struct Emitter {
    pub name: String,
    pub children: Vec<Emitter>,
    pub key_counts: [u32; 6],
    pub air_resistance: f32,
    pub color_scale: f32,
    pub color0_keys: Keys,
    pub alpha0_keys: Keys,
    pub scale_keys: Keys,
    pub rotation: [f32; 3],
    pub rotation_random: [f32; 3],
    pub rotation_add: [f32; 3],
    pub rotation_add_random: [f32; 3],
    pub translate: [f32; 3],
    pub translate_random: [f32; 3],
    pub emitter_rotate: [f32; 3],
    pub emitter_scale: [f32; 3],
    pub emitter_color0: [f32; 4],
    pub one_time: bool,
    pub start: u32,
    pub duration: u32,
    pub rate: f32,
    pub rate_random: f32,
    pub interval: i32,
    pub interval_random: f32,
    pub gravity_scale: f32,
    pub gravity_dir: [f32; 3],
    pub volume: u8,
    pub arc: f32,
    pub volume_radius: [f32; 3],
    pub volume_scale: [f32; 3],
    pub blend: u8,
    pub infinite_life: bool,
    pub particle_type: u8,
    pub life: i32,
    pub life_random: i32,
    pub speed_out: f32,
    pub speed_dir: f32,
    pub direction: [f32; 3],
    pub spread_deg: f32,
    pub speed_random: f32,
    pub color0_type: ColorType,
    pub alpha0_type: ColorType,
    pub color0: [f32; 3],
    pub alpha0: f32,
    pub scale: [f32; 3],
    pub scale_random: f32,
    pub samplers: [Sampler; 3],
}

impl Emitter {
    fn read(b: &[u8], e: usize) -> Result<Emitter> {
        if e + EMITTER_SIZE > b.len() {
            bail!("emitter data at {e:#x} runs past the end");
        }
        let counts: [u32; 6] = std::array::from_fn(|i| u32_at(b, e + 0x60 + 4 * i));
        let samplers = std::array::from_fn(|n| {
            let pat = e + 0x110 + 0x90 * n;
            let uv = e + 0x2C0 + 0x50 * n;
            let smp = e + 0x9F0 + 0x20 * n;
            let frames = f32_at(b, pat).clamp(0.0, 32.0) as u32;
            Sampler {
                texture: u64_at(b, smp),
                wrap: [u8_at(b, smp + 8), u8_at(b, smp + 9)],
                pattern_type: u8_at(b, e + 0xA50 + 0x10 * n),
                frames,
                frames_per_step: f32_at(b, pat + 4),
                table: (0..frames as usize)
                    .map(|i| i32_at(b, pat + 0x10 + 4 * i))
                    .collect(),
                div: [
                    f32_at(b, uv + 72).clamp(1.0, 64.0),
                    f32_at(b, uv + 76).clamp(1.0, 64.0),
                ],
                uv_scale: [f32_at(b, uv + 32), f32_at(b, uv + 36)],
                scroll: [f32_at(b, uv + 8), f32_at(b, uv + 12)],
                scroll_add: [f32_at(b, uv), f32_at(b, uv + 4)],
                scroll_random: [f32_at(b, uv + 16), f32_at(b, uv + 20)],
            }
        });
        let c = |o: usize| f32_at(b, e + o);
        Ok(Emitter {
            name: cstr(b, e + 0x10, 64),
            children: Vec::new(),
            key_counts: counts,
            air_resistance: c(0xC0),
            color_scale: c(0x3A8),
            color0_keys: Keys::read(b, e + 0x3C0, counts[0]),
            alpha0_keys: Keys::read(b, e + 0x440, counts[1]),
            scale_keys: Keys::read(b, e + 0x600, counts[4]),
            rotation: vec3_at(b, e + 0x700),
            rotation_random: vec3_at(b, e + 0x710),
            rotation_add: vec3_at(b, e + 0x720),
            rotation_add_random: vec3_at(b, e + 0x730),
            translate: vec3_at(b, e + 0x770),
            translate_random: vec3_at(b, e + 0x77C),
            emitter_rotate: vec3_at(b, e + 0x788),
            emitter_scale: vec3_at(b, e + 0x7A0),
            emitter_color0: [c(0x7AC), c(0x7B0), c(0x7B4), c(0x7B8)],
            one_time: u8_at(b, e + 0x7F0) != 0,
            start: u32_at(b, e + 0x7F4),
            duration: u32_at(b, e + 0x7FC),
            rate: c(0x800),
            rate_random: c(0x804),
            interval: i32_at(b, e + 0x808),
            interval_random: c(0x80C),
            gravity_scale: c(0x814),
            gravity_dir: vec3_at(b, e + 0x818),
            volume: u8_at(b, e + 0x838),
            arc: c(0x840),
            volume_radius: vec3_at(b, e + 0x85C),
            volume_scale: vec3_at(b, e + 0x868),
            blend: u8_at(b, e + 0x89E),
            infinite_life: u8_at(b, e + 0x8A8) != 0,
            particle_type: u8_at(b, e + 0x8AA),
            life: i32_at(b, e + 0x8B8),
            life_random: i32_at(b, e + 0x8BC),
            speed_out: c(0x964),
            speed_dir: c(0x968),
            direction: vec3_at(b, e + 0x96C),
            spread_deg: c(0x978),
            speed_random: c(0x98C),
            color0_type: ColorType::from(u8_at(b, e + 0x99C)),
            alpha0_type: ColorType::from(u8_at(b, e + 0x99E)),
            color0: vec3_at(b, e + 0x9A0),
            alpha0: c(0x9AC),
            scale: vec3_at(b, e + 0x9C0),
            scale_random: c(0x9CC),
            samplers,
        })
    }

    /// This emitter and its children, depth first.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Emitter::count).sum::<usize>()
    }
}

#[derive(Clone, Debug)]
pub struct EmitterSet {
    pub name: String,
    pub emitters: Vec<Emitter>,
}

pub struct Ptcl {
    pub version: u16,
    pub sets: Vec<EmitterSet>,
    /// Texture id (from `GTNT`) to its name.
    pub texture_names: HashMap<u64, String>,
    /// The `GRTF` BNTX (all the textures), as a range of the file.
    pub bntx: Option<std::ops::Range<usize>>,
}

struct Section {
    magic: [u8; 4],
    at: usize,
    child: u32,
    next: u32,
    data: u32,
}

fn section(b: &[u8], at: usize) -> Option<Section> {
    let magic: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    if !magic.iter().all(|c| c.is_ascii_alphanumeric()) || at + 0x20 > b.len() {
        return None;
    }
    Some(Section {
        magic,
        at,
        child: u32_at(b, at + 8),
        next: u32_at(b, at + 12),
        data: u32_at(b, at + 20),
    })
}

/// The sections of a sibling chain starting at `at`.
fn chain(b: &[u8], at: usize) -> Vec<Section> {
    let mut out = Vec::new();
    let mut at = Some(at);
    while let Some(a) = at {
        let Some(s) = section(b, a) else { break };
        at = (s.next != NONE && s.next != 0)
            .then(|| a + s.next as usize)
            .filter(|n| *n > a && out.len() < 10_000);
        out.push(s);
    }
    out
}

fn emitters(b: &[u8], at: usize, depth: usize) -> Result<Vec<Emitter>> {
    let mut out = Vec::new();
    for s in chain(b, at) {
        if &s.magic != b"EMTR" {
            continue;
        }
        let mut e = Emitter::read(b, s.at + s.data as usize)?;
        if s.child != NONE && depth < 4 {
            e.children = emitters(b, s.at + s.child as usize, depth + 1)?;
        }
        out.push(e);
    }
    Ok(out)
}

pub fn parse(b: &[u8]) -> Result<Ptcl> {
    if !b.starts_with(b"VFXB") {
        bail!("not a VFXB effect file");
    }
    let version = u32_at(b, 0x0A) as u16;
    let first = (u32_at(b, 0x16) & 0xFFFF) as usize;
    let mut sets = Vec::new();
    let mut texture_names = HashMap::new();
    let mut bntx = None;
    for top in chain(b, first) {
        match &top.magic {
            b"ESTA" if top.child != NONE => {
                for s in chain(b, top.at + top.child as usize) {
                    if &s.magic != b"ESET" {
                        continue;
                    }
                    let name = cstr(b, s.at + s.data as usize + 0x10, 64);
                    let emitters = if s.child != NONE {
                        emitters(b, s.at + s.child as usize, 0)?
                    } else {
                        Vec::new()
                    };
                    sets.push(EmitterSet { name, emitters });
                }
            }
            b"GRTF" => {
                let start = top.at + top.data as usize;
                if b.get(start..start + 4) == Some(b"BNTX") {
                    bntx = Some(start..b.len());
                }
                if top.child != NONE {
                    for s in chain(b, top.at + top.child as usize) {
                        if &s.magic != b"GTNT" {
                            continue;
                        }
                        let mut p = s.at + s.data as usize;
                        for _ in 0..4096 {
                            let id = u64_at(b, p);
                            let next = u32_at(b, p + 8) as usize;
                            let len = u32_at(b, p + 12) as usize;
                            texture_names.insert(id, cstr(b, p + 16, len));
                            if next == 0 || p + next >= b.len() {
                                break;
                            }
                            p += next;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if version != 0x16 {
        bail!("VFXB version {version:#x}; only 0x16 is known");
    }
    Ok(Ptcl {
        version,
        sets,
        texture_names,
        bntx,
    })
}
