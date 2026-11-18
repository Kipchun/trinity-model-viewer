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
//! children are child emitters, spawned from its particles). An emitter's attribute chain (section +0x10) holds
//! extra blocks, read into `Extras`: emitter animations `EA**` (427 over the dump, layout from EffectLibrary),
//! fields `F***` and the stripe plugin `EP02` / `EP03` (layouts from the dump). Not read: `FCLN` curl noise (114),
//! `FCSF` (8), `EAET` (14), `EP04` (19, looks like an area loop), `CADP` / `CSDP` / `CUDP` (shader and user data). `GRTF`: data is a BNTX with all the textures;
//! child `GTNT` lists them: entries {u64 id, u32 offset to the next entry (0 = last), u32 name length, name}.
//! `G3PR`: primitive meshes; its data is a BFRES (`bfres.rs`; size from the BFRES header, the section's size is
//! smaller), child `G3NT` lists the primitives in the BFRES's model order, same entry layout as `GTNT` with 8
//! bytes: the BFRES attribute index for position, normal, tangent, colour, UV 0, UV 1 (0xFF none), 2 unknown.
//! `PRMA` (older primitive table) is empty here but in 14 files. `GRSN`/`GRSC`: compiled shaders (not used).
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
//!   0x700 f32x3 rotation, 0x710 rotation random, 0x720 rotation per frame, 0x72C f32 spin kept per frame (1 or
//!   less; looks like a decay), 0x730 rotation per frame random (radians). Axes x, y, z; quads use z.
//!   0x770 emitter translation, 0x77C its random, 0x788 rotation, 0x794 its random, 0x7A0 scale,
//!   0x7AC emitter colour 0 RGBA, 0x7BC colour 1
//!   0x7F0 u8 one-time (stops after the duration), 0x7F4 u32 start frame, 0x7FC u32 duration, 0x800 f32 particles
//!   per emission, 0x804 its random, 0x808 i32 frames between emissions, 0x80C its random,
//!   0x814 f32 gravity scale, 0x818 f32x3 gravity direction
//!   0x838 u8 volume type, 0x840 f32 arc (longitude), 0x844 latitude, 0x85C f32x3 volume radius, 0x868 form scale
//!   0x898 render state: u8 blend on, depth test, depth func, depth write, alpha test, alpha func, 0x89E blend type
//!   (0 alpha, 1 add, 2 subtract, 3 screen, 4 multiply, 5 screen distortion: bends what is behind), 0x89F
//!   display side
//!   0x8A8 u8 infinite life, 0x8AA u8 particle type (0 billboard, 2 Y billboard, 3 polygon XY, 4 polygon XZ,
//!   5/6 along the velocity), 0x8B8 i32 life (frames), 0x8BC its random
//!   0x8C8 u64 primitive id (`G3NT`; 0 = quads; 1034 of 2594 emitters have one, all found)
//!   0x8F8 combiner, u8 each (EffectLibrary's `EmitterCombiner`, field order for versions < 36; meanings worked
//!   out from the dump, see `Combiner`): colour process, alpha process, texture 1 / texture 2 / primitive colour
//!   blend, texture 1 / 2 / primitive alpha blend, texture 0 / 1 / 2 colour input, texture 0 / 1 / 2 alpha input,
//!   primitive colour input, primitive alpha input, 0x908 shader type (0 2287, 1 24, 2 283; see `Combiner`),
//!   0x909 apply alpha (1 in 2578), 0x90A distortion by camera distance
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

/// What an emitter animation (`EA**`) drives; the value replaces the emitter's own setting while it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimTarget {
    /// `EATR` emitter translation.
    Translate,
    /// `EASL` emitter scale.
    Scale,
    /// `EAER` particles per emission (x).
    Rate,
    /// `EAC0` / `EAC1` emitter colour 0 / 1 (rgb).
    Color0,
    Color1,
    /// `EAA0` emitter alpha 0 (x).
    Alpha0,
    /// `EAOV` speed outwards (x), `EADV` speed along the direction (x).
    SpeedOut,
    SpeedDir,
    /// `EAGV` gravity scale (x).
    Gravity,
    /// `EAPL` particle life in frames (x).
    Life,
    /// `EASS` particle scale multiplier (xyz).
    ParticleScale,
    /// `EAES` volume scale (xyz).
    VolumeScale,
}

/// An emitter animation: keys over the emitter's own time, in frames.
#[derive(Clone, Debug)]
pub struct EmitterAnim {
    pub target: AnimTarget,
    pub looping: bool,
    /// {x, y, z} at frame `time`.
    pub keys: Keys,
}

impl EmitterAnim {
    /// The value `frame` frames into the emitter's life.
    pub fn at(&self, frame: f32) -> Option<[f32; 3]> {
        let last = self.keys.0.last()?.time;
        let t = if self.looping && last > 0.0 {
            frame.rem_euclid(last)
        } else {
            frame
        };
        self.keys.at(t)
    }
}

/// Fields: forces on the particles each frame. Layouts from the dump (2026-11-18, `probe --vfxb-blocks <magic>`);
/// meanings guessed from the values and names, not checked against the game.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    /// `FSPN` (142 emitters): turn about the emitter's axis +4 (0 x, 1 y, 2 z) by +0 radians per frame; +8 speed
    /// outwards from that axis per frame.
    pub spin: Option<(f32, u8, f32)>,
    /// `FRN1` (11): velocity jitter: +0 xyz amplitude, +0xC every n frames.
    pub jitter: Option<([f32; 3], u32)>,
    /// `FRND` (240): smooth random offset of the drawn position; +4 xyz amplitude. The block also holds 4 noise
    /// frequencies and weights (+0x1C: 4, 3, 2, 1.5, then 0.6, 0.42, 0.23, 0.15 in every file seen).
    pub noise: Option<([f32; 3], [f32; 4], [f32; 4])>,
    /// `FCOV` (60): pull towards the emitter, a fraction per frame: +0x10, or over life from its keys (+0x20).
    pub converge: Option<(f32, Keys)>,
    /// `FMAG` (9): magnet towards the emitter: +4 strength (added to the velocity per frame, per unit of distance),
    /// +0 u8 x4 axes on.
    pub magnet: Option<(f32, [bool; 3])>,
    /// `FPAD` (1): added to the position each frame.
    pub pos_add: Option<[f32; 3]>,
    /// `FCOL` (5): a floor at height +8 (emitter space); +0 u8 0 bounce, else stop; +0x10 bounce kept.
    pub floor: Option<(f32, u8, f32)>,
}

/// The stripe plugin: the particle is drawn as a ribbon through its last `length` positions (`EP02`: 97 emitters
/// named `trail`, `stripe`, `tail`; history length at +0x14. `EP03`: 14, at +0x18).
#[derive(Clone, Copy, Debug)]
pub struct Stripe {
    pub length: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Extras {
    pub anims: Vec<EmitterAnim>,
    pub fields: Fields,
    pub stripe: Option<Stripe>,
    /// Blocks read but not used (e.g. `FCLN` curl noise, `EP04`), for the UI.
    pub skipped: Vec<String>,
}

impl Extras {
    /// The emitter's attribute chain at `at`.
    fn read(b: &[u8], at: usize) -> Extras {
        let mut x = Extras::default();
        for s in chain(b, at) {
            let d = s.at + s.data as usize;
            let f = |i: usize| f32_at(b, d + 4 * i);
            let v3 = |i: usize| [f(i), f(i + 1), f(i + 2)];
            let magic = String::from_utf8_lossy(&s.magic).into_owned();
            let target = match &s.magic {
                b"EATR" => Some(AnimTarget::Translate),
                b"EASL" => Some(AnimTarget::Scale),
                b"EAER" => Some(AnimTarget::Rate),
                b"EAC0" => Some(AnimTarget::Color0),
                b"EAC1" => Some(AnimTarget::Color1),
                b"EAA0" => Some(AnimTarget::Alpha0),
                b"EAOV" => Some(AnimTarget::SpeedOut),
                b"EADV" => Some(AnimTarget::SpeedDir),
                b"EAGV" => Some(AnimTarget::Gravity),
                b"EAPL" => Some(AnimTarget::Life),
                b"EASS" => Some(AnimTarget::ParticleScale),
                b"EAES" => Some(AnimTarget::VolumeScale),
                _ => None,
            };
            if let Some(target) = target {
                // u8 enable, loop, random start, pad; u32 key count; u32 loop count; keys {x, y, z, frame}
                if u8_at(b, d) != 0 {
                    x.anims.push(EmitterAnim {
                        target,
                        looping: u8_at(b, d + 1) != 0,
                        keys: Keys::read(b, d + 12, u32_at(b, d + 4)),
                    });
                }
                continue;
            }
            let fl = &mut x.fields;
            match &s.magic {
                b"FSPN" => fl.spin = Some((f(0), u8_at(b, d + 4), f(2))),
                b"FRN1" => fl.jitter = Some((v3(0), u32_at(b, d + 12).max(1))),
                b"FRND" => {
                    fl.noise = Some((
                        v3(1),
                        [f(7), f(8), f(9), f(10)],
                        [f(11), f(12), f(13), f(14)],
                    ))
                }
                b"FCOV" => fl.converge = Some((f(4), Keys::read(b, d + 0x28, u32_at(b, d + 0x20)))),
                b"FMAG" => {
                    fl.magnet = Some((
                        f(1),
                        [u8_at(b, d) != 0, u8_at(b, d + 1) != 0, u8_at(b, d + 2) != 0],
                    ))
                }
                b"FPAD" => fl.pos_add = Some(v3(0)),
                b"FCOL" => fl.floor = Some((f(2), u8_at(b, d), f(4))),
                b"EP02" => {
                    x.stripe = Some(Stripe {
                        length: (f(5) as u32).clamp(2, 256),
                    })
                }
                b"EP03" => {
                    x.stripe = Some(Stripe {
                        length: (f(6) as u32).clamp(2, 256),
                    })
                }
                b"CADP" | b"CSDP" | b"CUDP" => {}
                _ => x.skipped.push(magic),
            }
        }
        x
    }
}

/// How the textures and colours are mixed (emitter 0x8F8). Values as read from the dump (2026-11-18):
/// - colour process: 0 colour 0 only, 1 texture x colour 0, 2 colour 1 where the texture is dark to colour 0
///   where it is bright (`glow`: white core, orange edge; `stone`: white to black), 3 texture x colour 0 +
///   colour 1
/// - alpha process: 0 texture x alpha 0, 1 texture - (1 - alpha 0) (dissolve), 2 texture x alpha 0 x alpha 1,
///   3 dissolve x alpha 1, 4 distance field (threshold at 1 - alpha 0; edge width a guess)
/// - blends (textures 1, 2 and the mesh's vertex colour onto texture 0): 0 multiply, 1 add, 2 subtract
/// - inputs (colour and alpha): 0 as sampled, 1 red (BC4 glows: `glow_back` alpha 1), 2 one (opaque BC1 51 of
///   60, additive BC7 sparkles), colour 3 alpha; alpha 3 and colour 4 rare, not known
/// - shader type 1 / 2: with a two-channel texture 0 (BC5, RG8; most of the 283 type 2), texture 0 pushes the
///   UVs of textures 1-2; `star_A_01` (type 1) and `star_A_02` (type 2) have colour textures there, so the type
///   alone does not say it. Screen distortion is blend type 5 (the `dist` emitters).
///
/// Single-channel textures read as R everywhere through the BNTX channel selectors (`texture::channel_select`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Combiner {
    pub color_process: u8,
    pub alpha_process: u8,
    /// Texture 1, texture 2, primitive.
    pub color_blend: [u8; 3],
    pub alpha_blend: [u8; 3],
    /// Texture 0, 1, 2, primitive.
    pub color_input: [u8; 4],
    pub alpha_input: [u8; 4],
    pub shader_type: u8,
}

impl Combiner {
    fn read(b: &[u8], at: usize) -> Combiner {
        let v = |i: usize| u8_at(b, at + i);
        Combiner {
            color_process: v(0),
            alpha_process: v(1),
            color_blend: [v(2), v(3), v(4)],
            alpha_blend: [v(5), v(6), v(7)],
            color_input: [v(8), v(9), v(10), v(14)],
            alpha_input: [v(11), v(12), v(13), v(15)],
            shader_type: v(16),
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
    pub color1_keys: Keys,
    pub alpha1_keys: Keys,
    pub scale_keys: Keys,
    pub rotation: [f32; 3],
    pub rotation_random: [f32; 3],
    pub rotation_add: [f32; 3],
    pub rotation_add_random: [f32; 3],
    /// Spin kept per frame (1 = none lost); 0x72C.
    pub rotation_decay: f32,
    pub translate: [f32; 3],
    pub translate_random: [f32; 3],
    pub emitter_rotate: [f32; 3],
    pub emitter_scale: [f32; 3],
    pub emitter_color0: [f32; 4],
    pub emitter_color1: [f32; 4],
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
    pub color1_type: ColorType,
    pub alpha1_type: ColorType,
    pub color1: [f32; 3],
    pub alpha1: f32,
    pub combiner: Combiner,
    pub scale: [f32; 3],
    pub scale_random: f32,
    pub samplers: [Sampler; 3],
    /// Primitive (mesh) id in the `G3NT` table; 0 or all ones = none (the particle is a quad).
    pub primitive: u64,
    /// Emitter animations (`EA**` blocks), fields (`F***`) and the stripe plugin (`EP02` / `EP03`).
    pub extras: Extras,
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
            color1_keys: Keys::read(b, e + 0x4C0, counts[2]),
            alpha1_keys: Keys::read(b, e + 0x540, counts[3]),
            scale_keys: Keys::read(b, e + 0x600, counts[4]),
            rotation: vec3_at(b, e + 0x700),
            rotation_random: vec3_at(b, e + 0x710),
            rotation_add: vec3_at(b, e + 0x720),
            rotation_add_random: vec3_at(b, e + 0x730),
            rotation_decay: c(0x72C),
            translate: vec3_at(b, e + 0x770),
            translate_random: vec3_at(b, e + 0x77C),
            emitter_rotate: vec3_at(b, e + 0x788),
            emitter_scale: vec3_at(b, e + 0x7A0),
            emitter_color0: [c(0x7AC), c(0x7B0), c(0x7B4), c(0x7B8)],
            emitter_color1: [c(0x7BC), c(0x7C0), c(0x7C4), c(0x7C8)],
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
            color1_type: ColorType::from(u8_at(b, e + 0x99D)),
            alpha1_type: ColorType::from(u8_at(b, e + 0x99F)),
            color1: vec3_at(b, e + 0x9B0),
            alpha1: c(0x9BC),
            combiner: Combiner::read(b, e + 0x8F8),
            scale: vec3_at(b, e + 0x9C0),
            scale_random: c(0x9CC),
            samplers,
            primitive: u64_at(b, e + 0x8C8),
            extras: Extras::default(),
        })
    }

    pub fn has_primitive(&self) -> bool {
        self.primitive != 0 && self.primitive != u64::MAX
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
    /// The `G3PR` BFRES (the primitive meshes), as a range of the file.
    pub bfres: Option<std::ops::Range<usize>>,
    /// `G3NT`: the primitives, in the BFRES's model order: id and attribute map.
    pub primitives: Vec<PrimitiveEntry>,
}

/// One entry of the `G3NT` table.
#[derive(Clone, Debug)]
pub struct PrimitiveEntry {
    pub id: u64,
    /// Index of the BFRES attribute for position, normal, tangent, colour, UV 0, UV 1 (0xFF = none), then 2
    /// bytes not known.
    #[allow(dead_code)] // checked by the probe
    pub map: Vec<u8>,
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

/// One section in the file's tree (for the probe): depth, magic, offset, size, binary data offset (absolute),
/// attribute section offset (absolute), child count.
#[allow(dead_code)] // used by the probe
#[derive(Clone, Debug)]
pub struct SectionInfo {
    pub depth: usize,
    pub magic: String,
    pub at: usize,
    pub size: u32,
    pub data: Option<usize>,
    pub attr: Option<usize>,
    pub children: u16,
}

/// Every section of the file, depth first, attribute chains included (as children one level down).
#[allow(dead_code)] // used by the probe
pub fn sections(b: &[u8]) -> Vec<SectionInfo> {
    fn walk(b: &[u8], at: usize, depth: usize, out: &mut Vec<SectionInfo>) {
        for s in chain(b, at) {
            let abs = |o: u32| (o != NONE).then(|| s.at + o as usize);
            let attr = abs(u32_at(b, s.at + 0x10));
            out.push(SectionInfo {
                depth,
                magic: String::from_utf8_lossy(&s.magic).into_owned(),
                at: s.at,
                size: u32_at(b, s.at + 4),
                data: abs(s.data),
                attr,
                children: u32_at(b, s.at + 0x1C) as u16,
            });
            if out.len() > 20_000 || depth > 8 {
                return;
            }
            if let Some(a) = attr {
                walk(b, a, depth + 1, out);
            }
            if s.child != NONE {
                walk(b, s.at + s.child as usize, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    if b.starts_with(b"VFXB") {
        walk(b, (u32_at(b, 0x16) & 0xFFFF) as usize, 0, &mut out);
    }
    out
}

/// A `GTNT` / `G3NT` table: entries {u64 id, u32 offset to the next entry (0 = last), u32 length, bytes}.
fn id_table(b: &[u8], mut p: usize) -> Vec<(u64, Vec<u8>)> {
    let mut out = Vec::new();
    for _ in 0..4096 {
        if p + 16 > b.len() {
            break;
        }
        let id = u64_at(b, p);
        let next = u32_at(b, p + 8) as usize;
        let len = u32_at(b, p + 12) as usize;
        out.push((id, b.get(p + 16..p + 16 + len).unwrap_or(&[]).to_vec()));
        if next == 0 || p + next >= b.len() {
            break;
        }
        p += next;
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
        let attr = u32_at(b, s.at + 0x10);
        if attr != NONE {
            e.extras = Extras::read(b, s.at + attr as usize);
        }
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
    let mut bfres = None;
    let mut primitives = Vec::new();
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
                        if &s.magic == b"GTNT" {
                            for (id, name) in id_table(b, s.at + s.data as usize) {
                                let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                                texture_names
                                    .insert(id, String::from_utf8_lossy(&name[..end]).into_owned());
                            }
                        }
                    }
                }
            }
            b"G3PR" if top.data != NONE => {
                let start = top.at + top.data as usize;
                if b.get(start..start + 4) == Some(b"FRES") {
                    let size = u32_at(b, start + 0x1C) as usize;
                    bfres = Some(start..(start + size).min(b.len()));
                }
                if top.child != NONE {
                    for s in chain(b, top.at + top.child as usize) {
                        if &s.magic == b"G3NT" {
                            primitives.extend(
                                id_table(b, s.at + s.data as usize)
                                    .into_iter()
                                    .map(|(id, map)| PrimitiveEntry { id, map }),
                            );
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
        bfres,
        primitives,
    })
}
