//! `.trtml` timelines: the scripts behind cutscenes. A timeline does not hold animation itself; it is a list of
//! commands on a frame line (load a camera animation, play it, show a model, move an object, post a sound event,
//! show a message window, load a sub-scene, ...), each aimed at a named object of the cutscene's scenes.
//!
//! Layout (FlatBuffers, slots as checked against the game's files with `probe --timelines`, 2026-11-18):
//! - root: `0` tracks; `1` u32, the cutscene's length in frames; `3`, `4` u32 (not known). All 285 cutscene
//!   timelines parse and no clip ends after slot 1. Frames are 60 per second: camera animations (`.trcma`,
//!   header `{1: frame count, 2: fps = 60}`) follow each other with no gap (`d010_cam_c002` plays from 120 for
//!   240 frames, `c003` starts on 360), and sound events are named after their frame (`PLAY_D010_01_001308`
//!   starts on frame 1308).
//! - track: `0` name (the object it drives: `CAM`, `d010.Camera`, `d010.tr0690_G_Mod`, `cut_name`, ...);
//!   `2` groups; `3` conditions `{0: variable, 1: value}`: when the track plays. Empty in story events; battle
//!   timelines repeat a track (`CAM`) per case (`PlayerRide` = `Ride` / `NoRide`, `SelfOther` = `Self`, ...), and
//!   their first, unnamed track lists every variable with `None`.
//! - group: `0` name (`attach`, `XYZ`, `Label`, `NoRideNoFall`, ... often a condition value or a parameter
//!   name); `2` clips.
//! - clip: `0` object name (same as the track's, so far); `1` command (`CameraAnimation/Play`,
//!   `Model/SetVisibility`, `Audio/PostEventGlobal`, `Title/Message/ShowMessageWindow`, `Scene/LoadSubScene`,
//!   ...); `2` start frame, `3` end frame (u32); `4` u32 flags (not known); `9` parameters.
//! - parameter: `0` name (`FileName`, `Path`, `EventName`, `Visibility`, ...); `1` u8 value type (a FlatBuffers
//!   union tag); `2` the value table, slot `0` holding the value. Tags: see [`Value`].
//!
//! Where cutscenes live (see `cutscenes.rs`): `demo/<kind>/<name>/<name>.trtml`, with camera animations
//! (`.trcma`), object and character animations (`.tracp`, `.tracm`, `.tranm`) and models next to it, and its
//! scenes under `world/scene/parts/demo/<kind>/<name>/<sub>_/<sub>_0.trscn`.

use crate::fb::Table;
use anyhow::{Context, Result, bail};

/// One parameter value, by union tag: 1 int, 2 float, 3 bool (one byte), 4 string, 5 three floats (a struct
/// inline in the value table: translations, rotations in degrees, scales, colours). Over all 285 timelines every
/// int is below 65536 and every float finite (`probe --timelines`; layout from `probe --timeline-values`).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    Str(String),
    Vec3([f32; 3]),
    /// A tag not known yet, with its value table's slot 0 as a u32 (0 when absent).
    Other(u8, u32),
}

impl Value {
    pub fn text(&self) -> String {
        match self {
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Str(s) => format!("{s:?}"),
            Value::Float(f) => format!("{f}"),
            Value::Vec3([x, y, z]) => format!("({x}, {y}, {z})"),
            Value::Other(t, v) => format!("(type {t}) {v}"),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub command: String,
    pub start: u32,
    pub end: u32,
    pub params: Vec<Param>,
}

impl Clip {
    pub fn param(&self, name: &str) -> Option<&Value> {
        self.params
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.value)
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug)]
pub struct Track {
    /// The object the track drives.
    pub name: String,
    pub groups: Vec<Group>,
    /// When the track plays: variable and value (`PlayerRide` = `Ride`; `None` = any).
    pub conditions: Vec<(String, String)>,
}

impl Track {
    pub fn clips(&self) -> impl Iterator<Item = &Clip> {
        self.groups.iter().flat_map(|g| &g.clips)
    }
}

#[derive(Clone, Debug)]
pub struct Timeline {
    pub tracks: Vec<Track>,
    /// Root slot 1: the length in frames.
    pub length: u32,
    /// Root slots 3 and 4 (meaning not known).
    pub unknown: [u32; 2],
}

/// Timeline frames per second (see the module comment).
pub const FPS: f32 = 60.0;

impl Timeline {
    pub fn parse(buf: &[u8]) -> Result<Timeline> {
        let root = Table::root(buf, 0)?;
        if root.slot_count() < 1 {
            bail!("no tracks slot");
        }
        let mut tracks = Vec::new();
        for (ti, t) in root.tables(0)?.iter().enumerate() {
            let name = t.string(0)?.unwrap_or_default();
            let mut groups = Vec::new();
            for g in t
                .tables(2)
                .with_context(|| format!("track {ti} ({name})"))?
            {
                let mut clips = Vec::new();
                for c in g.tables(2)? {
                    let mut params = Vec::new();
                    for p in c.tables(9)? {
                        params.push(Param {
                            name: p.string(0)?.unwrap_or_default(),
                            value: value(&p)?,
                        });
                    }
                    clips.push(Clip {
                        command: c.string(1)?.unwrap_or_default(),
                        start: c.u32(2, 0),
                        end: c.u32(3, 0),
                        params,
                    });
                }
                groups.push(Group {
                    name: g.string(0)?.unwrap_or_default(),
                    clips,
                });
            }
            let mut conditions = Vec::new();
            for c in t.tables(3)? {
                conditions.push((
                    c.string(0)?.unwrap_or_default(),
                    c.string(1)?.unwrap_or_default(),
                ));
            }
            tracks.push(Track {
                name,
                groups,
                conditions,
            });
        }
        Ok(Timeline {
            tracks,
            length: root.u32(1, 0),
            unknown: [root.u32(3, 0), root.u32(4, 0)],
        })
    }

    pub fn clips(&self) -> impl Iterator<Item = (&Track, &Clip)> {
        self.tracks
            .iter()
            .flat_map(|t| t.clips().map(move |c| (t, c)))
    }

    /// The last frame any clip ends on.
    pub fn last_clip_end(&self) -> u32 {
        self.clips().map(|(_, c)| c.end).max().unwrap_or(0)
    }

    /// Length in frames: root slot 1, or the last clip end when that is larger (or slot 1 is missing).
    pub fn frames(&self) -> u32 {
        self.length.max(self.last_clip_end())
    }

    /// Every string parameter that looks like a file path (has a `/` and a `.`), sorted, without repeats.
    pub fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .clips()
            .flat_map(|(_, c)| &c.params)
            .filter_map(|p| p.value.as_str())
            .filter(|s| s.contains('/') && s.contains('.'))
            .map(str::to_string)
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// How often each command is used, most used first.
    pub fn command_counts(&self) -> Vec<(String, usize)> {
        let mut m = std::collections::BTreeMap::<&str, usize>::new();
        for (_, c) in self.clips() {
            *m.entry(&c.command).or_default() += 1;
        }
        let mut v: Vec<(String, usize)> = m.into_iter().map(|(k, n)| (k.to_string(), n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }
}

fn value(p: &Table) -> Result<Value> {
    let tag = p.u8(1, 0);
    // an absent value table is the default value of its type
    let v = p.table(2)?;
    let int = v.map_or(0, |v| v.u32(0, 0));
    let float = v.map_or(0.0, |v| v.f32(0, 0.0));
    Ok(match tag {
        1 => Value::Int(int as i32),
        2 => Value::Float(float),
        3 => Value::Bool(v.is_some_and(|v| v.u8(0, 0) != 0)),
        4 => Value::Str(match v {
            Some(v) => v.string(0)?.unwrap_or_default(),
            None => String::new(),
        }),
        5 => {
            let mut xyz = [0.0; 3];
            if let Some(a) = v.and_then(|v| v.field(0)) {
                for (k, c) in xyz.iter_mut().enumerate() {
                    *c = f32::from_bits(crate::fb::u32_at(p.buf, a + 4 * k)?);
                }
            }
            Value::Vec3(xyz)
        }
        t => Value::Other(t, int),
    })
}
