//! Trinity skeletal animations (.tranm). Field slots, checked against the game's files:
//!   tranm  { 0: Info {0: loops (u32), 1: frame count, 2: frames per second}, 1: {0: [BoneTrack]} }
//!   BoneTrack { 0: bone name, 1/2: scale (union type, value), 3/4: rotation, 5/6: translation }
//!   union types: 1 fixed {0: one value}, 2 dynamic {0: one value per frame},
//!                3 keyed, 16-bit frames {0: [u16] frames, 1: values}, 4 keyed, 8-bit frames {0: [u8] frames, 1: values}
//!   values: vectors are 3 x f32; rotations are packed quaternions, 3 x u16 ("smallest three": two bits name the
//!   largest component, which is left out and rebuilt; one bit is its sign; three 15-bit fields hold the others).
//! Tracks hold the bone's local transform (they replace the rest pose of the bone they name).
//! The animation list of a model is in its <name>_base.tracr; the .tranm files sit next to the model.

use crate::archive::Archive;
use crate::fb::{Table, get, u16_at, u32_at};
use crate::skeleton::Local;
use anyhow::{Result, anyhow, bail};
use three_d::{InnerSpace, Quat, Vec3};

/// One channel of values over frames (also used by camera animations, `camera_anim.rs`).
#[derive(Clone)]
pub(crate) enum Track<T> {
    None,
    Fixed(T),
    Dynamic(Vec<T>),
    Keyed(Vec<u32>, Vec<T>),
}

pub struct BoneTrack {
    pub name: String,
    scale: Track<Vec3>,
    rotation: Track<Quat>,
    translation: Track<Vec3>,
}

pub struct Animation {
    pub frames: u32,
    pub fps: u32,
    pub looping: bool,
    pub tracks: Vec<BoneTrack>,
}

/// One bone's local transform at some time; `None` parts keep the rest pose.
pub struct Sample {
    pub scale: Option<Vec3>,
    pub rotation: Option<Quat>,
    pub translation: Option<Vec3>,
}

/// How the packed quaternion is expanded. `probe --quat-layouts` compared the options on fixed tracks against the
/// rest pose: the 15-bit fields map linearly to ±π/4 (exact, e.g. raw 0005 0002 0cc3 -> (0, 0.7071, 0, -0.7071)),
/// the index is in bits 0-1, and bit 2 only flips the whole quaternion (the same rotation), so it is ignored.
#[derive(Clone, Copy, Debug)]
pub struct QuatCodec {
    /// true: components span ±π/4, false: ±1/√2.
    pub quarter_pi: bool,
    /// true: bit 2 negates the rebuilt largest component.
    pub sign_bit: bool,
}

pub const CODEC: QuatCodec = QuatCodec {
    quarter_pi: true,
    sign_bit: false,
};

pub fn unpack_quat(x: u16, y: u16, z: u16, c: QuatCodec) -> Quat {
    let pack = ((z as u64) << 32) | ((y as u64) << 16) | x as u64;
    let expand = |i: u64| -> f32 {
        let t = i as f32 / 32767.0;
        if c.quarter_pi {
            t * std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_4
        } else {
            (t - 0.5) * std::f32::consts::SQRT_2
        }
    };
    let q = [
        expand((pack >> 3) & 0x7fff),
        expand((pack >> 18) & 0x7fff),
        expand((pack >> 33) & 0x7fff),
    ];
    let mut largest = (1.0 - (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]))
        .max(0.0)
        .sqrt();
    if c.sign_bit && pack & 4 != 0 {
        largest = -largest;
    }
    let missing = (pack & 3) as usize;
    let mut v = [0.0f32; 4];
    let mut k = 0;
    for (i, slot) in v.iter_mut().enumerate() {
        if i == missing {
            *slot = largest;
        } else {
            *slot = q[k];
            k += 1;
        }
    }
    // [x, y, z, w]
    Quat::new(v[3], v[0], v[1], v[2]).normalize()
}

pub(crate) fn vec3_at(buf: &[u8], at: usize) -> Result<Vec3> {
    let f = |k: usize| -> Result<f32> { Ok(f32::from_bits(u32_at(buf, at + 4 * k)?)) };
    Ok(Vec3::new(f(0)?, f(1)?, f(2)?))
}

pub(crate) fn quat_at(buf: &[u8], at: usize, c: QuatCodec) -> Result<Quat> {
    Ok(unpack_quat(
        u16_at(buf, at)?,
        u16_at(buf, at + 2)?,
        u16_at(buf, at + 4)?,
        c,
    ))
}

/// Reads one union channel: `kind` is the union type, `value` the table it points to; `size` is the byte size of
/// one value, `read` reads one at a position.
pub(crate) fn channel<T>(
    kind: u8,
    value: Option<Table>,
    size: usize,
    read: &dyn Fn(&[u8], usize) -> Result<T>,
) -> Result<Track<T>> {
    let Some(t) = value else {
        return Ok(Track::None);
    };
    let values = |slot: usize| -> Result<Vec<T>> {
        let (start, n) = t.vector(slot)?;
        get(t.buf, start, n * size)?;
        (0..n).map(|i| read(t.buf, start + i * size)).collect()
    };
    Ok(match kind {
        0 => Track::None,
        1 => match t.field(0) {
            Some(at) => Track::Fixed(read(t.buf, at)?),
            None => Track::None,
        },
        2 => Track::Dynamic(values(0)?),
        3 | 4 => {
            let (start, n) = t.vector(0)?;
            let frames: Vec<u32> = if kind == 3 {
                (0..n)
                    .map(|i| u16_at(t.buf, start + 2 * i).map(|f| f as u32))
                    .collect::<Result<_>>()?
            } else {
                get(t.buf, start, n)?.iter().map(|b| *b as u32).collect()
            };
            let v = values(1)?;
            if v.len() != frames.len() || v.is_empty() {
                bail!("keyed track: {} frames, {} values", frames.len(), v.len());
            }
            Track::Keyed(frames, v)
        }
        k => bail!("track type {k} not handled"),
    })
}

pub fn parse(buf: &[u8], _label: &str, codec: QuatCodec) -> Result<Animation> {
    let root = Table::root(buf, 0)?;
    let info = root.table(0)?.ok_or_else(|| anyhow!("no info table"))?;
    let looping = info.u32(0, 0) != 0;
    let frames = info.u32(1, 0).max(1);
    let fps = info.u32(2, 30).max(1);
    let bones = root.table(1)?.ok_or_else(|| anyhow!("no bone animation"))?;
    let rq = move |b: &[u8], at: usize| quat_at(b, at, codec);
    let mut tracks = Vec::new();
    for t in bones.tables(0)? {
        let name = t.string(0)?.unwrap_or_default();
        tracks.push(BoneTrack {
            scale: channel(t.u8(1, 0), t.table(2)?, 12, &vec3_at)?,
            rotation: channel(t.u8(3, 0), t.table(4)?, 6, &rq)?,
            translation: channel(t.u8(5, 0), t.table(6)?, 12, &vec3_at)?,
            name,
        });
    }
    Ok(Animation {
        frames,
        fps,
        looping,
        tracks,
    })
}

pub(crate) trait Blend: Copy {
    fn blend(a: Self, b: Self, t: f32) -> Self;
}
impl Blend for Vec3 {
    fn blend(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}
impl Blend for f32 {
    fn blend(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}
impl Blend for Quat {
    fn blend(a: Self, b: Self, t: f32) -> Self {
        nlerp(a, b, t)
    }
}

/// Normalized linear interpolation of two rotations, along the shorter arc.
pub fn nlerp(a: Quat, b: Quat, t: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    (a * (1.0 - t) + b * t).normalize()
}

pub(crate) fn sample<T: Blend>(track: &Track<T>, frame: f32) -> Option<T> {
    match track {
        Track::None => None,
        Track::Fixed(v) => Some(*v),
        Track::Dynamic(v) => {
            let last = v.len().checked_sub(1)?;
            let f = frame.clamp(0.0, last as f32);
            let i = f.floor() as usize;
            let j = (i + 1).min(last);
            Some(T::blend(v[i], v[j], f - i as f32))
        }
        Track::Keyed(frames, v) => {
            // the last key at or before `frame`, blended towards the next
            let k = frames.partition_point(|f| (*f as f32) <= frame);
            if k == 0 {
                return Some(v[0]);
            }
            if k >= frames.len() {
                return Some(v[v.len() - 1]);
            }
            let (f0, f1) = (frames[k - 1] as f32, frames[k] as f32);
            let t = if f1 > f0 {
                (frame - f0) / (f1 - f0)
            } else {
                0.0
            };
            Some(T::blend(v[k - 1], v[k], t))
        }
    }
}

impl Sample {
    /// The full local transform, with the parts the track leaves out taken from `rest`.
    pub fn or_rest(&self, rest: Local) -> Local {
        Local {
            scale: self.scale.unwrap_or(rest.scale),
            rotation: self.rotation.unwrap_or(rest.rotation),
            translation: self.translation.unwrap_or(rest.translation),
        }
    }
}

impl BoneTrack {
    pub fn sample(&self, frame: f32) -> Sample {
        Sample {
            scale: sample(&self.scale, frame),
            rotation: sample(&self.rotation, frame),
            translation: sample(&self.translation, frame),
        }
    }
}

/// The animations a model's <stem>_base.tracr lists, as (display name, .tranm path) pairs. Names are read as
/// strings ending in ".tranm" so that the exact schema of the list does not matter.
pub fn list(ar: &mut Archive, model_path: &str) -> Vec<(String, String)> {
    let dir = model_path
        .rfind('/')
        .map(|i| &model_path[..=i])
        .unwrap_or("");
    let stem = model_path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".trmdl");
    list_at(ar, &format!("{dir}{stem}_base.tracr"), stem)
}

/// The animations listed in the .tracr at `tracr_path` (resolved against its folder). `stem` is cut off the front
/// of each clip's name for its label (e.g. "p0_00" for the player's "p0_00_00000_defaultwait01_loop").
pub fn list_at(ar: &mut Archive, tracr_path: &str, stem: &str) -> Vec<(String, String)> {
    let dir = tracr_path
        .rfind('/')
        .map(|i| &tracr_path[..=i])
        .unwrap_or("");
    let Ok(buf) = ar.read(tracr_path) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i + 4 <= buf.len() {
        // FlatBuffers strings: u32 length, bytes, NUL. Checked at every byte offset: the list is small, and
        // stepping by the alignment loses step after a match.
        let n = u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]) as usize;
        if (7..200).contains(&n)
            && i + 4 + n < buf.len()
            && buf[i + 4 + n] == 0
            && let Ok(s) = std::str::from_utf8(&buf[i + 4..i + 4 + n])
            && s.ends_with(".tranm")
            && s.is_ascii()
        {
            let path = crate::model::resolve(dir, s);
            if !out.iter().any(|(_, p)| *p == path) && ar.contains(&path) {
                let label = s
                    .trim_end_matches(".tranm")
                    .trim_start_matches(stem)
                    .trim_start_matches('_');
                out.push((label.to_string(), path));
            }
            i += 4 + n;
            continue;
        }
        i += 1;
    }
    // Labels start with a five-digit number (00000 idle, 00030 walk, ...), so this sorts them in the game's order.
    out.sort();
    out
}

pub fn load(ar: &mut Archive, path: &str, label: &str) -> Result<Animation> {
    parse(&ar.read(path)?, label, CODEC)
}
