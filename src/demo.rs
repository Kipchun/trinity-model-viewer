//! Demonstrate mode (F7): walk the model around with the keyboard and watch its locomotion and jump animations
//! hand over to each other, the way a game character controller drives them.
//!
//! Most models list the same core set (labels after the five-digit number): defaultwait01_loop, walk01_loop,
//! run01_loop, jumpup01_start, jumpup01_loop, jumpdown01_start, jumpdown01_loop and land02.
//! - Walk and run carry root motion: their `origin` bone travels forward (+Z) a fixed stride per cycle. That
//!   stride over the cycle time is the speed the clip was authored for, so the character moves at exactly that
//!   speed and the motion is taken out of the pose (the feet then stay planted instead of sliding).
//! - On the ground, idle, walk and run are blended by speed (a 1D blend space). Walk and run share one cycle
//!   phase so their footfalls line up while they are mixed.
//! - The jump clips play in place, so the height comes from simple physics: take-off clip, rising loop while
//!   moving up, fall start and falling loop on the way down, landing clip on touching the ground. Each switch
//!   crossfades from the pose shown last.

use crate::skeleton::{Local, Skeleton};
use crate::{Clip, mix_pose};
use three_d::{InnerSpace, Mat4, Vec3, radians, vec3};

pub const IDLE: usize = 0;
pub const WALK: usize = 1;
pub const RUN: usize = 2;
pub const TAKEOFF: usize = 3;
pub const RISE: usize = 4;
pub const FALL_START: usize = 5;
pub const FALL: usize = 6;
pub const LAND: usize = 7;
pub const ROLES: usize = 8;

/// Per role: display name, the game's label for it (after the number), and a looser fallback to look for.
pub const ROLE_INFO: [(&str, &str, &str); ROLES] = [
    ("Idle", "defaultwait01_loop", "wait"),
    ("Walk", "walk01_loop", "walk"),
    ("Run", "run01_loop", "run"),
    ("Take-off", "jumpup01_start", "jumpup"),
    ("Rising", "jumpup01_loop", "jumpup"),
    ("Fall start", "jumpdown01_start", "jumpdown"),
    ("Falling", "jumpdown01_loop", "jumpdown"),
    ("Landing", "land02", "land"),
];

/// Roles whose clip repeats; the others play once.
fn looping(role: usize) -> bool {
    matches!(role, IDLE | WALK | RUN | RISE | FALL)
}

/// For each role, the entry of the model's animation list to use. The list is sorted, so the first match is
/// the lowest-numbered one (the 00xxx set rather than the 10xxx variants).
pub fn pick(list: &[(String, String)]) -> [Option<usize>; ROLES] {
    let name = |l: &str| l.split_once('_').map_or(l, |(_, n)| n).to_string();
    std::array::from_fn(|r| {
        let (_, exact, loose) = ROLE_INFO[r];
        list.iter().position(|(l, _)| name(l) == exact).or_else(|| {
            // the start/loop pairs share their loose name, so the suffix has to agree as well
            list.iter().position(|(l, _)| {
                let n = name(l);
                n.contains(loose)
                    && match r {
                        TAKEOFF | FALL_START => n.ends_with("_start"),
                        RISE | FALL | IDLE | WALK | RUN => n.ends_with("_loop"),
                        _ => true,
                    }
            })
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Ground,
    Takeoff,
    Rising,
    FallStart,
    Falling,
    Landing,
}

impl State {
    /// The role whose clip this state plays (the ground blends several).
    fn role(self) -> Option<usize> {
        match self {
            State::Ground => None,
            State::Takeoff => Some(TAKEOFF),
            State::Rising => Some(RISE),
            State::FallStart => Some(FALL_START),
            State::Falling => Some(FALL),
            State::Landing => Some(LAND),
        }
    }
}

/// The keys held this frame, already turned into a direction.
#[derive(Default, Clone, Copy)]
pub struct Input {
    /// Forward (+1) / back (-1) and right (+1) / left (-1), relative to the camera.
    pub forward: f32,
    pub right: f32,
    pub run: bool,
    /// Pressed since the last update.
    pub jump: bool,
}

/// What the character stands on: the ground height under a point, `None` where there is none (a hole, off the
/// map). See PLAY_IN_SCENES_PLAN.md, step S0.
pub trait Ground {
    fn height(&self, x: f32, z: f32) -> Option<f32>;
}

/// The checker floor: flat at y = 0 everywhere.
pub struct FlatGround;

impl Ground for FlatGround {
    fn height(&self, _x: f32, _z: f32) -> Option<f32> {
        Some(0.0)
    }
}

pub struct Demo {
    pub clips: [Option<Clip>; ROLES],
    pub labels: [Option<String>; ROLES],
    /// The bone that carries root motion (`origin`).
    root: Option<usize>,
    pub pos: Vec3,
    /// Facing, radians around +Y; 0 faces +Z like the clips.
    pub yaw: f32,
    pub speed: f32,
    vy: f32,
    pub state: State,
    state_time: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    /// True when the speeds were measured from the clips' root motion, false when guessed from the model size.
    pub measured: bool,
    jump_speed: f32,
    gravity: f32,
    /// Highest rise the character walks up without jumping, and the deepest drop it follows down a slope
    /// before falling: about a third of its radius (stairs, kerbs), at least 0.3 m.
    step: f32,
    /// Shared cycle position of walk and run, 0 to 1.
    phase: f32,
    /// Weights of idle, walk and run in the last ground pose, for the panel.
    pub weights: [f32; 3],
    /// The held pose being faded out, the time since the switch, and the fade length.
    fade: Option<(Vec<Local>, f32, f32)>,
    last_pose: Option<Vec<Local>>,
}

/// How far the root bone travels per second over one cycle of a clip.
fn root_speed(clip: &mut Clip, s: &Skeleton, root: usize) -> Option<f32> {
    clip.frame = 0.0;
    let a = s.posed_world(&clip.locals(s))[root].w.truncate();
    clip.frame = clip.last();
    let b = s.posed_world(&clip.locals(s))[root].w.truncate();
    clip.frame = 0.0;
    let d = vec3(b.x - a.x, 0.0, b.z - a.z).magnitude();
    let seconds = clip.last() / clip.anim.fps as f32;
    (d > 1e-4 && seconds > 0.0).then_some(d / seconds)
}

impl Demo {
    /// `height` and `radius` are the model's, used for the jump and when the clips give no speed.
    pub fn new(
        mut clips: [Option<Clip>; ROLES],
        labels: [Option<String>; ROLES],
        skel: Option<&Skeleton>,
        height: f32,
        radius: f32,
    ) -> Demo {
        let root = skel.and_then(|s| s.bones.iter().position(|b| b.name == "origin"));
        let mut measure = |r: usize| match (clips[r].as_mut(), skel, root) {
            (Some(c), Some(s), Some(i)) => root_speed(c, s, i),
            _ => None,
        };
        let (walk, run) = (measure(WALK), measure(RUN));
        let measured = walk.is_some() || run.is_some();
        let walk_speed = walk.or(run.map(|r| r * 0.4)).unwrap_or(radius * 1.2);
        let run_speed = run.unwrap_or(walk_speed * 2.5).max(walk_speed * 1.1);
        // jump to half the model's height, reaching the top in 0.38 s
        let (h, t) = (height.max(0.05) * 0.5, 0.38);
        let gravity = 2.0 * h / (t * t);
        Demo {
            clips,
            labels,
            root,
            pos: Vec3::new(0.0, 0.0, 0.0),
            yaw: 0.0,
            speed: 0.0,
            vy: 0.0,
            state: State::Ground,
            state_time: 0.0,
            walk_speed,
            run_speed,
            measured,
            jump_speed: gravity * t,
            step: (radius * 0.35).max(0.3),
            gravity,
            phase: 0.0,
            weights: [1.0, 0.0, 0.0],
            fade: None,
            last_pose: None,
        }
    }

    /// Where the model stands and which way it faces.
    pub fn transform(&self) -> Mat4 {
        Mat4::from_translation(self.pos) * Mat4::from_angle_y(radians(self.yaw))
    }

    fn enter(&mut self, state: State, crossfade: f32) {
        self.state = state;
        self.state_time = 0.0;
        // never fade for longer than half of a clip that plays once, or it would be over before it shows
        let mut len = crossfade;
        if let Some(c) = state.role().and_then(|r| self.clips[r].as_mut()) {
            c.frame = 0.0;
            if !looping(state.role().unwrap_or(IDLE)) {
                len = len.min(0.5 * c.last() / c.anim.fps as f32);
            }
        }
        self.fade = self
            .last_pose
            .clone()
            .filter(|_| len > 0.0)
            .map(|p| (p, 0.0, len));
    }

    /// Moves the character over `ground` and returns the pose for this frame (`None` without a skeleton).
    /// `camera_forward` is the view direction; movement keys are relative to it.
    pub fn update(
        &mut self,
        input: Input,
        dt: f32,
        camera_forward: Vec3,
        skel: Option<&Skeleton>,
        crossfade: f32,
        ground: &dyn Ground,
    ) -> Option<Vec<Local>> {
        let dt = dt.min(0.1);
        self.state_time += dt;
        let airborne = matches!(
            self.state,
            State::Rising | State::FallStart | State::Falling
        );

        // Steering: turn towards the pressed direction, speed up or slow down towards walk or run speed.
        let fwd = vec3(camera_forward.x, 0.0, camera_forward.z);
        let fwd = if fwd.magnitude() > 1e-4 {
            fwd.normalize()
        } else {
            vec3(0.0, 0.0, -1.0)
        };
        let right = vec3(-fwd.z, 0.0, fwd.x);
        let dir = fwd * input.forward + right * input.right;
        let control = match self.state {
            State::Ground | State::Landing => 1.0,
            State::Takeoff => 0.5,
            _ => 0.35,
        };
        let target_speed = if dir.magnitude() > 0.01 {
            let want = dir.x.atan2(dir.z);
            let mut d = want - self.yaw;
            while d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            }
            while d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            let turn = 3.0 * std::f32::consts::PI * control * dt;
            self.yaw += d.clamp(-turn, turn);
            if input.run {
                self.run_speed
            } else {
                self.walk_speed
            }
        } else {
            0.0
        };
        if !airborne {
            let rate = if target_speed > self.speed { 2.5 } else { 4.0 } * self.run_speed * control;
            self.speed += (target_speed - self.speed).clamp(-rate * dt, rate * dt);
        }
        let before = self.pos;
        self.pos += vec3(self.yaw.sin(), 0.0, self.yaw.cos()) * self.speed * dt;
        let under = ground.height(self.pos.x, self.pos.z);
        if !airborne {
            match under {
                // a rise higher than a step is a wall: stay where we were
                Some(h) if h > before.y + self.step => {
                    self.pos = before;
                    self.speed = 0.0;
                }
                // up a step or a slope, or down one within a step: follow the ground
                Some(h) if h >= before.y - self.step => self.pos.y = h,
                // a ledge or a hole: fall
                _ => {
                    self.vy = 0.0;
                    self.enter(State::FallStart, crossfade);
                }
            }
        }

        // The clip of the current state; `done` when a clip that plays once has reached its end.
        let mut done = true;
        if let Some(r) = self.state.role()
            && let Some(c) = self.clips[r].as_mut()
        {
            done = !c.advance(dt, looping(r));
        }

        // Jump state machine.
        match self.state {
            State::Ground => {
                if input.jump {
                    self.enter(State::Takeoff, crossfade);
                }
            }
            State::Takeoff => {
                if done {
                    self.vy = self.jump_speed;
                    self.enter(State::Rising, crossfade);
                }
            }
            State::Rising | State::FallStart | State::Falling => {
                self.vy -= self.gravity * dt;
                self.pos.y += self.vy * dt;
                let floor = under.unwrap_or(f32::MIN);
                if self.pos.y <= floor && self.vy < 0.0 {
                    self.pos.y = floor;
                    self.vy = 0.0;
                    self.enter(State::Landing, crossfade);
                } else if self.state == State::Rising && self.vy <= 0.0 {
                    self.enter(State::FallStart, crossfade);
                } else if self.state == State::FallStart && done {
                    self.enter(State::Falling, crossfade);
                }
            }
            State::Landing => {
                // moving or jumping again cuts the landing short once the impact has shown
                let early = self.state_time > 0.15;
                if input.jump && early {
                    self.enter(State::Takeoff, crossfade);
                } else if done || (target_speed > 0.0 && early) {
                    self.enter(State::Ground, crossfade);
                }
            }
        }

        // Clocks of the ground clips run all the time, so the ground pose is current when it comes back.
        if let Some(c) = self.clips[IDLE].as_mut() {
            c.advance(dt, true);
        }
        self.advance_gait(dt);

        let s = skel?;
        let mut pose = match self.state.role().and_then(|r| self.clips[r].as_ref()) {
            Some(c) => c.locals(s),
            None => self.ground_pose(s),
        };
        // root motion becomes the character's movement, so it is taken out of the pose
        if let Some(r) = self.root.filter(|r| *r < pose.len()) {
            let rest = s.rest_local(r);
            pose[r].translation.x = rest.translation.x;
            pose[r].translation.z = rest.translation.z;
        }
        if let Some((from, t, len)) = self.fade.as_mut() {
            *t += dt;
            let x = (*t / *len).clamp(0.0, 1.0);
            let mut held = from.clone();
            mix_pose(&mut held, &pose, x * x * (3.0 - 2.0 * x));
            if x >= 1.0 {
                self.fade = None;
            }
            pose = held;
        }
        self.last_pose = Some(pose.clone());
        Some(pose)
    }

    /// The speeds of the ground clips that exist, as (role, speed) from slow to fast.
    fn gait_points(&self) -> Vec<(usize, f32)> {
        [(IDLE, 0.0), (WALK, self.walk_speed), (RUN, self.run_speed)]
            .into_iter()
            .filter(|(r, _)| self.clips[*r].is_some())
            .collect()
    }

    /// Weights of idle, walk and run at the current speed: the two clips around it, linearly.
    fn gait_weights(&self) -> [f32; 3] {
        let pts = self.gait_points();
        let mut w = [0.0; 3];
        let Some(&(first, s0)) = pts.first() else {
            return w;
        };
        if self.speed <= s0 {
            w[first] = 1.0;
            return w;
        }
        for pair in pts.windows(2) {
            let ((a, sa), (b, sb)) = (pair[0], pair[1]);
            if self.speed <= sb {
                let t = (self.speed - sa) / (sb - sa).max(1e-6);
                w[a] = 1.0 - t;
                w[b] = t;
                return w;
            }
        }
        w[pts[pts.len() - 1].0] = 1.0;
        w
    }

    /// Moves walk and run along their shared cycle. Mixed, the cycle length is mixed too, so both stay in step;
    /// below walking speed the cycle slows down with the speed (down to half), which keeps the feet from sliding.
    fn advance_gait(&mut self, dt: f32) {
        let cycle = |c: &Option<Clip>| c.as_ref().map(|c| c.last() / c.anim.fps as f32);
        let w = self.gait_weights();
        let rate = match (cycle(&self.clips[WALK]), cycle(&self.clips[RUN])) {
            (Some(tw), Some(tr)) if self.speed > self.walk_speed => {
                let t = w[RUN] / (w[WALK] + w[RUN]).max(1e-6);
                1.0 / (tw + (tr - tw) * t)
            }
            (Some(tw), _) => (self.speed / self.walk_speed).clamp(0.5, 1.0) / tw,
            (None, Some(tr)) => (self.speed / self.run_speed).clamp(0.5, 1.0) / tr,
            (None, None) => 0.0,
        };
        self.phase = (self.phase + rate * dt).fract();
        for r in [WALK, RUN] {
            if let Some(c) = self.clips[r].as_mut() {
                c.frame = self.phase * c.last();
            }
        }
    }

    fn ground_pose(&mut self, s: &Skeleton) -> Vec<Local> {
        let w = self.gait_weights();
        self.weights = w;
        let mut pose: Option<Vec<Local>> = None;
        let mut total = 0.0;
        for r in [IDLE, WALK, RUN] {
            let Some(c) = self.clips[r].as_ref().filter(|_| w[r] > 0.0) else {
                continue;
            };
            total += w[r];
            match pose.as_mut() {
                None => pose = Some(c.locals(s)),
                // running mix: the new clip's share of everything mixed so far
                Some(p) => mix_pose(p, &c.locals(s), w[r] / total),
            }
        }
        pose.unwrap_or_else(|| (0..s.bones.len()).map(|i| s.rest_local(i)).collect())
    }
}
