//! Scenes (`.trscn`): a tree of typed nodes. Field slots, checked against the game's files:
//!   scene { 0: name, 4: [node] }
//!   node  { 0: type name ("trinity_ObjectTemplate", "trinity_ModelComponent", "trinity_ScenePoint", "SubScene",
//!           "trinity_PropertySheet", ...), 1: [u8] the node's own data, itself a FlatBuffer whose layout
//!           depends on the type, 2: [node] children }
//! `SubScene` data { 0: relative path to another scene; stored as `<name>_0.trscn` } composes scenes: rooms are
//! nothing but sub-scenes (`<room>_obj` places the room model, others cameras, events, collision), and the world's
//! resident scenes chain the same way (`world_resident` -> `outdoor_resident`, ...; `load_content`).
//! The layouts of the node types the viewer reads are documented where they are read (`scene_object`,
//! `Node::object`, `Node::reference`). Transforms chain from parent to child; top-level objects are in world
//! metres, the terrain's frame. Rotations are Euler radians (nearly always about y), applied y, x, z.
//! `--check` over the three maps' streamed scenes (`maps::field_scenes`, with sub-scenes and the `xl` size): 579
//! scenes, 2803 placed objects, 22,587 instances, 0 failures; 151 rooms and 5 resident scenes, 0 failures.
//! Earlier, `probe --town-scenes` over Paldea's streamed scenes: 368 scenes parse
//! with 0 node errors; 2044 placed objects, 1560 instance files (17,630 instances, all parse), 1497 models (all
//! load). The scene packs hold about 3000 scenes in all (`probe --scene-packs`): NPCs, trainers, items, events.

use crate::fb::Table;
use anyhow::{Result, bail};

pub struct Node {
    pub kind: String,
    /// The node's data: a FlatBuffer of its own (empty for some kinds).
    pub data: Vec<u8>,
    pub children: Vec<Node>,
}

pub struct Scene {
    #[allow(dead_code)] // printed by the probe
    pub name: String,
    pub nodes: Vec<Node>,
}

fn node(t: &Table, depth: usize) -> Result<Node> {
    if depth > 64 {
        bail!("scene nodes nested more than 64 deep");
    }
    let kind = t.string(0)?.unwrap_or_default();
    let data = match t.vector(1) {
        Ok((start, n)) if n > 0 => crate::fb::get(t.buf, start, n)?.to_vec(),
        _ => Vec::new(),
    };
    let children = t
        .tables(2)?
        .iter()
        .map(|c| node(c, depth + 1))
        .collect::<Result<Vec<_>>>()?;
    Ok(Node {
        kind,
        data,
        children,
    })
}

pub fn parse(buf: &[u8]) -> Result<Scene> {
    let root = Table::root(buf, 0)?;
    let name = root.string(0)?.unwrap_or_default();
    let nodes = root
        .tables(4)?
        .iter()
        .map(|t| node(t, 0))
        .collect::<Result<Vec<_>>>()?;
    Ok(Scene { name, nodes })
}

impl Node {
    /// The node's data as a FlatBuffer table, if it has data.
    pub fn table(&self) -> Option<Table<'_>> {
        if self.data.len() < 8 {
            return None;
        }
        Table::root(&self.data, 0).ok()
    }

    /// Visits this node and all below it, with the chain of parents.
    pub fn walk<'a>(
        &'a self,
        parents: &mut Vec<&'a Node>,
        f: &mut dyn FnMut(&'a Node, &[&'a Node]),
    ) {
        f(self, parents);
        parents.push(self);
        for c in &self.children {
            c.walk(parents, f);
        }
        parents.pop();
    }
}

/// A node's local transform: scale, rotation (Euler angles in radians, nearly always about y only), translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Srt {
    pub scale: [f32; 3],
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
}

impl Srt {
    pub const IDENTITY: Srt = Srt {
        scale: [1.0; 3],
        rotation: [0.0; 3],
        translation: [0.0; 3],
    };
}

/// Three f32 of an inline struct field (absent = `def`).
fn vec3_struct(t: &Table, slot: usize, def: [f32; 3]) -> Result<[f32; 3]> {
    let Some(a) = t.field(slot) else {
        return Ok(def);
    };
    let b = crate::fb::get(t.buf, a, 12)?;
    Ok(std::array::from_fn(|k| {
        f32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]])
    }))
}

/// `trinity_SceneObject` data { 0: name, 1: transform { 0: scale vec3, 1: rotation vec3, 2: translation vec3 }
/// (inline structs), 3: flags?, ... }. Returns the name and transform.
pub fn scene_object(t: &Table) -> Result<(String, Srt)> {
    let name = t.string(0)?.unwrap_or_default();
    let srt = match t.table(1)? {
        Some(x) => Srt {
            scale: vec3_struct(&x, 0, [1.0; 3])?,
            rotation: vec3_struct(&x, 1, [0.0; 3])?,
            translation: vec3_struct(&x, 2, [0.0; 3])?,
        },
        None => Srt::IDENTITY,
    };
    Ok((name, srt))
}

impl Node {
    /// The name and local transform this node gives its subtree: a `trinity_SceneObject`'s own, or the
    /// SceneObject a `trinity_ObjectTemplate` embeds (data { 0: name, 1: template name, 4: "trinity_SceneObject",
    /// 5: [u8] that object's data }). Other kinds: `None`.
    pub fn object(&self) -> Result<Option<(String, Srt)>> {
        let Some(t) = self.table() else {
            return Ok(None);
        };
        match self.kind.as_str() {
            "trinity_SceneObject" => scene_object(&t).map(Some),
            "trinity_ObjectTemplate" => {
                if t.string(4)?.as_deref() != Some("trinity_SceneObject") {
                    return Ok(None);
                }
                let (start, n) = t.vector(5)?;
                let inner = crate::fb::get(t.buf, start, n)?;
                let it = Table::root(inner, 0)?;
                let (_, srt) = scene_object(&it)?;
                Ok(Some((t.string(0)?.unwrap_or_default(), srt)))
            }
            _ => Ok(None),
        }
    }

    /// A `trinity_ModelComponent`'s model path (data slot 0), a `trinity_ModelInstancerComponent`'s instance
    /// file (data slot 0), as (kind, path).
    /// A `trinity_ModelComponent`'s LOD switch distances in metres: slot 5 (LOD 0 to 1) and slot 6 (LOD 1 to 2).
    /// Slot 7 is the next step (60 by default: the game's cull distance; the viewer does not cull there).
    /// `probe --lod-survey`: 3401 of the 3604 components use 20 / 40 / 60; buildings and landmarks 220-500 /
    /// 300-650 / 400-1000. Slots 9-15 hold the same values everywhere (20, 10000, 30, 2, 40, 1, 50). Absent:
    /// the 20 / 40 default.
    pub fn lod_distances(&self) -> [f32; 2] {
        let d = DEFAULT_LOD_DISTANCES;
        let Some(t) = self.table() else { return d };
        let a = t.f32(5, d[0]);
        let b = t.f32(6, d[1]);
        if a.is_finite() && b.is_finite() && a > 0.0 && b >= a {
            [a, b]
        } else {
            d
        }
    }

    pub fn reference(&self) -> Option<(&'static str, String)> {
        let t = self.table()?;
        let p = t.string(0).ok().flatten()?;
        match self.kind.as_str() {
            "trinity_ModelComponent" => Some(("model", p)),
            "trinity_ModelInstancerComponent" => Some(("instances", p)),
            _ => None,
        }
    }
}

/// One placed model: its path and world matrix (column-major).
/// The LOD switch distances most models use (ModelComponent slots 5 and 6).
pub const DEFAULT_LOD_DISTANCES: [f32; 2] = [20.0, 40.0];

pub struct Placement {
    /// The object's name (the nearest SceneObject / ObjectTemplate above the model).
    #[allow(dead_code)] // for debugging and the probe
    pub name: String,
    pub model: String,
    pub matrix: three_d::Mat4,
    /// Where the model switches to LOD 1 and LOD 2 (`Node::lod_distances`).
    pub lods: [f32; 2],
}

/// An instance file and the model it draws, with the model component's LOD distances.
pub struct Instancer {
    pub model: String,
    pub file: String,
    pub lods: [f32; 2],
}

/// What a scene places: single models, and instance files with the model they draw (their matrices are already
/// in world space; `probe --town-scenes` checks them against the ground).
#[derive(Default)]
pub struct Content {
    pub placements: Vec<Placement>,
    /// (model path, `.trins` path)
    pub instancers: Vec<Instancer>,
    /// Nodes whose data did not parse.
    pub errors: Vec<String>,
    /// `SubScene` references (relative path, transform above them); `load_content` reads them in.
    pub subscenes: Vec<(String, three_d::Mat4)>,
}

impl Srt {
    /// Translation * rotation (y, then x, then z, as Unity applies Euler angles) * scale.
    pub fn matrix(&self) -> three_d::Mat4 {
        use three_d::{Mat4, Rad, vec3};
        let [sx, sy, sz] = self.scale;
        let [rx, ry, rz] = self.rotation;
        let [tx, ty, tz] = self.translation;
        Mat4::from_translation(vec3(tx, ty, tz))
            * Mat4::from_angle_y(Rad(ry))
            * Mat4::from_angle_x(Rad(rx))
            * Mat4::from_angle_z(Rad(rz))
            * Mat4::from_nonuniform_scale(sx, sy, sz)
    }
}

fn collect_into(
    nodes: &[Node],
    parent: three_d::Mat4,
    parent_name: &str,
    out: &mut Content,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    // a ModelComponent next to a ModelInstancerComponent draws that instance file, not one copy
    let instance_file = nodes.iter().find_map(|n| match n.reference() {
        Some(("instances", p)) => Some(p),
        _ => None,
    });
    for n in nodes {
        // `SubScene` data { 0: path of another scene, relative to this one's folder }
        if n.kind == "SubScene"
            && let Some(Ok(Some(rel))) = n.table().map(|t| t.string(0))
        {
            out.subscenes.push((rel, parent));
        }
        match n.reference() {
            Some(("model", model)) => match &instance_file {
                Some(f) => out.instancers.push(Instancer {
                    model,
                    file: f.clone(),
                    lods: n.lod_distances(),
                }),
                None => out.placements.push(Placement {
                    name: parent_name.to_string(),
                    model,
                    matrix: parent,
                    lods: n.lod_distances(),
                }),
            },
            Some(_) => {}
            None => {}
        }
        let (matrix, name) = match n.object() {
            Ok(Some((name, srt))) => (parent * srt.matrix(), name),
            Ok(None) => (parent, parent_name.to_string()),
            Err(e) => {
                out.errors.push(format!("{}: {e:#}", n.kind));
                (parent, parent_name.to_string())
            }
        };
        collect_into(&n.children, matrix, &name, out, depth + 1);
    }
}

/// The models and instance files a scene places, with world matrices from the chain of object transforms.
pub fn content(scene: &Scene) -> Content {
    use three_d::SquareMatrix;
    let mut out = Content::default();
    collect_into(&scene.nodes, three_d::Mat4::identity(), "", &mut out, 0);
    out
}

/// The root of the field scenes.
pub const FIELD: &str = "world/scene/parts/field";

/// The folder holding Paldea's streamed scenes and their instance files. The DLC maps have their own
/// (`unity_output_1` Kitakami, `unity_output_2` the Terarium; see `maps.rs`).
pub const UNITY_OUTPUT: &str = "world/scene/parts/field/unity_output";

/// Size classes of the streamed scenes: `s` small props and plants, `m` buildings and big props, `l` and `xl`
/// (rare: landmarks, 28 `xl` scenes). Each is its own scene.
pub const SIZES: [&str; 4] = ["s", "m", "l", "xl"];

/// A streamed scene's path from its name (`town_t07_field_streaming_s`): the folder is the name plus `_`, the
/// file the name plus `_0.trscn`. A `_1` beside it is byte-identical in all but one checked (`c01_02` m).
pub fn streamed_path(stem: &str) -> String {
    streamed_path_in(UNITY_OUTPUT, stem)
}

/// The same in another folder (a DLC map's `unity_output_N`).
pub fn streamed_path_in(folder: &str, stem: &str) -> String {
    format!("{folder}/{stem}_/{stem}_0.trscn")
}

/// The streamed scenes' names, found in the scene packs (`probe --scene-packs`), follow a few families. Areas
/// (towns, the academy, caves, Team Star bases) are drawn as one group each; their prefixes before
/// `_streaming_<size>`:
fn area_prefixes() -> Vec<String> {
    let mut out = Vec::new();
    let mut town = |code: String| out.push(format!("town_{code}_field"));
    for n in 1..40 {
        town(format!("t{n:02}"));
    }
    for c in 1..10 {
        town(format!("c{c:02}"));
        for a in 1..10 {
            town(format!("c{c:02}_{a:02}"));
        }
    }
    town("city".into());
    town("sl".into());
    for c in 1..10 {
        out.push(format!("battle_c{c:02}_field"));
        for a in 1..10 {
            out.push(format!("c{c:02}_both_{a:02}_field"));
            out.push(format!("battle_c{c:02}_{a:02}_field"));
        }
    }
    for n in 1..20 {
        out.push(format!("cave_{n:02}_field"));
    }
    const TYPES: [&str; 18] = [
        "normal", "fire", "water", "grass", "electric", "ice", "fighting", "poison", "ground",
        "flying", "psychic", "bug", "rock", "ghost", "dragon", "dark", "steel", "fairy",
    ];
    out.extend(TYPES.iter().map(|t| format!("dan_{t}")));
    out
}

/// Every area the archive has: (prefix, the paths of its size classes).
pub fn area_scenes(ar: &crate::archive::Archive) -> Vec<(String, Vec<String>)> {
    area_prefixes()
        .into_iter()
        .filter_map(|prefix| {
            let paths: Vec<String> = SIZES
                .iter()
                .map(|z| streamed_path(&format!("{prefix}_streaming_{z}")))
                .filter(|p| ar.contains(p))
                .collect();
            (!paths.is_empty()).then_some((prefix, paths))
        })
        .collect()
}

/// A terrain tile's streamed scenes outside the areas, in its map's `folder`: `ter_XX_YY_streaming_<size>` (rocks,
/// signs, villages on the DLC maps) and `plan_ter_XX_YY_streaming_<size>` (ruins).
pub fn tile_scenes(ar: &crate::archive::Archive, folder: &str, x: i32, y: i32) -> Vec<String> {
    let mut out = Vec::new();
    for prefix in ["ter", "plan_ter"] {
        for z in SIZES {
            let p = streamed_path_in(folder, &format!("{prefix}_{x:02}_{y:02}_streaming_{z}"));
            if ar.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// A `SubScene` reference resolved against the folder of the scene holding it. The reference names
/// `<name>.trscn`; the file stored is `<name>_0.trscn` (all checked rooms and resident scenes), so that is tried
/// when the name as written is missing.
pub fn resolve_subscene(ar: &crate::archive::Archive, scene_path: &str, rel: &str) -> String {
    let dir = scene_path.rsplit_once('/').map_or("", |(d, _)| d);
    let p = crate::model::resolve(dir, rel);
    if ar.contains(&p) {
        return p;
    }
    match p.strip_suffix(".trscn") {
        Some(stem) => format!("{stem}_0.trscn"),
        None => p,
    }
}

/// A scene's content with its `SubScene`s read in (scenes only), recursively (up to 8 deep, each file once), placed by the
/// transform above the reference. Sub-scenes that are missing or do not parse become errors.
pub fn load_content(ar: &mut crate::archive::Archive, path: &str) -> Result<Content> {
    let scene = parse(&ar.read(path)?)?;
    let mut out = content(&scene);
    let mut seen = std::collections::HashSet::from([path.to_string()]);
    let mut queue: Vec<(String, three_d::Mat4, usize)> = std::mem::take(&mut out.subscenes)
        .into_iter()
        .map(|(rel, m)| (resolve_subscene(ar, path, &rel), m, 1))
        .collect();
    while let Some((p, matrix, depth)) = queue.pop() {
        // only scenes: rooms also reference event-scenario objects (`.trsog`, `world/scene/parts/event/...`; 1638
        // references in the 151 rooms, none of them in the archive under that name)
        if depth > 8 || !p.ends_with(".trscn") || !seen.insert(p.clone()) {
            continue;
        }
        let sub = match ar.read(&p).and_then(|b| parse(&b)) {
            Ok(s) => content(&s),
            Err(e) => {
                out.errors.push(format!("sub-scene {p}: {e:#}"));
                continue;
            }
        };
        for mut x in sub.placements {
            x.matrix = matrix * x.matrix;
            out.placements.push(x);
        }
        // instance files hold world-space matrices already
        out.instancers.extend(sub.instancers);
        out.errors.extend(sub.errors);
        for (rel, m) in sub.subscenes {
            queue.push((resolve_subscene(ar, &p, &rel), matrix * m, depth + 1));
        }
    }
    Ok(out)
}
