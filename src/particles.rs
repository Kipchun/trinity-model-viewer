//! Plays a `.ptcl` effect (parsed by `vfxb.rs`) in the 3D view: a CPU particle simulation at the game's 60 frames
//! per second, drawn as textured quads built each frame.
//!
//! What is used from each emitter: start, duration, rate and interval; the volume the particles start in; speed
//! outwards and along a direction with spread; gravity and air resistance; life; scale with its random and scale
//! keys; colour 0 and alpha 0 (fixed or keyed) times the emitter colour and colour scale; rotation; the first
//! texture with its sprite sheet, pattern animation and UV scroll; blend type; particle type (billboard, Y
//! billboard, flat XY / XZ polygon, along the velocity). Child emitters are started at each parent particle.
//!
//! Not yet: colour 1 / alpha 1 and the other two textures (the game mixes them in shader "combiners"),
//! primitives (mesh particles), the emitter's attached blocks (`CADP`, `FCOL`, ...), fields, emission by
//! distance, and the game's exact random numbers. So effects look close, not exact.

use crate::texture;
use crate::vfxb::{self, ColorType, Emitter, Ptcl};
use std::collections::HashMap;
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

#[derive(Clone)]
pub struct ParticleMaterial {
    texture: Option<Texture2DRef>,
    alpha_only: bool,
    blend: u8,
    color_scale: f32,
}

impl Material for ParticleMaterial {
    fn fragment_shader_source(&self, _lights: &[&dyn Light]) -> String {
        include_str!("shaders/particle.frag").to_string()
    }

    fn id(&self) -> EffectMaterialId {
        // public-use range; terrain 0x0100, eye 0x0101, water uses its own
        EffectMaterialId(0x0110)
    }

    fn use_uniforms(&self, program: &Program, _viewer: &dyn Viewer, _lights: &[&dyn Light]) {
        if let Some(t) = &self.texture {
            program.use_texture("tex", t);
        }
        program.use_uniform_if_required("useTex", self.texture.is_some() as i32);
        program.use_uniform_if_required("alphaOnly", self.alpha_only as i32);
        program.use_uniform_if_required("colorScale", self.color_scale);
        program.use_uniform_if_required("premultiplyForAdd", (self.blend == 1) as i32);
    }

    fn render_states(&self) -> RenderStates {
        use BlendMultiplierType as M;
        let blend = match self.blend {
            // add: rgb already weighted by alpha in the shader
            1 => Blend::Enabled {
                source_rgb_multiplier: M::One,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::One,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::Add,
                alpha_equation: BlendEquationType::Add,
            },
            // subtract
            2 => Blend::Enabled {
                source_rgb_multiplier: M::SrcAlpha,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::One,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::ReverseSubtract,
                alpha_equation: BlendEquationType::Add,
            },
            // screen
            3 => Blend::Enabled {
                source_rgb_multiplier: M::OneMinusDstColor,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::One,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::Add,
                alpha_equation: BlendEquationType::Add,
            },
            // multiply
            4 => Blend::Enabled {
                source_rgb_multiplier: M::DstColor,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::Zero,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::Add,
                alpha_equation: BlendEquationType::Add,
            },
            _ => Blend::TRANSPARENCY,
        };
        RenderStates {
            write_mask: WriteMask::COLOR,
            blend,
            cull: Cull::None,
            ..Default::default()
        }
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Transparent
    }
}

/// An uploaded texture and what its channels hold.
type Tex = (Texture2DRef, TexKind);

/// What a texture's channels hold, as far as can be told without the game's combiners.
#[derive(Clone, Copy, PartialEq)]
enum TexKind {
    /// Colour and alpha.
    Rgba,
    /// Colour, alpha all opaque.
    NoAlpha,
    /// One channel (BC4, R8): a shape.
    Mask,
}

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
    material: ParticleMaterial,
    /// Emitter rotation and scale, then translation (as a matrix).
    transform: Mat4,
}

struct Particle {
    id: u64,
    node: usize,
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    scale: Vec3,
    rot: f32,
    rot_add: f32,
    color: Vec3,
    random_frame: u32,
    uv_offset: Vec2,
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
}

fn wrapping(mode: u8) -> Wrapping {
    match mode {
        0 => Wrapping::MirroredRepeat,
        2 => Wrapping::ClampToEdge,
        _ => Wrapping::Repeat,
    }
}

fn euler(r: [f32; 3]) -> Mat4 {
    Mat4::from_angle_z(radians(r[2]))
        * Mat4::from_angle_y(radians(r[1]))
        * Mat4::from_angle_x(radians(r[0]))
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
        let mut v = EffectView {
            path: path.to_string(),
            ptcl,
            set: 0,
            nodes: Vec::new(),
            warnings: Vec::new(),
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
            let s0 = &e.samplers[0];
            let tex = if s0.has_texture() {
                v.texture(context, s0.texture, s0.wrap)
            } else {
                None
            };
            let scale = Vec3::from(e.emitter_scale);
            let scale = if scale.magnitude2() == 0.0 {
                vec3(1.0, 1.0, 1.0)
            } else {
                scale
            };
            let transform = Mat4::from_translation(Vec3::from(e.translate))
                * euler(e.emitter_rotate)
                * Mat4::from_nonuniform_scale(scale.x, scale.y, scale.z);
            let material = ParticleMaterial {
                // single-channel textures are masks; so are colour textures without alpha on alpha-blended
                // emitters (their shapes sit in red / green, picked by the game's combiner)
                alpha_only: tex.as_ref().is_some_and(|t| {
                    t.1 == TexKind::Mask || (t.1 == TexKind::NoAlpha && e.blend != 1)
                }),
                texture: tex.map(|t| t.0),
                blend: e.blend,
                color_scale: if e.color_scale > 0.0 {
                    e.color_scale
                } else {
                    1.0
                },
            };
            let i = v.nodes.len();
            v.nodes.push(Node {
                em: e,
                parent,
                depth,
                enabled: true,
                material,
                transform,
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
            .and_then(|i| texture::decode_bntx_texture(&self.bntx, i, 0));
        let t = match result {
            Ok(img) => {
                let kind = if img.format.starts_with("BC4")
                    || img.format.starts_with("R8 ")
                    || img.format == "R8"
                {
                    TexKind::Mask
                } else if img.has_alpha() {
                    TexKind::Rgba
                } else {
                    TexKind::NoAlpha
                };
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
                Some((Texture2DRef::from_texture(tex), kind))
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
                    let n = (em.rate + self.rng.f() * em.rate_random).max(0.0);
                    let count = n.floor() as usize + (self.rng.f() < n.fract()) as usize;
                    for _ in 0..count.min(500) {
                        new_particles.push((inst.node, inst.origin));
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
        for (node, origin) in new_particles {
            if self.particles.len() >= MAX_PARTICLES {
                break;
            }
            self.spawn(node, origin);
        }

        // move and age
        let mut alive: HashMap<u64, Vec3> = HashMap::new();
        let track = self.instances.iter().any(|i| i.parent.is_some());
        for p in &mut self.particles {
            let em = &self.nodes[p.node].em;
            let g = Vec3::from(em.gravity_dir) * em.gravity_scale;
            p.vel += g * dt;
            let air = if em.air_resistance > 0.0 {
                em.air_resistance
            } else {
                1.0
            };
            p.vel *= air.powf(dt);
            p.pos += p.vel * dt;
            p.rot += p.rot_add * dt;
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

    fn spawn(&mut self, node: usize, origin: Vec3) {
        let n = &self.nodes[node];
        let em = &n.em;
        let rng = &mut self.rng;
        let r = Vec3::from(em.volume_radius).mul_element_wise(Vec3::from(em.volume_scale));
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
        let vel = (outward * em.speed_out + dir * em.speed_dir) * speed_mul;
        // into the emitter's frame
        let pos = origin + (n.transform * local.extend(1.0)).truncate();
        let vel = (n.transform * vel.extend(0.0)).truncate();
        let life = if em.infinite_life {
            f32::INFINITY
        } else {
            (em.life.max(1) as f32 * (1.0 - rng.f() * em.life_random.clamp(0, 100) as f32 / 100.0))
                .max(1.0)
        };
        let scale_mul = (1.0 - rng.f() * em.scale_random.clamp(0.0, 100.0) / 100.0).max(0.0);
        let color = match em.color0_type {
            ColorType::Random if !em.color0_keys.0.is_empty() => {
                let k = (rng.f() * em.color0_keys.0.len() as f32) as usize;
                Vec3::from(em.color0_keys.0[k.min(em.color0_keys.0.len() - 1)].value)
            }
            _ => Vec3::from(em.color0),
        };
        let s0 = &em.samplers[0];
        let cells = (s0.div[0] * s0.div[1]).max(1.0) as u32;
        let random_frame =
            (rng.next() % (if s0.frames > 0 { s0.frames } else { cells }) as u64) as u32;
        let uv_offset = vec2(rng.s() * s0.scroll_random[0], rng.s() * s0.scroll_random[1]);
        let id = self.next_id;
        self.next_id += 1;
        let p = Particle {
            id,
            node,
            pos,
            vel,
            age: 0.0,
            life,
            scale: Vec3::from(em.scale) * scale_mul,
            rot: em.rotation[2] + rng.s() * em.rotation_random[2],
            rot_add: em.rotation_add[2] + rng.s() * em.rotation_add_random[2],
            color,
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
        let pad = |p: &Particle| p.scale.x.abs().max(p.scale.y.abs()) * 0.5;
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

    /// Draws the particles: one mesh per emitter, quads facing the camera (or as the particle type says).
    pub fn render(&self, context: &Context, target: &RenderTarget, camera: &Camera) {
        let view = camera.view();
        let cam_right = vec3(view.x.x, view.y.x, view.z.x);
        let cam_up = vec3(view.x.y, view.y.y, view.z.y);
        let cam_pos = camera.position();
        let mut by_node: Vec<Vec<&Particle>> = (0..self.nodes.len()).map(|_| Vec::new()).collect();
        for p in &self.particles {
            if self.nodes[p.node].enabled {
                by_node[p.node].push(p);
            }
        }
        let mut objects = Vec::new();
        for (ni, ps) in by_node.iter().enumerate() {
            if ps.is_empty() {
                continue;
            }
            let node = &self.nodes[ni];
            let em = &node.em;
            let s0 = &em.samplers[0];
            let ec = em.emitter_color0;
            let mut positions = Vec::with_capacity(ps.len() * 6);
            let mut uvs = Vec::with_capacity(ps.len() * 6);
            let mut colors = Vec::with_capacity(ps.len() * 6);
            for p in ps {
                let t = if p.life.is_finite() {
                    (p.age / p.life).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let mut scale = p.scale;
                if let Some(k) = em.scale_keys.at(t) {
                    scale.x *= k[0];
                    scale.y *= k[1];
                }
                let rgb = match em.color0_type {
                    ColorType::Keyed => em.color0_keys.at(t).map_or(p.color, Vec3::from),
                    _ => p.color,
                };
                let a = match em.alpha0_type {
                    ColorType::Keyed => em.alpha0_keys.at(t).map_or(em.alpha0, |k| k[0]),
                    _ => em.alpha0,
                };
                let c = vec4(
                    rgb.x * ec[0],
                    rgb.y * ec[1],
                    rgb.z * ec[2],
                    (a * ec[3]).clamp(0.0, 1.0),
                );
                if c.w <= 0.0 {
                    continue;
                }
                // the quad's axes
                let (right, up) = match em.particle_type {
                    2 => {
                        let r = vec3(cam_right.x, 0.0, cam_right.z);
                        let r = if r.magnitude2() > 1e-8 {
                            r.normalize()
                        } else {
                            vec3(1.0, 0.0, 0.0)
                        };
                        (r, vec3(0.0, 1.0, 0.0))
                    }
                    3 => (
                        (node.transform * vec4(1.0, 0.0, 0.0, 0.0))
                            .truncate()
                            .normalize(),
                        (node.transform * vec4(0.0, 1.0, 0.0, 0.0))
                            .truncate()
                            .normalize(),
                    ),
                    4 => (
                        (node.transform * vec4(1.0, 0.0, 0.0, 0.0))
                            .truncate()
                            .normalize(),
                        (node.transform * vec4(0.0, 0.0, 1.0, 0.0))
                            .truncate()
                            .normalize(),
                    ),
                    5 | 6 if p.vel.magnitude2() > 1e-12 => {
                        let up = p.vel.normalize();
                        let to_cam = cam_pos - p.pos;
                        let r = up.cross(to_cam);
                        let r = if r.magnitude2() > 1e-12 {
                            r.normalize()
                        } else {
                            cam_right
                        };
                        (r, up)
                    }
                    _ => (cam_right, cam_up),
                };
                let (sin, cos) = p.rot.sin_cos();
                let rx = (right * cos + up * sin) * scale.x * 0.5;
                let ry = (up * cos - right * sin) * scale.y * 0.5;
                // sprite sheet cell
                let frames = s0.frames.max(1);
                let step = if s0.frames_per_step > 0.0 {
                    s0.frames_per_step
                } else {
                    1.0
                };
                let idx = match s0.pattern_type {
                    1 => ((t * frames as f32) as u32).min(frames - 1),
                    2 => ((p.age / step) as u32).min(frames - 1),
                    3 => (p.age / step) as u32 % frames,
                    4 => p.random_frame,
                    _ => 0,
                };
                let cell = if s0.pattern_type == 4 || s0.table.is_empty() {
                    idx
                } else {
                    s0.table.get(idx as usize).copied().unwrap_or(0).max(0) as u32
                };
                let (dx, dy) = (s0.div[0], s0.div[1]);
                let col = (cell % dx as u32) as f32;
                let row = ((cell / dx as u32) as f32).min(dy - 1.0);
                // mirrored textures are a quarter: UV runs 0..2 and folds back
                let mirror = [s0.wrap[0] == 0, s0.wrap[1] == 0];
                let span = |m: bool| if m { 2.0 } else { 1.0 };
                let us = if s0.uv_scale[0] != 0.0 {
                    s0.uv_scale[0]
                } else {
                    1.0
                } * span(mirror[0]);
                let vs = if s0.uv_scale[1] != 0.0 {
                    s0.uv_scale[1]
                } else {
                    1.0
                } * span(mirror[1]);
                let fold = |x: f32, m: bool| {
                    if m {
                        let r = x.rem_euclid(2.0);
                        if r > 1.0 { 2.0 - r } else { r }
                    } else {
                        x
                    }
                };
                let off = vec2(
                    s0.scroll[0] + s0.scroll_add[0] * p.age,
                    s0.scroll[1] + s0.scroll_add[1] * p.age,
                ) + p.uv_offset;
                let uv = |u: f32, v: f32| {
                    let (lu, lv) = (u * us + off.x, v * vs + off.y);
                    let (lu, lv) = if dx > 1.0 || dy > 1.0 {
                        // a sheet cell cannot wrap into its neighbours: fold or clamp inside it
                        (
                            fold(lu, mirror[0]).clamp(0.0, 1.0),
                            fold(lv, mirror[1]).clamp(0.0, 1.0),
                        )
                    } else {
                        (lu, lv)
                    };
                    vec2((lu + col) / dx, (lv + row) / dy)
                };
                let corners = [
                    (p.pos - rx - ry, uv(0.0, 1.0)),
                    (p.pos + rx - ry, uv(1.0, 1.0)),
                    (p.pos + rx + ry, uv(1.0, 0.0)),
                    (p.pos - rx + ry, uv(0.0, 0.0)),
                ];
                for k in [0, 1, 2, 0, 2, 3] {
                    positions.push(corners[k].0);
                    uvs.push(corners[k].1);
                    colors.push(c);
                }
            }
            if positions.is_empty() {
                continue;
            }
            let cpu = CpuMesh {
                positions: Positions::F32(positions),
                uvs: Some(uvs),
                ..Default::default()
            };
            let mut mesh = Mesh::new(context, &cpu);
            if mesh.set_colors(&colors).is_err() {
                continue;
            }
            objects.push(Gm::new(mesh, node.material.clone()));
        }
        // three-d sorts transparent objects back to front by their bounding boxes
        target.render(camera, objects.iter().map(|o| o as &dyn Object), &[]);
    }
}
