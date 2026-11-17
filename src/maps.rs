//! The places the World tab can show: the three overworld maps and the interior rooms.
//!
//! Overworld maps (terrain tiles `field_graphic/terrain/terrain_XX_YY`, `probe --map-tiles`; every tile 500 m,
//! 513 x 513 samples, origin in its height file):
//! - **Paldea**: XX, YY 00..09. Tile XX_YY has origin (XX*500, -YY*500 - 500), except the YY = 00 row, which lies
//!   past row 09 at z -5500. Streamed scenes in `unity_output/`.
//! - **Kitakami** (DLC 1, `su1`): XX 10..13, YY 00..03, origin ((XX-10)*500, -YY*500 - 500); no holes. Scenes in
//!   `unity_output_1/`.
//! - **Terarium** (DLC 2, `su2`): XX 20..23, YY 00..03, origin ((XX-20)*500, -YY*500 - 500); the sea biome is holes.
//!   Scenes in `unity_output_2/`.
//! - Tiles `99_91`, `99_92`, `99_98`, `99_99` are test tiles (one has no ground); not offered.
//!
//! Resident scenes (always loaded with the map; paths recovered from `SubScene` references and confirmed by hash):
//! `world_resident/outdoor_resident_/outdoor_resident_0.trscn` (Paldea's sea and water), `unity_output/
//! town_c01_field_resident_` (the academy's water and cliffs), `unity_output/c01_both_01_field_resident_`,
//! `unity_output_1/dlc1_resident_` (Kitakami's water, outer ground and forest), `unity_output_2/dlc2_resident_`
//! (the Terarium's sea, lakes, outer walls and dome). The game chains them from `world_resident_0.trscn` and the
//! `resident_field` scenes.
//!
//! Rooms: `world/scene/parts/field/room/<code>_/<code>_0.trscn` (or a sub-folder such as `battlebg`), made only
//! of `SubScene`s; `<code>_obj` places the room's model. Found from the pack names (`rooms`).

use crate::archive::Archive;
use crate::scene::FIELD;

/// A map's tile grid.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Grid {
    /// The first tile's XX and YY.
    pub first: (i32, i32),
    /// Tiles along XX and YY.
    pub size: (i32, i32),
    /// The first row (YY = first) lies past the last one (Paldea's 00 row at z -5500).
    pub wrap_first_row: bool,
}

impl Grid {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        (self.first.0..self.first.0 + self.size.0).contains(&x)
            && (self.first.1..self.first.1 + self.size.1).contains(&y)
    }

    /// The row's place along -z, 0 = nearest z 0.
    fn order(&self, y: i32) -> i32 {
        if self.wrap_first_row && y == self.first.1 {
            self.size.1
        } else {
            y - self.first.1
        }
    }

    /// Where a tile's minimum corner lies in x/z (metres), as its height file says (`probe --map-tiles`).
    #[allow(dead_code)] // checked by the probe
    pub fn origin(&self, x: i32, y: i32) -> (f32, f32) {
        (
            (x - self.first.0) as f32 * 500.0,
            -(self.order(y) + 1) as f32 * 500.0,
        )
    }

    /// The tile under world x/z, if it is on the grid.
    pub fn tile_at(&self, x: f32, z: f32) -> Option<(i32, i32)> {
        let tx = (x / 500.0).floor() as i32 + self.first.0;
        let order = (-z / 500.0).floor() as i32;
        let ty = if self.wrap_first_row && order == self.size.1 {
            self.first.1
        } else if self.wrap_first_row && order == 0 {
            return None;
        } else {
            order + self.first.1
        };
        self.contains(tx, ty).then_some((tx, ty))
    }

    /// Minimap cell of a tile: column, row (row 0 at the top = furthest along -z).
    pub fn cell(&self, x: i32, y: i32) -> (i32, i32) {
        let last = if self.wrap_first_row {
            self.size.1
        } else {
            self.size.1 - 1
        };
        (x - self.first.0, last - self.order(y))
    }

    /// The grid's tiles present in the archive, as (x, y, path to the tile's .trtrr).
    pub fn tiles(&self, ar: &Archive) -> Vec<(i32, i32, String)> {
        let mut out = Vec::new();
        for x in self.first.0..self.first.0 + self.size.0 {
            for y in self.first.1..self.first.1 + self.size.1 {
                let p = crate::terrain::tile_path(x, y);
                if ar.contains(&p) {
                    out.push((x, y, p));
                }
            }
        }
        out
    }
}

/// A place to show: an overworld map with terrain, or a room (scenes only).
#[derive(Clone, PartialEq, Debug)]
pub struct Map {
    pub name: String,
    pub grid: Option<Grid>,
    /// The folder of the map's streamed tile scenes.
    pub folder: String,
    /// Scenes loaded with the map, wherever the camera is: water, outer scenery, a room.
    pub resident: Vec<String>,
    /// Paldea's towns, academy, caves and Team Star bases (`scene::area_scenes`).
    pub areas: bool,
}

/// The three overworld maps.
pub fn overworlds() -> Vec<Map> {
    let out = |n: &str| format!("{FIELD}/{n}");
    vec![
        Map {
            name: "Paldea".into(),
            grid: Some(Grid {
                first: (0, 0),
                size: (10, 10),
                wrap_first_row: true,
            }),
            folder: out("unity_output"),
            resident: vec![
                out("world_resident/outdoor_resident_/outdoor_resident_0.trscn"),
                out("unity_output/town_c01_field_resident_/town_c01_field_resident_0.trscn"),
                out("unity_output/c01_both_01_field_resident_/c01_both_01_field_resident_0.trscn"),
            ],
            areas: true,
        },
        Map {
            name: "Kitakami".into(),
            grid: Some(Grid {
                first: (10, 0),
                size: (4, 4),
                wrap_first_row: false,
            }),
            folder: out("unity_output_1"),
            resident: vec![out("unity_output_1/dlc1_resident_/dlc1_resident_0.trscn")],
            areas: false,
        },
        Map {
            name: "Terarium".into(),
            grid: Some(Grid {
                first: (20, 0),
                size: (4, 4),
                wrap_first_row: false,
            }),
            folder: out("unity_output_2"),
            resident: vec![out("unity_output_2/dlc2_resident_/dlc2_resident_0.trscn")],
            areas: false,
        },
    ]
}

/// A room as a map: no terrain, the room's scene as its only resident scene.
pub fn room(name: &str, path: &str) -> Map {
    Map {
        name: name.into(),
        grid: None,
        folder: String::new(),
        resident: vec![path.into()],
        areas: false,
    }
}

/// Every streamed scene of every overworld map, areas then tiles (label, path).
pub fn field_scenes(ar: &Archive) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = crate::scene::area_scenes(ar)
        .into_iter()
        .flat_map(|(prefix, paths)| paths.into_iter().map(move |p| (prefix.clone(), p)))
        .collect();
    for map in overworlds() {
        let Some(g) = map.grid else { continue };
        for (x, y, _) in g.tiles(ar) {
            out.extend(
                crate::scene::tile_scenes(ar, &map.folder, x, y)
                    .into_iter()
                    .map(|p| (format!("{x:02}_{y:02}"), p)),
            );
        }
    }
    out
}

/// The room scenes, found from the pack names under `world/scene/parts/field/room`: (code, path), sorted.
/// A pack named after `room/<dirs>/<name>_/<name>_0.trscn` gives `<name>` (it repeats in the pack name) and the
/// folders before it; the room is `<first folder>_/<first folder>_0.trscn` when that exists (the pack is often
/// named after one of the room's sub-scenes), else that scene itself.
pub fn rooms(ar: &Archive) -> Vec<(String, String)> {
    let mut out = std::collections::BTreeMap::new();
    let base = format!("{FIELD}/room");
    for n in &ar.pack_names {
        let Some(rest) = n
            .strip_prefix("arc/worldscenepartsfieldroom")
            .and_then(|r| r.strip_suffix("_0.trscn.trpak"))
        else {
            continue;
        };
        if let Some((code, path)) = room_from_pack(ar, &base, rest) {
            out.entry(path).or_insert(code);
        }
    }
    let mut v: Vec<(String, String)> = out.into_iter().map(|(p, c)| (c, p)).collect();
    v.sort();
    v
}

/// One room pack's scene: `rest` is the pack name between `room` and `_0.trscn.trpak`, which ends with
/// `<name>_<name>`; what comes before is the folders without slashes (none, or one or two folders).
fn room_from_pack(ar: &Archive, base: &str, rest: &str) -> Option<(String, String)> {
    for len in (1..rest.len()).rev() {
        // rest = <dirs> <name> "_" <name>, so the name has `len` bytes
        let Some(start) = rest.len().checked_sub(2 * len + 1) else {
            continue;
        };
        if !rest.is_char_boundary(start) || !rest.is_char_boundary(rest.len() - len) {
            continue;
        }
        let name = &rest[rest.len() - len..];
        if &rest[start..start + len] != name || rest.as_bytes()[start + len] != b'_' {
            continue;
        }
        let dirs = &rest[..start];
        let file = format!("{name}_/{name}_0.trscn");
        // the folders: none, one, or two (every cut point)
        let mut candidates: Vec<Vec<&str>> =
            vec![if dirs.is_empty() { vec![] } else { vec![dirs] }];
        for c in 1..dirs.len() {
            if dirs.is_char_boundary(c) {
                candidates.push(vec![&dirs[..c], &dirs[c..]]);
            }
        }
        for folders in candidates {
            let mut p = base.to_string();
            for f in &folders {
                p.push('/');
                p.push_str(f);
            }
            let found = format!("{p}/{file}");
            if !ar.contains(&found) {
                continue;
            }
            // the room itself, when the pack is named after one of its sub-scenes
            if let Some(first) = folders.first() {
                let code = first.trim_end_matches('_');
                let main = format!("{base}/{code}_/{code}_0.trscn");
                if ar.contains(&main) {
                    return Some((code.to_string(), main));
                }
                let label = format!("{}/{name}", folders.join("/"));
                return Some((label, found));
            }
            return Some((name.to_string(), found));
        }
    }
    None
}
