//! Camera animations (`.trcma`), the shots of cutscenes. Field slots, checked against the game's files
//! (`probe --trcma`, `probe --cameras`):
//!   trcma  { 0: Info {1: frame count, 2: frames per second (60)}, 1: Camera }
//!   Camera { 0: name ("ubercam1"), 1: Lens, 2: Transform }
//!   Lens   (tag, value) pairs: 0/1 near plane, 2/3 far plane, 4/5 vertical field of view in radians (f32 values)
//!   Transform (tag, value) pairs: 0/1 rotation (packed quaternions, `anim::CODEC`), 2/3 translation (3 x f32,
//!          metres in the scene's frame), 4/5, 6/7, 8/9 all zero in the files looked at
//! The union tags are those of `.tranm` tracks (`anim.rs`): 1 fixed, 2 one value per frame, 3 / 4 keyed.
//! A timeline plays a shot with `CameraAnimation/Play` (`fileName` = the file's stem); the shot holds its last
//! frame once it has run out.

use crate::anim::{self, Track};
use crate::fb::{Table, u32_at};
use anyhow::{Result, anyhow};
use three_d::{Quat, Rotation, Vec3, vec3};

pub struct CameraAnim {
    pub frames: u32,
    pub fps: u32,
    near: Track<f32>,
    far: Track<f32>,
    fov: Track<f32>,
    rotation: Track<Quat>,
    translation: Track<Vec3>,
}

/// The camera at one moment.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub position: Vec3,
    pub rotation: Quat,
    /// Vertical field of view, radians.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Shot {
    /// The direction the camera looks: its local -z.
    pub fn forward(&self) -> Vec3 {
        self.rotation.rotate_vector(vec3(0.0, 0.0, -1.0))
    }
    pub fn up(&self) -> Vec3 {
        self.rotation.rotate_vector(vec3(0.0, 1.0, 0.0))
    }
}

fn f32_at(b: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_bits(u32_at(b, at)?))
}

impl CameraAnim {
    pub fn parse(buf: &[u8]) -> Result<CameraAnim> {
        let root = Table::root(buf, 0)?;
        let info = root.table(0)?.ok_or_else(|| anyhow!("no info table"))?;
        let cam = root.table(1)?.ok_or_else(|| anyhow!("no camera table"))?;
        let rq = |b: &[u8], at: usize| anim::quat_at(b, at, anim::CODEC);
        let (mut near, mut far, mut fov) = (Track::None, Track::None, Track::None);
        if let Some(l) = cam.table(1)? {
            near = anim::channel(l.u8(0, 0), l.table(1)?, 4, &f32_at)?;
            far = anim::channel(l.u8(2, 0), l.table(3)?, 4, &f32_at)?;
            fov = anim::channel(l.u8(4, 0), l.table(5)?, 4, &f32_at)?;
        }
        let (mut rotation, mut translation) = (Track::None, Track::None);
        if let Some(t) = cam.table(2)? {
            rotation = anim::channel(t.u8(0, 0), t.table(1)?, 6, &rq)?;
            translation = anim::channel(t.u8(2, 0), t.table(3)?, 12, &anim::vec3_at)?;
        }
        Ok(CameraAnim {
            frames: info.u32(1, 0).max(1),
            fps: info.u32(2, 60).max(1),
            near,
            far,
            fov,
            rotation,
            translation,
        })
    }

    /// The camera `seconds` into the shot (held on the last frame after the end).
    pub fn sample(&self, seconds: f32) -> Shot {
        let f = (seconds * self.fps as f32).clamp(0.0, (self.frames - 1) as f32);
        Shot {
            position: anim::sample(&self.translation, f).unwrap_or(vec3(0.0, 0.0, 0.0)),
            rotation: anim::sample(&self.rotation, f).unwrap_or(Quat::new(1.0, 0.0, 0.0, 0.0)),
            fov: anim::sample(&self.fov, f).unwrap_or(0.8),
            near: anim::sample(&self.near, f).unwrap_or(0.1),
            far: anim::sample(&self.far, f).unwrap_or(1000.0),
        }
    }
}
