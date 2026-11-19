//! Playing a cutscene in the 3D view (step C1 of `.claude/docs/CUTSCENE_PLAN.md`): its scenes and its camera.
//!
//! - Scenes: the root scene `world/scene/parts/<kind>/<name>_/<name>_0.trscn` (for `demo/<kind>/<name>/<name>.trtml`)
//!   and each of its `SubScene`s, shown from the start; and every scene a `Scene/LoadSubScene` clip names, shown from
//!   that clip's frame. `Scene/TerminateSubScene` (by name: the file stem without `_0`) hides one again. Each scene
//!   is one placed-model group (`plants.rs`), built one per frame after the cutscene opens (the scenes shown now
//!   first, then by when they are first shown); playback waits while a scene that should show is not built.
//! - Camera: the shot (`.trcma`, `camera_anim.rs`) of the last `CameraAnimation/Play` clip started, sampled at the
//!   time since that clip's start; shots are found by name among the root scene's `CameraAnimationComponent`s
//!   (data slot 1: path), else next to the timeline.
//! - Tracks with a condition other than `None` play only for version A (`PackageType` = `PackageTypeA`).
//! - Cast (step C2, `cast.rs`): the actors every scene places (the root scene's too), built one per frame after
//!   the scenes. An actor shows while its scene does and its last `Model/SetVisibility` says so; it stands at
//!   its last `GameObject/SetTranslation` (with no target node; else where its scene puts it) and plays the clip
//!   of its last `Animation/SetInt int_cutnum` from that clip's frame.
//!
//! Not yet: rotations and scales, attachments, lights, fades, effects, sound, the head's own clips.

use crate::archive::Archive;
use crate::camera_anim::{CameraAnim, Shot};
use crate::cast::{self, Actor, Spec};
use crate::plants::{self, CpuModel, Group, LodSettings};
use crate::scene;
use crate::timeline::{FPS, Timeline, Track, Value};
use std::collections::HashMap;
use std::rc::Rc;
use three_d::*;

struct SceneGroup {
    /// File stem without `_0` (`d010_a_sch_room03`): the name `Scene/TerminateSubScene` uses.
    name: String,
    path: String,
    /// `None` until built (`build_next`).
    group: Option<Group>,
    /// Shown before the first load / end clip (sub-scenes of the root scene).
    from_start: bool,
    /// (frame, shown) from the timeline, sorted.
    events: Vec<(u32, bool)>,
}

impl SceneGroup {
    fn shown(&self, frame: f32) -> bool {
        self.events
            .iter()
            .rev()
            .find(|(f, _)| *f as f32 <= frame)
            .map_or(self.from_start, |(_, on)| *on)
    }
}

/// One actor and what the timeline does with it.
struct ActorSlot {
    spec: Spec,
    /// Index into `scenes`; `None` for the root scene's actors (always there).
    scene: Option<usize>,
    /// `None` until built; an error if it could not be.
    actor: Option<Result<Actor, String>>,
    /// (frame, cut) from `Animation/SetInt int_cutnum`, sorted.
    cuts: Vec<(u32, usize)>,
    /// (frame, shown) from `Model/SetVisibility`, sorted.
    visible: Vec<(u32, bool)>,
    /// (frame, position) from `GameObject/SetTranslation`, sorted.
    moves: Vec<(u32, Vec3)>,
}

/// The cuts the timeline asks of an actor, each with the shot it starts with (`cast_plan`).
pub type CutPlan = Vec<(usize, Option<String>)>;

/// An actor's state for the panel (`actor_states`).
pub struct ActorState {
    /// The track name (`d010.tr0630_A_Mod`).
    pub key: String,
    /// `None` while loading; `Some(false)` if it failed.
    pub built: Option<bool>,
    pub shown: bool,
    /// The cut playing (0: none set yet).
    pub cut: usize,
    /// Warnings, or the error.
    pub notes: Vec<String>,
}

/// The last value at or before `frame` of a sorted (frame, value) list.
fn last_at<T: Copy>(list: &[(u32, T)], frame: f32) -> Option<(u32, T)> {
    list.iter().rev().find(|(f, _)| *f as f32 <= frame).copied()
}

pub struct CutscenePlayer {
    pub name: String,
    /// Current frame (60 per second).
    pub frame: f32,
    pub frames: u32,
    pub playing: bool,
    pub looping: bool,
    pub speed: f32,
    /// true: the view follows the cutscene's camera; false: free orbit camera.
    pub use_camera: bool,
    scenes: Vec<SceneGroup>,
    /// Models loaded so far, shared between the scenes' groups.
    cache: HashMap<String, Result<Rc<CpuModel>, String>>,
    cameras: HashMap<String, CameraAnim>,
    /// (start frame, shot name), sorted.
    shots: Vec<(u32, String)>,
    /// (start, end, message file, label), sorted.
    pub messages: Vec<(u32, u32, String, String)>,
    actors: Vec<ActorSlot>,
    pub warnings: Vec<String>,
}

/// The scene path a timeline's sub-scene path stands for: as written, else with `_0` (as stored).
fn stored_scene(ar: &Archive, path: &str) -> String {
    if ar.contains(path) {
        return path.to_string();
    }
    path.strip_suffix(".trscn")
        .map_or(path.to_string(), |s| format!("{s}_0.trscn"))
}

/// `.../d010_tr0040_0.trscn` -> `d010_tr0040`.
fn scene_name(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.strip_suffix(".trscn").unwrap_or(file);
    stem.strip_suffix("_0").unwrap_or(stem).to_string()
}

/// Whether a track plays in version A (tracks without conditions always do).
fn plays(t: &Track) -> bool {
    t.conditions
        .iter()
        .all(|(k, v)| v == "None" || k != "PackageType" || v == "PackageTypeA")
}

impl CutscenePlayer {
    /// Opens the cutscene whose timeline is at `path` (`demo/ev/d010/d010.trtml`).
    pub fn new(ar: &mut Archive, path: &str, t: &Timeline) -> CutscenePlayer {
        let mut warnings = Vec::new();
        let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
        let name = file.trim_end_matches(".trtml").to_string();
        let kind_dir = folder.rsplit_once('/').map_or("", |(d, _)| d);
        let root_path = format!("world/scene/parts/{kind_dir}/{name}_/{name}_0.trscn");

        // the root scene: camera shots and the sub-scenes there from the start
        let mut shot_paths: HashMap<String, String> = HashMap::new();
        let mut start_scenes: Vec<String> = Vec::new();
        let mut specs: Vec<(Option<String>, Spec)> = Vec::new();
        let mut containers: HashMap<String, Vec<String>> = HashMap::new();
        if ar.contains(&root_path) {
            match ar.read(&root_path).and_then(|b| scene::parse(&b)) {
                Ok(root) => {
                    let (sp, co) = cast::specs(ar, &name, &root);
                    specs.extend(sp.into_iter().map(|s| (None, s)));
                    containers.extend(co);
                    for top in &root.nodes {
                        top.walk(&mut Vec::new(), &mut |n, _| {
                            if n.kind == "trinity_CameraAnimationComponent"
                                && let Some(p) = n.table().and_then(|t| t.string(1).ok().flatten())
                            {
                                let stem = p
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(&p)
                                    .trim_end_matches(".trcma");
                                shot_paths.insert(stem.to_string(), p.clone());
                            }
                        });
                    }
                    let c = scene::content(&root);
                    if !c.placements.is_empty() || !c.instancers.is_empty() {
                        start_scenes.push(root_path.clone());
                    }
                    for (rel, _) in c.subscenes {
                        if rel.ends_with(".trscn") {
                            start_scenes.push(scene::resolve_subscene(ar, &root_path, &rel));
                        }
                    }
                }
                Err(e) => warnings.push(format!("{root_path}: {e:#}")),
            }
        } else {
            warnings.push(format!("no root scene at {root_path}"));
        }

        // what the timeline loads, ends and plays
        let mut loaded: Vec<(u32, String)> = Vec::new();
        let mut ended: Vec<(u32, String)> = Vec::new();
        let mut shots = Vec::new();
        let mut messages = Vec::new();
        for tr in t.tracks.iter().filter(|t| plays(t)) {
            for c in tr.clips() {
                let s = |k: &str| {
                    c.param(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                match c.command.as_str() {
                    "Scene/LoadSubScene" => loaded.push((c.start, stored_scene(ar, &s("Path")))),
                    "Scene/TerminateSubScene" => ended.push((c.start, s("SubSceneName"))),
                    "CameraAnimation/Play" if !s("fileName").is_empty() => {
                        shots.push((c.start, s("fileName")))
                    }
                    "Title/Message/ShowMessageWindow" => {
                        messages.push((c.start, c.end, s("MessageDataPath"), s("Label")))
                    }
                    _ => {}
                }
            }
        }
        shots.sort();
        messages.sort();

        // one group per scene, built later
        let mut paths: Vec<(String, bool)> =
            start_scenes.iter().map(|p| (p.clone(), true)).collect();
        for (_, p) in &loaded {
            if !paths.iter().any(|(q, _)| q == p) {
                paths.push((p.clone(), false));
            }
        }
        // scenes the timeline ends or drives objects of (`<scene>.<object>` tracks) but never loads: loaded before
        // it starts (d010's `d010_bg_c01`, the academy outside), so shown from the start if found in its folder
        let mut named: Vec<String> = ended.iter().map(|(_, n)| n.clone()).collect();
        named.extend(
            t.tracks
                .iter()
                .filter_map(|tr| tr.name.split_once('.').map(|(s, _)| s.to_string())),
        );
        named.sort();
        named.dedup();
        for n in named {
            let p = format!("world/scene/parts/{kind_dir}/{name}/{n}_/{n}_0.trscn");
            if !paths.iter().any(|(q, _)| scene_name(q) == n) && ar.contains(&p) {
                paths.push((p, true));
            }
        }
        let mut scenes = Vec::new();
        for (p, from_start) in paths {
            let name = scene_name(&p);
            let mut events: Vec<(u32, bool)> = loaded
                .iter()
                .filter(|(_, q)| *q == p)
                .map(|(f, _)| (*f, true))
                .chain(
                    ended
                        .iter()
                        .filter(|(_, n)| *n == name)
                        .map(|(f, _)| (*f, false)),
                )
                .collect();
            events.sort();
            // the scene's actors
            if p != root_path {
                match ar.read(&p).and_then(|b| scene::parse(&b)) {
                    Ok(sc) => {
                        let (sp, co) = cast::specs(ar, &name, &sc);
                        specs.extend(sp.into_iter().map(|s| (Some(p.clone()), s)));
                        containers.extend(co);
                    }
                    Err(e) => warnings.push(format!("{p}: {e:#}")),
                }
            }
            scenes.push(SceneGroup {
                name,
                path: p,
                group: None,
                from_start,
                events,
            });
        }

        // what the timeline does with each actor, by track name
        let mut actors = Vec::new();
        for (scene_path, mut spec) in specs {
            let key = spec.key();
            // the body's clips: the container and file the timeline names for this actor
            let named = t
                .tracks
                .iter()
                .filter(|tr| tr.name == key)
                .flat_map(|tr| tr.clips())
                .filter(|c| {
                    c.command == "Animation/ChangeCharaCreateAnimation"
                        || c.command == "Animation/AddConfig"
                })
                .find(|c| {
                    // the body: PartsName "default" (characters) or an empty PartName (Pokémon, props)
                    let part = c
                        .param("PartsName")
                        .or(c.param("PartName"))
                        .and_then(|v| v.as_str());
                    part.is_none_or(|p| p == "default" || p.is_empty())
                });
            if let Some(c) = named
                && let Some(list) = c
                    .param("Container")
                    .and_then(|v| v.as_str())
                    .and_then(|k| containers.get(k))
            {
                let file = c.param("FileName").and_then(|v| v.as_str()).unwrap_or("");
                let stem = |p: &str| {
                    p.rsplit('/')
                        .next()
                        .unwrap_or(p)
                        .trim_end_matches(".tracn")
                        .to_string()
                };
                let pick = list
                    .iter()
                    .find(|p| stem(p) == file)
                    .or(list.first())
                    .cloned();
                spec.tracn = pick.into_iter().collect();
            }
            let (mut cuts, mut visible, mut moves) = (Vec::new(), Vec::new(), Vec::new());
            for tr in t.tracks.iter().filter(|tr| tr.name == key && plays(tr)) {
                for c in tr.clips() {
                    match (c.command.as_str(), c.param("Name"), c.param("Target")) {
                        ("Animation/SetInt", Some(Value::Str(n)), _) if n == "int_cutnum" => {
                            if let Some(Value::Int(k)) = c.param("Param") {
                                cuts.push((c.start, (*k).max(0) as usize));
                            }
                        }
                        ("Model/SetVisibility", _, _) => {
                            if let Some(Value::Bool(b)) = c.param("Visibility") {
                                visible.push((c.start, *b));
                            }
                        }
                        ("GameObject/SetTranslation", _, Some(Value::Str(target)))
                            if target.is_empty() =>
                        {
                            if let (Some(Value::Vec3([x, y, z])), None | Some(Value::Str(_))) =
                                (c.param("Translation"), c.param("Node"))
                                && c.param("Node")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .is_empty()
                            {
                                moves.push((c.start, vec3(*x, *y, *z)));
                            }
                        }
                        _ => {}
                    }
                }
            }
            cuts.sort();
            visible.sort();
            moves.sort_by_key(|m| m.0);
            let scene = scene_path.and_then(|sp| scenes.iter().position(|s| s.path == sp));
            actors.push(ActorSlot {
                spec,
                scene,
                actor: None,
                cuts,
                visible,
                moves,
            });
        }

        // the shots' cameras
        let mut cameras = HashMap::new();
        for (_, n) in &shots {
            if cameras.contains_key(n) {
                continue;
            }
            let p = shot_paths
                .get(n)
                .cloned()
                .unwrap_or_else(|| format!("{folder}/{n}.trcma"));
            match ar.read(&p).and_then(|b| CameraAnim::parse(&b)) {
                Ok(c) => {
                    cameras.insert(n.clone(), c);
                }
                Err(e) => warnings.push(format!("camera {p}: {e:#}")),
            }
        }

        CutscenePlayer {
            name,
            frame: 0.0,
            frames: t.frames().max(1),
            playing: true,
            looping: false,
            speed: 1.0,
            use_camera: !shots.is_empty(),
            scenes,
            cache: HashMap::new(),
            cameras,
            shots,
            messages,
            actors,
            warnings,
        }
    }

    /// The actors' specs with the cuts the timeline asks of each, and the shot each cut starts with (for
    /// `probe --cast`).
    #[allow(dead_code)] // used by the probe
    pub fn cast_plan(&self) -> Vec<(&Spec, CutPlan)> {
        self.actors
            .iter()
            .map(|a| {
                let cuts = a
                    .cuts
                    .iter()
                    .map(|(f, k)| (*k, self.shot_near(*f).map(str::to_string)))
                    .collect();
                (&a.spec, cuts)
            })
            .collect()
    }

    /// Whether actor `i` shows at the current frame.
    fn actor_shown(&self, i: usize) -> bool {
        let a = &self.actors[i];
        a.scene.is_none_or(|s| self.scenes[s].shown(self.frame))
            && last_at(&a.visible, self.frame).is_none_or(|(_, v)| v)
    }

    /// The actors: (track name, built, shown now, cut now, warnings or error).
    pub fn actor_states(&self) -> Vec<ActorState> {
        (0..self.actors.len())
            .map(|i| {
                let a = &self.actors[i];
                let (built, notes) = match &a.actor {
                    None => (None, Vec::new()),
                    Some(Ok(x)) => (Some(true), x.warnings.clone()),
                    Some(Err(e)) => (Some(false), vec![e.clone()]),
                };
                let cut = last_at(&a.cuts, self.frame).map_or(0, |(_, k)| k);
                ActorState {
                    key: a.spec.key(),
                    built,
                    shown: self.actor_shown(i),
                    cut,
                    notes,
                }
            })
            .collect()
    }

    /// Builds one actor that is not built yet, one showing now first. Returns false when all are built.
    fn build_actor(&mut self, context: &Context, ar: &mut Archive) -> bool {
        let todo: Vec<usize> = (0..self.actors.len())
            .filter(|&i| self.actors[i].actor.is_none())
            .collect();
        let Some(&i) = todo.iter().find(|&&i| self.actor_shown(i)).or(todo.first()) else {
            return false;
        };
        let spec = self.actors[i].spec.clone();
        self.actors[i].actor = Some(cast::load(context, ar, &spec).map_err(|e| format!("{e:#}")));
        true
    }

    /// Builds one scene that is not built yet: one shown now, else the one shown soonest after the current frame,
    /// else any. Returns false when all are built.
    pub fn build_next(&mut self, context: &Context, ar: &mut Archive) -> bool {
        let f = self.frame;
        let first_shown = |s: &SceneGroup| -> u32 {
            if s.shown(f) {
                return 0;
            }
            s.events
                .iter()
                .filter(|(at, on)| *on && *at as f32 > f)
                .map(|(at, _)| *at)
                .min()
                .unwrap_or(u32::MAX)
        };
        let Some(i) = (0..self.scenes.len())
            .filter(|&i| self.scenes[i].group.is_none())
            .min_by_key(|&i| first_shown(&self.scenes[i]))
        else {
            return self.build_actor(context, ar);
        };
        let path = self.scenes[i].path.clone();
        let (items, w) = plants::scene_items(ar, std::slice::from_ref(&path));
        let group = plants::build(context, ar, items, w, &mut self.cache);
        self.warnings.extend(group.warnings.iter().cloned());
        self.scenes[i].group = Some(group);
        true
    }

    /// How many scenes and actors are not built yet.
    pub fn loading(&self) -> usize {
        self.scenes.iter().filter(|s| s.group.is_none()).count()
            + self.actors.iter().filter(|a| a.actor.is_none()).count()
    }

    /// True while a scene or actor that should show now is not built (playback waits).
    pub fn waiting(&self) -> bool {
        self.scenes
            .iter()
            .any(|s| s.group.is_none() && s.shown(self.frame))
            || (0..self.actors.len()).any(|i| self.actors[i].actor.is_none() && self.actor_shown(i))
    }

    /// Advances by `seconds` of wall time (at most a tenth of a second per call, so a slow frame does not skip).
    pub fn update(&mut self, seconds: f32) {
        if !self.playing || self.waiting() {
            return;
        }
        self.frame += seconds.min(0.1) * FPS * self.speed;
        let end = self.frames as f32;
        if self.frame >= end {
            if self.looping {
                self.frame %= end;
            } else {
                self.frame = end;
                self.playing = false;
            }
        }
    }

    pub fn restart(&mut self) {
        self.frame = 0.0;
        self.playing = true;
    }

    /// The shot that goes with something starting on `frame`: one starting then or up to 3 frames later (a cut's
    /// clip is often set a frame before its shot), else the one playing.
    fn shot_near(&self, frame: u32) -> Option<&str> {
        self.shots
            .iter()
            .find(|(f, _)| *f >= frame && *f <= frame + 3)
            .or_else(|| self.shots.iter().rev().find(|(f, _)| *f <= frame))
            .map(|(_, n)| n.as_str())
    }

    /// The shot playing now: its name and the camera.
    pub fn shot(&self) -> Option<(&str, Shot)> {
        let k = self.shots.partition_point(|(f, _)| *f as f32 <= self.frame);
        let (start, name) = self.shots.get(k.checked_sub(1)?)?;
        let cam = self.cameras.get(name)?;
        Some((name, cam.sample((self.frame - *start as f32) / FPS)))
    }

    /// Points `camera` along the current shot (when `use_camera` is on). Returns whether it did.
    pub fn drive(&self, camera: &mut Camera) -> bool {
        if !self.use_camera {
            return false;
        }
        let Some((_, s)) = self.shot() else {
            return false;
        };
        camera.set_view(s.position, s.position + s.forward(), s.up());
        camera.set_perspective_projection(
            radians(s.fov),
            s.near.max(0.01),
            s.far.max(s.near + 1.0),
        );
        true
    }

    /// The message window showing now: (file, label).
    pub fn message(&self) -> Option<(&str, &str)> {
        self.messages
            .iter()
            .rev()
            .find(|(a, b, _, _)| *a as f32 <= self.frame && self.frame < *b as f32)
            .map(|(_, _, f, l)| (f.as_str(), l.as_str()))
    }

    /// The scenes and whether each shows now: (name, path, shown, instances or `None` while not built).
    pub fn scene_states(&self) -> Vec<(&str, &str, bool, Option<usize>)> {
        self.scenes
            .iter()
            .map(|s| {
                (
                    s.name.as_str(),
                    s.path.as_str(),
                    s.shown(self.frame),
                    s.group.as_ref().map(|g| g.instances),
                )
            })
            .collect()
    }

    /// Box around the scenes shown now (or all of them), to frame the free camera on.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let pick = |only_shown: bool| {
            self.scenes
                .iter()
                .filter(|s| !only_shown || s.shown(self.frame))
                .filter_map(|s| s.group.as_ref().and_then(|g| g.aabb))
                .reduce(|(a, b), (c, d)| (a.zip(c, f32::min), b.zip(d, f32::max)))
        };
        pick(true).or_else(|| pick(false))
    }

    /// Picks LODs for the camera's position and poses the actors showing now.
    pub fn prepare(&mut self, ar: &mut Archive, eye: Vec3) {
        let f = self.frame;
        for i in 0..self.actors.len() {
            if !self.actor_shown(i) {
                continue;
            }
            let (start, cut) = last_at(&self.actors[i].cuts, f).unwrap_or((0, 0));
            let shot = self.shot_near(start).map(str::to_string);
            let a = &mut self.actors[i];
            let at = match last_at(&a.moves, f) {
                Some((_, p)) => Mat4::from_translation(p),
                None => a.spec.placed,
            };
            if let Some(Ok(actor)) = &mut a.actor {
                actor.pose(
                    ar,
                    cut,
                    shot.as_deref(),
                    (f - start as f32).max(0.0) / FPS,
                    at,
                );
            }
        }
        for s in self.scenes.iter_mut().filter(|s| s.shown(f)) {
            if let Some(g) = &mut s.group {
                g.update_lods(eye, LodSettings::default());
            }
        }
    }

    pub fn render(&self, target: &RenderTarget, camera: &Camera, lights: &[&dyn Light]) {
        let objects = self
            .scenes
            .iter()
            .filter(|s| s.shown(self.frame))
            .filter_map(|s| s.group.as_ref())
            .flat_map(|g| g.objects());
        let cast = (0..self.actors.len())
            .filter(|&i| self.actor_shown(i))
            .filter_map(|i| match &self.actors[i].actor {
                Some(Ok(a)) => Some(a),
                _ => None,
            })
            .flat_map(|a| a.objects());
        target.render(camera, objects.chain(cast), lights);
    }
}
