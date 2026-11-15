//! Trinity skeletons (.trskl). Field slots, checked against the game's files:
//!   trskl  { 1: [TransformNode], 2: [JointInfo] }
//!   TransformNode { 0: name, 1: Transform {0: scale, 1: rotation (Euler, radians), 2: translation; each an inline
//!                   xyz}, 4: parent index (absent = none), 5: joint index (absent = not a skinning joint) }
//!   JointInfo { 2: inverse bind matrix {0..3: columns as inline xyz: x axis, y axis, z axis, translation} }
//! A bone's local matrix is T * Rz * Ry * Rx * S; its world matrix is the parent's world matrix times that. The
//! inverse bind matrices confirm the order: world * inverse bind = identity for the joints of 958 of the 976 Pokémon.
//! In the other 18 some limbs were skinned in a different pose than the stored rest pose, so two poses are kept:
//! `world` (rest pose, from the transforms; animations start from it) and `bind` (from the inverse bind matrices;
//! matches the mesh). A bone without a bind matrix follows its parent's bind pose with its own local transform.

use crate::archive::Archive;
use crate::fb::{Table, u32_at};
use anyhow::{Result, bail};
use three_d::{Mat4, Quat, Rotation3, SquareMatrix, Vec3, Vec4};

pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub joint: Option<usize>,
    pub scale: [f32; 3],
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    pub world: Mat4,
    pub bind: Mat4,
}

pub struct Skeleton {
    pub file: String,
    pub bones: Vec<Bone>,
    /// Inverse bind matrices by joint index.
    pub inverse_bind: Vec<Option<Mat4>>,
    /// Bones in an order where every parent comes before its children.
    order: Vec<usize>,
    /// The bone each joint index belongs to.
    joint_bone: Vec<Option<usize>>,
}

/// A bone's local transform: scale, rotation, translation (the matrix is T * R * S).
#[derive(Clone, Copy)]
pub struct Local {
    pub scale: Vec3,
    pub rotation: Quat,
    pub translation: Vec3,
}

impl Local {
    /// `a` at t = 0, `b` at t = 1: scale and translation interpolate linearly, rotation along the shorter arc.
    pub fn lerp(a: Local, b: Local, t: f32) -> Local {
        Local {
            scale: a.scale + (b.scale - a.scale) * t,
            rotation: crate::anim::nlerp(a.rotation, b.rotation, t),
            translation: a.translation + (b.translation - a.translation) * t,
        }
    }

    pub fn matrix(&self) -> Mat4 {
        let s = self.scale;
        Mat4::from_translation(self.translation)
            * Mat4::from(self.rotation)
            * Mat4::from_nonuniform_scale(s.x, s.y, s.z)
    }
}

impl Skeleton {
    /// Bone i's local transform in the rest pose (where animations start).
    pub fn rest_local(&self, i: usize) -> Local {
        let b = &self.bones[i];
        Local {
            scale: Vec3::from(b.scale),
            rotation: Quat::from_angle_z(three_d::radians(b.rotation[2]))
                * Quat::from_angle_y(three_d::radians(b.rotation[1]))
                * Quat::from_angle_x(three_d::radians(b.rotation[0])),
            translation: Vec3::from(b.translation),
        }
    }

    /// World matrices of all bones from their local transforms (one per bone, e.g. an animation frame).
    pub fn posed_world(&self, local: &[Local]) -> Vec<Mat4> {
        let mut world = vec![Mat4::identity(); self.bones.len()];
        for &i in &self.order {
            let m = local
                .get(i)
                .map_or_else(|| self.rest_local(i).matrix(), Local::matrix);
            world[i] = match self.bones[i].parent.filter(|p| *p < self.bones.len()) {
                Some(p) => world[p] * m,
                None => m,
            };
        }
        world
    }

    /// Per joint: the matrix that moves a bind-pose vertex to the posed position (posed world * inverse bind).
    pub fn skin_matrices(&self, world: &[Mat4]) -> Vec<Mat4> {
        self.joint_bone
            .iter()
            .zip(&self.inverse_bind)
            .map(|(bone, ibm)| match (bone, ibm) {
                (Some(b), Some(ibm)) => world[*b] * ibm,
                _ => Mat4::identity(),
            })
            .collect()
    }

    pub fn position(&self, i: usize, bind_pose: bool) -> Vec3 {
        let b = &self.bones[i];
        if bind_pose {
            b.bind.w.truncate()
        } else {
            b.world.w.truncate()
        }
    }

    pub fn children(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        self.bones
            .iter()
            .enumerate()
            .filter(move |(_, b)| b.parent == Some(i))
            .map(|(k, _)| k)
    }

    /// Largest difference from identity of world * inverse bind over all joints (0 = the bone maths matches the
    /// game's bind pose exactly).
    pub fn max_bind_error(&self) -> Option<f32> {
        let mut worst: Option<f32> = None;
        for b in &self.bones {
            let Some(ibm) = b
                .joint
                .and_then(|j| self.inverse_bind.get(j).copied().flatten())
            else {
                continue;
            };
            let m = b.world * ibm;
            let id = Mat4::identity();
            let mut e = 0.0f32;
            for c in 0..4 {
                for r in 0..4 {
                    e = e.max((m[c][r] - id[c][r]).abs());
                }
            }
            worst = Some(worst.map_or(e, |w| w.max(e)));
        }
        worst
    }
}

/// Inline xyz field `i` of table `t`, or `def` when absent.
fn vec3(t: &Table, i: usize, def: [f32; 3]) -> Result<[f32; 3]> {
    match t.field(i) {
        None => Ok(def),
        Some(at) => {
            let f = |k: usize| -> Result<f32> { Ok(f32::from_bits(u32_at(t.buf, at + 4 * k)?)) };
            Ok([f(0)?, f(1)?, f(2)?])
        }
    }
}

fn local_matrix(s: [f32; 3], r: [f32; 3], t: [f32; 3]) -> Mat4 {
    Mat4::from_translation(Vec3::new(t[0], t[1], t[2]))
        * Mat4::from_angle_z(three_d::radians(r[2]))
        * Mat4::from_angle_y(three_d::radians(r[1]))
        * Mat4::from_angle_x(three_d::radians(r[0]))
        * Mat4::from_nonuniform_scale(s[0], s[1], s[2])
}

pub fn parse(buf: &[u8], file: &str) -> Result<Skeleton> {
    let root = Table::root(buf, 0)?;
    let mut bones = Vec::new();
    for n in root.tables(1)? {
        let name = n.string(0)?.unwrap_or_default();
        let (scale, rotation, translation) = match n.table(1)? {
            Some(t) => (
                vec3(&t, 0, [1.0; 3])?,
                vec3(&t, 1, [0.0; 3])?,
                vec3(&t, 2, [0.0; 3])?,
            ),
            None => ([1.0; 3], [0.0; 3], [0.0; 3]),
        };
        let parent = n
            .field(4)
            .map(|_| n.u32(4, 0) as i32)
            .filter(|p| *p >= 0)
            .map(|p| p as usize);
        let joint = n
            .field(5)
            .map(|_| n.u32(5, 0) as i32)
            .filter(|j| *j >= 0)
            .map(|j| j as usize);
        let id = Mat4::identity();
        bones.push(Bone {
            name,
            parent,
            joint,
            scale,
            rotation,
            translation,
            world: id,
            bind: id,
        });
    }
    if bones.is_empty() {
        bail!("{file}: no bones");
    }
    // Parents come before children in the files seen so far, but resolve in any order to be safe.
    let mut done = vec![false; bones.len()];
    let mut order = Vec::with_capacity(bones.len());
    for _ in 0..bones.len() {
        let mut progressed = false;
        for i in 0..bones.len() {
            if done[i] {
                continue;
            }
            let parent_world = match bones[i].parent {
                None => Some(Mat4::identity()),
                Some(p) if p < bones.len() && p != i => done[p].then(|| bones[p].world),
                Some(_) => Some(Mat4::identity()), // bad index: treat as a root
            };
            if let Some(pw) = parent_world {
                let b = &bones[i];
                bones[i].world = pw * local_matrix(b.scale, b.rotation, b.translation);
                done[i] = true;
                order.push(i);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    let mut inverse_bind = Vec::new();
    for j in root.tables(2)? {
        inverse_bind.push(match j.table(2)? {
            Some(m) => {
                let c = |i: usize, w: f32| -> Result<Vec4> {
                    let v = vec3(&m, i, [0.0; 3])?;
                    Ok(Vec4::new(v[0], v[1], v[2], w))
                };
                Some(Mat4::from_cols(
                    c(0, 0.0)?,
                    c(1, 0.0)?,
                    c(2, 0.0)?,
                    c(3, 1.0)?,
                ))
            }
            None => None,
        });
    }
    for &i in &order {
        let ibm = bones[i]
            .joint
            .and_then(|j| inverse_bind.get(j).copied().flatten());
        bones[i].bind = match ibm.and_then(|m| m.invert()) {
            Some(m) => m,
            None => {
                let pb = bones[i]
                    .parent
                    .filter(|p| *p < bones.len())
                    .map(|p| bones[p].bind)
                    .unwrap_or(Mat4::identity());
                let b = &bones[i];
                pb * local_matrix(b.scale, b.rotation, b.translation)
            }
        };
    }
    let mut joint_bone = vec![None; inverse_bind.len()];
    for (i, b) in bones.iter().enumerate() {
        if let Some(j) = b.joint.filter(|j| *j < joint_bone.len()) {
            joint_bone[j] = Some(i);
        }
    }
    Ok(Skeleton {
        file: file.to_string(),
        bones,
        inverse_bind,
        order,
        joint_bone,
    })
}

/// The model's skeleton file (trmdl slot 2), if it has one.
pub fn load(ar: &mut Archive, dir: &str, name: &str) -> Result<Skeleton> {
    let path = crate::model::resolve(dir, name);
    parse(&ar.read(&path)?, name)
}
