//! Plays a `.ptcl` effect (parsed by `vfxb.rs`) in the 3D view: a CPU particle simulation at the game's 60 frames
//! per second, drawn as textured quads built each frame.
//!
//! What is used from each emitter: start, duration, rate and interval; the volume the particles start in; speed
//! outwards and along a direction with spread; gravity and air resistance; life; scale with its random and scale
//! keys; colour 0 / alpha 0 and colour 1 / alpha 1 (fixed, random key or keyed) times the emitter colours and
//! colour scale; rotation on three axes with spin and its decay; the three textures with their sprite sheets,
//! pattern animation and UV scroll, mixed by the emitter's combiner (`particle_gpu.rs`); blend type;
//! particle type (billboard, Y billboard, flat XY / XZ polygon, along the velocity). Child emitters are started at
//! each parent particle.
//!
//! Primitives (mesh particles, 1034 of the 2594 emitters): the emitter's `G3PR` mesh (`bfres.rs`) drawn once per
//! particle, instanced (one matrix, the colours and three UV transforms per particle). The particle type turns
//! the mesh as it turns a quad: billboards map its XY plane to the screen, type 3 keeps it in the emitter's frame,
//! type 4 lays its XY plane on XZ, types 5/6 point its Y along the velocity. Billboard quads spin on the screen
//! only; everything else turns on all three axes.
//!
//! Emitter animations (`EA**`) replace the emitter's translation, scale, rate, colours, speeds, gravity, life,
//! particle scale and volume scale over its life. Fields: spin about an axis, velocity jitter, a smooth random
//! offset, pull towards the emitter, magnet, position add, a floor. Stripes are ribbons through each particle's
//! last positions. The fields' formulas are guesses from their values (see `vfxb::Fields`).
//!
//! Particles of an emitter are drawn back to front. Emitters with blend type 5 (screen distortion: they bend what
//! is behind them) start switched off; the viewer cannot draw them.
//!
//! Not yet: the emitter's attached blocks (`CADP`, `FCOL`, ...), fields, emission by distance, the strength of UV
//! distortion (shader type 2), and the game's exact random numbers. So effects look close, not exact.

use crate::bfres;
use crate::particle_gpu::{MeshInstance, ParticleGeometry, ParticleMaterial, QuadVertex};
use crate::texture;
use crate::vfxb::{self, AnimTarget, ColorType, Emitter, Ptcl};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use three_d::*;

/// Per-component product (cgmath's `ElementWise` is not re-exported by three-d).
trait MulEach {
    fn mul_element_wise(self, o: Vec3) -> Vec3;
}

impl MulEach for Vec3 {
    fn mul_element_wise(self, o: Vec3) -> Vec3 {
        vec3(self.x * o.x, self.y * o.y, self.z * o.z)
    }
}

/// Particles alive at most, over all emitters.
const MAX_PARTICLES: usize = 30_000;
/// Child emitter instances at most.
const MAX_INSTANCES: usize = 4_000;

/// An uploaded texture (channel selectors applied) and whether it has two channels (BC5, RG8: distortion maps).
type Tex = (Texture2DRef, bool);

/// Small fast random numbers (xorshift); the game's own sequence is not reproduced.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// 0..1
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// -1..1
    fn s(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
    fn unit(&mut self) -> Vec3 {
        let z = self.s();
        let a = self.f() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        vec3(r * a.cos(), z, r * a.sin())
    }
}

/// One emitter of the set, with its place in the tree.
pub struct Node {
    pub em: Emitter,
    pub parent: Option<usize>,
    pub depth: usize,
    pub enabled: bool,
    /// What is drawn: quads or the primitive mesh, refilled each frame.
    gm: RefCell<Gm<ParticleGeometry, ParticleMaterial>>,
    /// Emitter rotation and scale, then translation (as a matrix).
    transform: Mat4,
    /// The emitter's rotation alone.
    rotation: Mat3,
    /// For mesh particles: the distance of the mesh's farthest vertex from its origin. None = quads.
    mesh_radius: Option<f32>,
    /// No texture and no mesh: the emitter only carries child emitters (`gp1`, `EmitOya`, `*_parent`: 48 of the
    /// 53 such emitters have children). Simulated, not drawn.
    pub spawner: bool,
}

struct Particle {
    id: u64,
    node: usize,
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    scale: Vec3,
    /// Rotation on x, y, z (radians) and its change per frame.
    rot: Vec3,
    rot_add: Vec3,
    color0: Vec3,
    color1: Vec3,
    /// Frame its emitter instance started emitting (emitter animations run from here).
    t0: f32,
    /// Where its emitter was when it was born (fields pull towards / turn about it).
    center: Vec3,
    /// Past positions, newest first (stripes only).
    history: VecDeque<Vec3>,
    /// Per sampler: the frame picked for "random frame" patterns and the random UV offset.
    random_frame: [u32; 3],
    uv_offset: [Vec2; 3],
}

struct Instance {
    node: usize,
    /// Frame the instance started (its emitter's start counts from here).
    born: f32,
    origin: Vec3,
    /// The parent particle it follows (child emitters).
    parent: Option<u64>,
    /// Emissions done so far.
    emissions: u32,
    next_emit: f32,
    done: bool,
}

pub struct EffectView {
    pub path: String,
    pub ptcl: Ptcl,
    pub set: usize,
    pub nodes: Vec<Node>,
    pub warnings: Vec<String>,
    pub playing: bool,
    pub looping: bool,
    pub speed: f32,
    /// Frames since the start (60 per second).
    pub frame: f32,
    /// Restarts after this many frames when looping (from the emitters' timing).
    pub length: f32,
    particles: Vec<Particle>,
    instances: Vec<Instance>,
    rng: Rng,
    next_id: u64,
    /// Decoded textures by GTNT id (None = failed, warned once).
    textures: HashMap<(u64, [u8; 2]), Option<Tex>>,
    bntx: Vec<u8>,
    bntx_names: Vec<String>,
    /// The primitive meshes, by `G3NT` id.
    primitives: HashMap<u64, bfres::Model>,
}

fn wrapping(mode: u8) -> Wrapping {
    match mode {
        0 => Wrapping::MirroredRepeat,
        2 => Wrapping::ClampToEdge,
        _ => Wrapping::Repeat,
    }
}

fn euler(r: [f32; 3]) -> Mat4 {
    Mat4::from(euler3(Vec3::from(r)))
}

/// Rotation on x, then y, then z (radians).
fn euler3(r: Vec3) -> Mat3 {
    Mat3::from_angle_z(radians(r.z))
        * Mat3::from_angle_y(radians(r.y))
        * Mat3::from_angle_x(radians(r.x))
}

/// The camera's axes in world space: right, up, towards the viewer; and its position.
struct CamAxes {
    right: Vec3,
    up: Vec3,
    back: Vec3,
    pos: Vec3,
}

/// The frame a particle is drawn in (columns: the quad's or mesh's x, y, z), before its own rotation.
fn particle_frame(node: &Node, p: &Particle, cam: &CamAxes) -> Mat3 {
    let from = |x: Vec3, y: Vec3| Mat3::from_cols(x, y, x.cross(y));
    match node.em.particle_type {
        2 => {
            let r = vec3(cam.right.x, 0.0, cam.right.z);
            let r = if r.magnitude2() > 1e-8 {
                r.normalize()
            } else {
                vec3(1.0, 0.0, 0.0)
            };
            from(r, vec3(0.0, 1.0, 0.0))
        }
        3 => node.rotation,
        // XY plane laid on XZ: y goes to z
        4 => node.rotation * Mat3::from_angle_x(degrees(90.0)),
        5 | 6 if p.vel.magnitude2() > 1e-12 => {
            let up = p.vel.normalize();
            let r = up.cross(cam.pos - p.pos);
            let r = if r.magnitude2() > 1e-12 {
                r.normalize()
            } else {
                cam.right
            };
            from(r, up)
        }
        _ => Mat3::from_cols(cam.right, cam.up, cam.back),
    }
}

impl Node {
    /// The value of this emitter's animation of `target`, `frame` frames into its life.
    fn anim(&self, target: AnimTarget, frame: f32) -> Option<[f32; 3]> {
        self.em
            .extras
            .anims
            .iter()
            .find(|a| a.target == target)
            .and_then(|a| a.at(frame))
    }

    /// Emitter translation, rotation and scale `frame` frames into its life (animated by `EATR` / `EASL`).
    fn transform_at(&self, frame: f32) -> Mat4 {
        let e = &self.em;
        if e.extras.anims.is_empty() {
            return self.transform;
        }
        let t = self
            .anim(AnimTarget::Translate, frame)
            .unwrap_or(e.translate);
        let s = self
            .anim(AnimTarget::Scale, frame)
            .unwrap_or(e.emitter_scale);
        let s = if s.iter().all(|&v| v == 0.0) {
            [1.0; 3]
        } else {
            s
        };
        Mat4::from_translation(Vec3::from(t))
            * Mat4::from(self.rotation)
            * Mat4::from_nonuniform_scale(s[0], s[1], s[2])
    }
}

/// Smooth noise in -1..1 per axis (sums of sines at the field's frequencies; the game's noise is not known).
fn noise3(t: f32, seed: f32, freq: [f32; 4], weight: [f32; 4]) -> Vec3 {
    let total: f32 = weight.iter().map(|w| w.abs()).sum::<f32>().max(1e-6);
    let axis = |k: f32| {
        (0..4)
            .map(|i| weight[i] * (freq[i] * t * 0.1 + seed * (1.7 + k) + k * 2.1 + i as f32).sin())
            .sum::<f32>()
            / total
    };
    vec3(axis(0.0), axis(1.0), axis(2.0))
}

/// The particle's own rotation: billboard quads spin on the screen only, the rest on all three axes.
fn particle_rotation(node: &Node, p: &Particle) -> Mat3 {
    let billboard = !matches!(node.em.particle_type, 3..=6);
    if billboard && node.mesh_radius.is_none() {
        Mat3::from_angle_z(radians(p.rot.z))
    } else {
        euler3(p.rot)
    }
}

impl EffectView {
    pub fn new(context: &Context, path: &str, bytes: &[u8]) -> anyhow::Result<EffectView> {
        let ptcl = vfxb::parse(bytes)?;
        let bntx = ptcl
            .bntx
            .clone()
            .map(|r| bytes[r].to_vec())
            .unwrap_or_default();
        let bntx_names = if bntx.is_empty() {
            Vec::new()
        } else {
            texture::texture_names(&bntx).unwrap_or_default()
        };
        let mut warnings = Vec::new();
        let models = match &ptcl.bfres {
            Some(r) => bfres::parse(&bytes[r.clone()]).unwrap_or_else(|e| {
                warnings.push(format!("primitives: {e:#}"));
                Vec::new()
            }),
            None => Vec::new(),
        };
        // G3NT lists the primitives in the BFRES's model order
        let primitives = ptcl
            .primitives
            .iter()
            .zip(models)
            .map(|(e, m)| (e.id, m))
            .collect();
        let mut v = EffectView {
            path: path.to_string(),
            ptcl,
            set: 0,
            nodes: Vec::new(),
            warnings,
            playing: true,
            looping: true,
            speed: 1.0,
            frame: 0.0,
            length: 60.0,
            particles: Vec::new(),
            instances: Vec::new(),
            rng: Rng(0x9E37_79B9_7F4A_7C15),
            next_id: 1,
            textures: HashMap::new(),
            bntx,
            bntx_names,
            primitives,
        };
        v.pick_set(context, 0);
        Ok(v)
    }

    /// Shows emitter set `set` of the file (most files hold one).
    pub fn pick_set(&mut self, context: &Context, set: usize) {
        self.set = set.min(self.ptcl.sets.len().saturating_sub(1));
        self.nodes.clear();
        let emitters: Vec<Emitter> = self
            .ptcl
            .sets
            .get(self.set)
            .map(|s| s.emitters.clone())
            .unwrap_or_default();
        fn add(
            v: &mut EffectView,
            context: &Context,
            e: Emitter,
            parent: Option<usize>,
            depth: usize,
        ) {
            let children = e.children.clone();
            let textures: [Option<Tex>; 3] = std::array::from_fn(|n| {
                let s = &e.samplers[n];
                if s.has_texture() {
                    v.texture(context, s.texture, s.wrap)
                } else {
                    None
                }
            });
            let scale = Vec3::from(e.emitter_scale);
            let scale = if scale.magnitude2() == 0.0 {
                vec3(1.0, 1.0, 1.0)
            } else {
                scale
            };
            let transform = Mat4::from_translation(Vec3::from(e.translate))
                * euler(e.emitter_rotate)
                * Mat4::from_nonuniform_scale(scale.x, scale.y, scale.z);
            let mesh = if e.has_primitive() {
                match v.primitives.get(&e.primitive) {
                    Some(m) if !m.indices.is_empty() => Some(m),
                    Some(_) => None,
                    None => {
                        v.warnings.push(format!(
                            "{}: primitive {:#x} not in the file; drawn as quads",
                            e.name, e.primitive
                        ));
                        None
                    }
                }
            } else {
                None
            };
            let no_texture = textures.iter().all(Option::is_none);
            let distort = matches!(e.combiner.shader_type, 1 | 2)
                && textures[0].as_ref().is_some_and(|t| t.1);
            let material = ParticleMaterial {
                textures: textures.map(|t| t.map(|t| t.0)),
                distort,
                blend: e.blend,
                color_scale: if e.color_scale > 0.0 {
                    e.color_scale
                } else {
                    1.0
                },
                combiner: e.combiner,
                vertex_colors: mesh.is_some_and(|m| m.attributes.iter().any(|a| a.name == "_c0")),
            };
            let (geometry, mesh_radius) = match mesh {
                Some(m) => {
                    let radius = m
                        .positions
                        .iter()
                        .map(|&p| Vec3::from(p).magnitude())
                        .fold(0.0f32, f32::max);
                    let positions: Vec<Vec3> = m.positions.iter().map(|&p| Vec3::from(p)).collect();
                    let uvs: Vec<Vec2> = m.uvs.iter().map(|&u| Vec2::from(u)).collect();
                    let colors: Vec<Vec4> = if material.vertex_colors {
                        m.colors.iter().map(|&c| Vec4::from(c)).collect()
                    } else {
                        Vec::new()
                    };
                    (
                        ParticleGeometry::mesh(context, &positions, &uvs, &colors, &m.indices),
                        Some(radius),
                    )
                }
                None => (ParticleGeometry::quads(context), None),
            };
            let spawner = mesh_radius.is_none() && no_texture;
            let rotation = euler3(Vec3::from(e.emitter_rotate));
            // screen distortion (blend 5) bends what is behind the emitter; not drawable here
            let enabled = e.blend != 5;
            let i = v.nodes.len();
            v.nodes.push(Node {
                em: e,
                parent,
                depth,
                enabled,
                gm: RefCell::new(Gm::new(geometry, material)),
                transform,
                rotation,
                mesh_radius,
                spawner,
            });
            for c in children {
                add(v, context, c, Some(i), depth + 1);
            }
        }
        for e in emitters {
            add(self, context, e, None, 0);
        }
        // loop length: the longest emitter's start + duration + life, at least one second, at most 20
        self.length = self
            .nodes
            .iter()
            .filter(|n| n.parent.is_none())
            .map(|n| {
                let emit = if n.em.one_time {
                    n.em.duration as f32
                } else {
                    120.0
                };
                n.em.start as f32 + emit + n.em.life.max(1) as f32
            })
            .fold(60.0f32, f32::max)
            .min(1200.0);
        self.restart();
    }

    fn texture(&mut self, context: &Context, id: u64, wrap: [u8; 2]) -> Option<Tex> {
        if let Some(t) = self.textures.get(&(id, wrap)) {
            return t.clone();
        }
        let name = self.ptcl.texture_names.get(&id).cloned();
        let result = name
            .as_ref()
            .and_then(|n| self.bntx_names.iter().position(|b| b == n))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "texture {id:#x} ({}) not in the file",
                    name.as_deref().unwrap_or("?")
                )
            })
            .and_then(|i| {
                let mut img = texture::decode_bntx_texture(&self.bntx, i, 0)?;
                // sample as the game does: single-channel textures read R in every channel, ...
                texture::apply_channels(&mut img.rgba, texture::channel_select(&self.bntx, i)?);
                Ok(img)
            });
        let t = match result {
            Ok(img) => {
                let two_channel = img.format.starts_with("BC5")
                    || img.format.starts_with("R8G8 ")
                    || img.format == "R8G8";
                let tex = Texture2D::new(
                    context,
                    &CpuTexture {
                        data: TextureData::RgbaU8(img.rgba),
                        width: img.width,
                        height: img.height,
                        wrap_s: wrapping(wrap[0]),
                        wrap_t: wrapping(wrap[1]),
                        ..Default::default()
                    },
                );
                Some((Texture2DRef::from_texture(tex), two_channel))
            }
            Err(e) => {
                self.warnings.push(format!("{e:#}"));
                None
            }
        };
        self.textures.insert((id, wrap), t.clone());
        t
    }

    pub fn restart(&mut self) {
        self.frame = 0.0;
        self.particles.clear();
        self.instances = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].parent.is_none())
            .map(|node| Instance {
                node,
                born: 0.0,
                origin: Vec3::zero(),
                parent: None,
                emissions: 0,
                next_emit: 0.0,
                done: false,
            })
            .collect();
    }

    /// Advances by `seconds` of wall time (scaled by `speed`), in whole game frames.
    pub fn update(&mut self, seconds: f32) {
        if !self.playing {
            return;
        }
        let frames = (seconds * 60.0 * self.speed).clamp(0.0, 10.0);
        // step in whole frames of at most 1 for stable motion
        let mut left = frames;
        while left > 0.0 {
            let dt = left.min(1.0);
            self.step(dt);
            left -= dt;
        }
        if self.looping && self.frame > self.length {
            self.restart();
        }
    }

    /// Runs `frames` frames at once (used to frame the camera on the effect).
    pub fn advance(&mut self, frames: u32) {
        for _ in 0..frames {
            self.step(1.0);
        }
    }

    fn step(&mut self, dt: f32) {
        self.frame += dt;
        // emit
        let mut new_particles = Vec::new();
        let mut i = 0;
        while i < self.instances.len() {
            let inst = &mut self.instances[i];
            i += 1;
            if inst.done {
                continue;
            }
            let node = &self.nodes[inst.node];
            let em = &node.em;
            let t = self.frame - inst.born - em.start as f32;
            if t < 0.0 {
                continue;
            }
            if em.one_time && t > em.duration.max(1) as f32 {
                inst.done = true;
                continue;
            }
            while inst.next_emit <= t && !inst.done {
                if node.enabled {
                    let rate = node.anim(AnimTarget::Rate, t).map_or(em.rate, |v| v[0]);
                    let n = (rate + self.rng.f() * em.rate_random).max(0.0);
                    let count = n.floor() as usize + (self.rng.f() < n.fract()) as usize;
                    for _ in 0..count.min(500) {
                        new_particles.push((inst.node, inst.origin, inst.born + em.start as f32));
                    }
                }
                inst.emissions += 1;
                let gap =
                    (em.interval.max(0) as f32 + 1.0 + self.rng.f() * em.interval_random.max(0.0))
                        .max(1.0);
                inst.next_emit += gap;
                if em.one_time && inst.next_emit > em.duration.max(1) as f32 {
                    inst.done = true;
                }
            }
        }
        for (node, origin, t0) in new_particles {
            if self.particles.len() >= MAX_PARTICLES {
                break;
            }
            self.spawn(node, origin, t0);
        }

        // move and age
        let mut alive: HashMap<u64, Vec3> = HashMap::new();
        let track = self.instances.iter().any(|i| i.parent.is_some());
        for p in &mut self.particles {
            let node = &self.nodes[p.node];
            let em = &node.em;
            let et = self.frame - p.t0;
            let gs = node
                .anim(AnimTarget::Gravity, et)
                .map_or(em.gravity_scale, |v| v[0]);
            let g = Vec3::from(em.gravity_dir) * gs;
            p.vel += g * dt;
            let f = &em.extras.fields;
            if let Some((amp, every)) = f.jitter
                && (p.age as u32).is_multiple_of(every)
            {
                p.vel += vec3(
                    self.rng.s() * amp[0],
                    self.rng.s() * amp[1],
                    self.rng.s() * amp[2],
                ) * dt;
            }
            if let Some((strength, axes)) = f.magnet {
                let d = p.center - p.pos;
                p.vel += vec3(
                    if axes[0] { d.x } else { 0.0 },
                    if axes[1] { d.y } else { 0.0 },
                    if axes[2] { d.z } else { 0.0 },
                ) * strength
                    * dt;
            }
            let air = if em.air_resistance > 0.0 {
                em.air_resistance
            } else {
                1.0
            };
            p.vel *= air.powf(dt);
            p.pos += p.vel * dt;
            if let Some(v) = f.pos_add {
                p.pos += Vec3::from(v) * dt;
            }
            if let Some((angle, axis, outward)) = f.spin {
                // about the emitter's axis through where the emitter was
                let a = match axis {
                    0 => vec3(1.0, 0.0, 0.0),
                    2 => vec3(0.0, 0.0, 1.0),
                    _ => vec3(0.0, 1.0, 0.0),
                };
                let a = (node.rotation * a).normalize();
                let r = p.pos - p.center;
                let along = a * r.dot(a);
                let radial = r - along;
                let turned = Mat3::from_axis_angle(a, radians(angle * dt)) * radial;
                let out = if turned.magnitude2() > 1e-12 {
                    turned.normalize() * outward * dt
                } else {
                    Vec3::zero()
                };
                p.pos = p.center + along + turned + out;
            }
            if let Some((rate, keys)) = &f.converge {
                let t = if p.life.is_finite() {
                    (p.age / p.life).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let k = if keys.0.len() > 1 {
                    keys.at(t).map_or(*rate, |v| v[0])
                } else {
                    *rate
                };
                p.pos += (p.center - p.pos) * (k * dt).clamp(0.0, 1.0);
            }
            if let Some((height, kind, keep)) = f.floor {
                let floor = p.center.y + height;
                if p.pos.y < floor {
                    p.pos.y = floor;
                    p.vel.y = if kind == 0 { -p.vel.y * keep } else { 0.0 };
                }
            }
            if let Some(st) = em.extras.stripe {
                p.history.push_front(p.pos);
                p.history.truncate(st.length as usize);
            }
            p.rot += p.rot_add * dt;
            // spin kept per frame; 0 in some files, taken as none lost
            let keep = em.rotation_decay;
            if keep > 0.0 && keep != 1.0 {
                p.rot_add *= keep.min(1.2).powf(dt);
            }
            p.age += dt;
            if track && p.age < p.life {
                alive.insert(p.id, p.pos);
            }
        }
        self.particles.retain(|p| p.age < p.life);
        // child emitters follow their particle; they stop when it is gone
        for inst in &mut self.instances {
            if let Some(id) = inst.parent {
                match alive.get(&id) {
                    Some(pos) => inst.origin = *pos,
                    None => inst.done = true,
                }
            }
        }
        self.instances.retain(|i| !i.done || i.parent.is_none());
    }

    fn spawn(&mut self, node: usize, origin: Vec3, t0: f32) {
        let n = &self.nodes[node];
        let em = &n.em;
        let et = self.frame - t0;
        let transform = n.transform_at(et);
        let rng = &mut self.rng;
        let volume_scale = n
            .anim(AnimTarget::VolumeScale, et)
            .unwrap_or(em.volume_scale);
        let r = Vec3::from(em.volume_radius).mul_element_wise(Vec3::from(volume_scale));
        let arc = if em.arc > 0.0 {
            em.arc
        } else {
            std::f32::consts::TAU
        };
        let ang = rng.f() * arc;
        let circle = vec3(ang.cos(), 0.0, ang.sin());
        let local = match em.volume {
            1 | 2 => circle.mul_element_wise(r),
            3 => circle.mul_element_wise(r) * rng.f().sqrt(),
            4..=6 => rng.unit().mul_element_wise(r),
            7 => rng.unit().mul_element_wise(r) * rng.f().cbrt(),
            8 => circle.mul_element_wise(r) + vec3(0.0, rng.s() * r.y, 0.0),
            9 => circle.mul_element_wise(r) * rng.f().sqrt() + vec3(0.0, rng.s() * r.y, 0.0),
            10 => {
                let mut v = vec3(rng.s(), rng.s(), rng.s());
                let axis = (rng.f() * 3.0) as usize % 3;
                v[axis] = if rng.f() < 0.5 { -1.0 } else { 1.0 };
                v.mul_element_wise(r)
            }
            11 => vec3(rng.s(), rng.s(), rng.s()).mul_element_wise(r),
            12 | 13 => vec3(0.0, 0.0, rng.s() * r.z),
            14 => vec3(rng.s() * r.x, 0.0, rng.s() * r.z),
            _ => Vec3::zero(),
        };
        let local = local
            + Vec3::from(em.translate_random).mul_element_wise(vec3(rng.s(), rng.s(), rng.s()));
        let outward = if local.magnitude2() > 1e-12 {
            local.normalize()
        } else {
            rng.unit()
        };
        let dir = Vec3::from(em.direction);
        let dir = if dir.magnitude2() > 1e-12 {
            dir.normalize()
        } else {
            vec3(0.0, 1.0, 0.0)
        };
        // spread: a random direction within the cone around `dir`
        let spread = em.spread_deg.to_radians().clamp(0.0, std::f32::consts::PI);
        let dir = if spread > 0.0 {
            let helper = if dir.y.abs() < 0.9 {
                vec3(0.0, 1.0, 0.0)
            } else {
                vec3(1.0, 0.0, 0.0)
            };
            let a = dir.cross(helper).normalize();
            let b = dir.cross(a);
            let cos_t = 1.0 - rng.f() * (1.0 - spread.cos());
            let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
            let phi = rng.f() * std::f32::consts::TAU;
            dir * cos_t + (a * phi.cos() + b * phi.sin()) * sin_t
        } else {
            dir
        };
        let speed_mul = 1.0 + rng.s() * em.speed_random / 100.0;
        let speed_out = n
            .anim(AnimTarget::SpeedOut, et)
            .map_or(em.speed_out, |v| v[0]);
        let speed_dir = n
            .anim(AnimTarget::SpeedDir, et)
            .map_or(em.speed_dir, |v| v[0]);
        let vel = (outward * speed_out + dir * speed_dir) * speed_mul;
        // into the emitter's frame
        let pos = origin + (transform * local.extend(1.0)).truncate();
        let vel = (transform * vel.extend(0.0)).truncate();
        let center = origin + transform.w.truncate();
        let base_life = n
            .anim(AnimTarget::Life, et)
            .map_or(em.life as f32, |v| v[0]);
        let life = if em.infinite_life {
            f32::INFINITY
        } else {
            (base_life.max(1.0) * (1.0 - rng.f() * em.life_random.clamp(0, 100) as f32 / 100.0))
                .max(1.0)
        };
        let scale_anim = Vec3::from(n.anim(AnimTarget::ParticleScale, et).unwrap_or([1.0; 3]));
        let scale_mul = (1.0 - rng.f() * em.scale_random.clamp(0.0, 100.0) / 100.0).max(0.0);
        let mut pick = |ty: ColorType, keys: &vfxb::Keys, fixed: [f32; 3]| match ty {
            ColorType::Random if !keys.0.is_empty() => {
                let k = (rng.f() * keys.0.len() as f32) as usize;
                Vec3::from(keys.0[k.min(keys.0.len() - 1)].value)
            }
            _ => Vec3::from(fixed),
        };
        let color0 = pick(em.color0_type, &em.color0_keys, em.color0);
        let color1 = pick(em.color1_type, &em.color1_keys, em.color1);
        let random_frame = std::array::from_fn(|n| {
            let s = &em.samplers[n];
            let cells = (s.div[0] * s.div[1]).max(1.0) as u32;
            (rng.next() % (if s.frames > 0 { s.frames } else { cells }) as u64) as u32
        });
        let uv_offset = std::array::from_fn(|n| {
            let s = &em.samplers[n];
            vec2(rng.s() * s.scroll_random[0], rng.s() * s.scroll_random[1])
        });
        let id = self.next_id;
        self.next_id += 1;
        let p = Particle {
            id,
            node,
            pos,
            vel,
            age: 0.0,
            life,
            scale: Vec3::from(em.scale).mul_element_wise(scale_anim) * scale_mul,
            rot: Vec3::from(std::array::from_fn::<f32, 3, _>(|i| {
                em.rotation[i] + rng.s() * em.rotation_random[i]
            })),
            rot_add: Vec3::from(std::array::from_fn::<f32, 3, _>(|i| {
                em.rotation_add[i] + rng.s() * em.rotation_add_random[i]
            })),
            color0,
            color1,
            t0,
            center,
            history: VecDeque::new(),
            random_frame,
            uv_offset,
        };
        // child emitters start at this particle
        let children: Vec<usize> = (0..self.nodes.len())
            .filter(|&c| self.nodes[c].parent == Some(node))
            .collect();
        for c in children {
            if self.instances.len() < MAX_INSTANCES {
                self.instances.push(Instance {
                    node: c,
                    born: self.frame,
                    origin: pos,
                    parent: Some(id),
                    emissions: 0,
                    next_emit: 0.0,
                    done: false,
                });
            }
        }
        self.particles.push(p);
    }

    pub fn particle_count(&self) -> usize {
        self.particles.len()
    }

    /// Bounds of the particles alive now (for framing the camera).
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.particles.iter().filter(|p| self.nodes[p.node].enabled);
        let first = it.next()?;
        let pad = |p: &Particle| {
            let r = self.nodes[p.node].mesh_radius.unwrap_or(0.5);
            p.scale.x.abs().max(p.scale.y.abs()).max(p.scale.z.abs()) * r
        };
        let mut min = first.pos - {
            let v = pad(first);
            vec3(v, v, v)
        };
        let mut max = first.pos + {
            let v = pad(first);
            vec3(v, v, v)
        };
        for p in it {
            let s = {
                let v = pad(p);
                vec3(v, v, v)
            };
            min = min.zip(p.pos - s, f32::min);
            max = max.zip(p.pos + s, f32::max);
        }
        Some((min, max))
    }

    /// Draws the particles: per emitter, quads facing the camera (or as the particle type says) or its primitive mesh
    /// once per particle, back to front.
    pub fn render(&self, _context: &Context, target: &RenderTarget, camera: &Camera) {
        let view = camera.view();
        let right = vec3(view.x.x, view.y.x, view.z.x);
        let up = vec3(view.x.y, view.y.y, view.z.y);
        let cam = CamAxes {
            right,
            up,
            back: right.cross(up),
            pos: camera.position(),
        };
        let mut by_node: Vec<Vec<&Particle>> = (0..self.nodes.len()).map(|_| Vec::new()).collect();
        for p in &self.particles {
            let n = &self.nodes[p.node];
            if n.enabled && !n.spawner {
                by_node[p.node].push(p);
            }
        }
        let mut drawn = Vec::new();
        for (ni, ps) in by_node.iter_mut().enumerate() {
            if ps.is_empty() {
                continue;
            }
            // back to front within the emitter
            ps.sort_by(|a, b| {
                b.pos
                    .distance2(cam.pos)
                    .partial_cmp(&a.pos.distance2(cam.pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let node = &self.nodes[ni];
            let em = &node.em;
            let mut quads = Vec::new();
            let mut instances = Vec::new();
            let mut lo = vec3(f32::MAX, f32::MAX, f32::MAX);
            let mut hi = -lo;
            for p in ps.iter() {
                let t = if p.life.is_finite() {
                    (p.age / p.life).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let mut scale = p.scale;
                if let Some(k) = em.scale_keys.at(t) {
                    scale.x *= k[0];
                    scale.y *= k[1];
                    scale.z *= k[2];
                }
                let keyed = |ty: ColorType, keys: &vfxb::Keys, v: Vec3| match ty {
                    ColorType::Keyed => keys.at(t).map_or(v, Vec3::from),
                    _ => v,
                };
                let alpha = |ty: ColorType, keys: &vfxb::Keys, v: f32| match ty {
                    ColorType::Keyed => keys.at(t).map_or(v, |k| k[0]),
                    _ => v,
                };
                let rgb0 = keyed(em.color0_type, &em.color0_keys, p.color0);
                let rgb1 = keyed(em.color1_type, &em.color1_keys, p.color1);
                let a0 = alpha(em.alpha0_type, &em.alpha0_keys, em.alpha0);
                let a1 = alpha(em.alpha1_type, &em.alpha1_keys, em.alpha1);
                // emitter colours, animated over the emitter's life (EAC0 / EAC1 / EAA0)
                let et = self.frame - p.t0;
                let mut ec0 = em.emitter_color0;
                let mut ec1 = em.emitter_color1;
                if let Some(v) = node.anim(AnimTarget::Color0, et) {
                    ec0[..3].copy_from_slice(&v);
                }
                if let Some(v) = node.anim(AnimTarget::Color1, et) {
                    ec1[..3].copy_from_slice(&v);
                }
                if let Some(v) = node.anim(AnimTarget::Alpha0, et) {
                    ec0[3] = v[0];
                }
                let c0 = vec4(
                    rgb0.x * ec0[0],
                    rgb0.y * ec0[1],
                    rgb0.z * ec0[2],
                    (a0 * ec0[3]).clamp(0.0, 1.0),
                );
                let c1 = vec4(
                    rgb1.x * ec1[0],
                    rgb1.y * ec1[1],
                    rgb1.z * ec1[2],
                    (a1 * ec1[3]).clamp(0.0, 1.0),
                );
                // every alpha process scales by alpha 0
                if c0.w <= 0.0 {
                    continue;
                }
                let sheets: [SheetUv; 3] =
                    std::array::from_fn(|n| SheetUv::new(&em.samplers[n], p, t, n));
                // FRND: a smooth random offset of where the particle is drawn
                let pos = match em.extras.fields.noise {
                    Some((amp, freq, weight)) => {
                        p.pos
                            + noise3(p.age, p.id as f32 * 0.618, freq, weight)
                                .mul_element_wise(Vec3::from(amp))
                    }
                    None => p.pos,
                };
                // stripes: a ribbon through the particle's last positions, texture U along it
                if em.extras.stripe.is_some() && p.history.len() >= 2 {
                    let h = &p.history;
                    let n = h.len();
                    let side = |i: usize| {
                        let d = h[(i + 1).min(n - 1)] - h[i.saturating_sub(1)];
                        let s = d.cross(cam.pos - h[i]);
                        if s.magnitude2() > 1e-12 {
                            s.normalize() * scale.x * 0.5
                        } else {
                            cam.up * scale.x * 0.5
                        }
                    };
                    let corner = |pos: Vec3, u: f32, v: f32| QuadVertex {
                        pos,
                        c0,
                        c1,
                        uv: std::array::from_fn(|k| sheets[k].at(u, v)),
                    };
                    let mut prev = (h[0], side(0), 0.0);
                    for (i, &at) in h.iter().enumerate().skip(1) {
                        let cur = (at, side(i), i as f32 / (n - 1) as f32);
                        let quad = [
                            corner(prev.0 - prev.1, prev.2, 1.0),
                            corner(cur.0 - cur.1, cur.2, 1.0),
                            corner(cur.0 + cur.1, cur.2, 0.0),
                            corner(prev.0 + prev.1, prev.2, 0.0),
                        ];
                        for c in &quad {
                            lo = lo.zip(c.pos, f32::min);
                            hi = hi.zip(c.pos, f32::max);
                        }
                        for k in [0, 1, 2, 0, 2, 3] {
                            quads.push(quad[k]);
                        }
                        prev = cur;
                    }
                    continue;
                }
                let frame = particle_frame(node, p, &cam) * particle_rotation(node, p);
                if let Some(radius) = node.mesh_radius {
                    instances.push(MeshInstance {
                        transform: Mat4::from_translation(pos)
                            * Mat4::from(frame)
                            * Mat4::from_nonuniform_scale(scale.x, scale.y, scale.z),
                        c0,
                        c1,
                        uv: std::array::from_fn(|n| sheets[n].transform()),
                    });
                    let r = scale.x.abs().max(scale.y.abs()).max(scale.z.abs()) * radius;
                    lo = lo.zip(pos - vec3(r, r, r), f32::min);
                    hi = hi.zip(pos + vec3(r, r, r), f32::max);
                    continue;
                }
                let rx = frame.x * scale.x * 0.5;
                let ry = frame.y * scale.y * 0.5;
                let corner = |pos: Vec3, u: f32, v: f32| QuadVertex {
                    pos,
                    c0,
                    c1,
                    uv: std::array::from_fn(|n| sheets[n].at(u, v)),
                };
                let corners = [
                    corner(pos - rx - ry, 0.0, 1.0),
                    corner(pos + rx - ry, 1.0, 1.0),
                    corner(pos + rx + ry, 1.0, 0.0),
                    corner(pos - rx + ry, 0.0, 0.0),
                ];
                for c in &corners {
                    lo = lo.zip(c.pos, f32::min);
                    hi = hi.zip(c.pos, f32::max);
                }
                for k in [0, 1, 2, 0, 2, 3] {
                    quads.push(corners[k]);
                }
            }
            if quads.is_empty() && instances.is_empty() {
                continue;
            }
            let aabb = AxisAlignedBoundingBox::new_with_positions(&[lo, hi]);
            let mut gm = node.gm.borrow_mut();
            if node.mesh_radius.is_some() {
                gm.geometry.set_instances(&instances, aabb);
            } else {
                gm.geometry.set_quads(&quads, aabb);
            }
            drop(gm);
            let gm = node.gm.borrow();
            if !gm.geometry.is_empty() {
                drawn.push(gm);
            }
        }
        // three-d sorts transparent objects back to front by their bounding boxes
        target.render(camera, drawn.iter().map(|g| &**g as &dyn Object), &[]);
    }
}

/// Where a particle's quad or mesh UVs land in one sampler's texture: UV scale and scroll, sprite sheet cell
/// (pattern animation), mirror folding.
struct SheetUv {
    /// UV scale (doubled for mirrored textures: they are a quarter, shown over UV 0..2).
    scale: Vec2,
    offset: Vec2,
    cell: Vec2,
    div: Vec2,
    mirror: [bool; 2],
}

impl SheetUv {
    fn new(s: &vfxb::Sampler, p: &Particle, t: f32, n: usize) -> SheetUv {
        let mirror = [s.wrap[0] == 0, s.wrap[1] == 0];
        let span = |m: bool| if m { 2.0 } else { 1.0 };
        let nonzero = |x: f32| if x != 0.0 { x } else { 1.0 };
        let frames = s.frames.max(1);
        let step = if s.frames_per_step > 0.0 {
            s.frames_per_step
        } else {
            1.0
        };
        let idx = match s.pattern_type {
            1 => ((t * frames as f32) as u32).min(frames - 1),
            2 => ((p.age / step) as u32).min(frames - 1),
            3 => (p.age / step) as u32 % frames,
            4 => p.random_frame[n],
            _ => 0,
        };
        let cell = if s.pattern_type == 4 || s.table.is_empty() {
            idx
        } else {
            s.table.get(idx as usize).copied().unwrap_or(0).max(0) as u32
        };
        let (dx, dy) = (s.div[0], s.div[1]);
        SheetUv {
            scale: vec2(
                nonzero(s.uv_scale[0]) * span(mirror[0]),
                nonzero(s.uv_scale[1]) * span(mirror[1]),
            ),
            offset: vec2(
                s.scroll[0] + s.scroll_add[0] * p.age,
                s.scroll[1] + s.scroll_add[1] * p.age,
            ) + p.uv_offset[n],
            cell: vec2(
                (cell % dx as u32) as f32,
                ((cell / dx as u32) as f32).min(dy - 1.0),
            ),
            div: vec2(dx, dy),
            mirror,
        }
    }

    /// The texture UV of quad corner (u, v).
    fn at(&self, u: f32, v: f32) -> Vec2 {
        let fold = |x: f32, m: bool| {
            if m {
                let r = x.rem_euclid(2.0);
                if r > 1.0 { 2.0 - r } else { r }
            } else {
                x
            }
        };
        let (lu, lv) = (
            u * self.scale.x + self.offset.x,
            v * self.scale.y + self.offset.y,
        );
        let (lu, lv) = if self.div.x > 1.0 || self.div.y > 1.0 {
            // a sheet cell cannot wrap into its neighbours: fold or clamp inside it
            (
                fold(lu, self.mirror[0]).clamp(0.0, 1.0),
                fold(lv, self.mirror[1]).clamp(0.0, 1.0),
            )
        } else {
            (lu, lv)
        };
        vec2(
            (lu + self.cell.x) / self.div.x,
            (lv + self.cell.y) / self.div.y,
        )
    }

    /// The same as a UV transform for mesh UVs (scale xy, offset zw; no folding inside a cell, unlike quads).
    fn transform(&self) -> Vec4 {
        vec4(
            self.scale.x / self.div.x,
            self.scale.y / self.div.y,
            (self.offset.x + self.cell.x) / self.div.x,
            (self.offset.y + self.cell.y) / self.div.y,
        )
    }
}
