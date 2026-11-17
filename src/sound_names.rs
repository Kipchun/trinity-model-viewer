//! Names for sounds. Wwise keeps only 32-bit FNV-1 hashes of lower-case names, so names come from elsewhere and
//! are confirmed by hash:
//! - Event names (`PLAY_BGM_TI_VS_BOSS_02`, `SET_STATE_CLUBROOM_12`, `PLAY_ME_TI_ST_BIRTHDAY`) are strings in the
//!   game's audio tables (the packs of `audio/fb/env_poke_voice/.../env_poke_voice_lottery_settings_data.bin` and
//!   `audio/exl/fb/EnvPokeVoiceChainData.bin`): 1506 of them are events. Cries are `PLAY_PV_<dex number>` (a
//!   format string in the executable): 733 species.
//! - State values (`CLUBROOM_12`) are the set-state events' names without `SET_STATE_`, checked against the keys
//!   of the music switch containers.
//!
//! What a name reaches: an event's actions (HIRC 3) are found among its bytes; Play (0x04xx) names a target at
//! +6, Set State (0x1204) ends with the state group and value. From a node down: music switches (12) by their
//! decision tree, playlists (13), segments (10) and random / switch containers (5, 6) by the ids of child objects
//! among their bytes (the children list sits after variable-length parameters; a stray match is unlikely with
//! 32-bit ids), music tracks (11) and sounds (2) give the media ids.
//!
//! Each media file gets the named state values on the paths to it (a switch below a leaf is walked with that
//! leaf's values, skipping combinations that set one state group twice; `_OFF` / `_OUT` conditions are left
//! out), unless one event reaches it alone; else the event reaching the fewest files (at most 4). Titles: the
//! club room speaker (`CLUBROOM_NN` = message `common/clubroom_bgm` `clubroom_bgm_name_NN`), the battle music
//! selector (`VS_SELECT_TYPE_NN` = `common/btl_bgm_select` `msg_ui_btl_bgm_select_vsNN`), species for cries
//! (`common/monsname`). Over the user's dump (`probe --audio`, 2026-11-16): music 160 titles and 326 developer
//! names of 439, jingles 20 of 24, cries 10,106 of 10,107, ambience 64, objects 19.
//!
//! Decision tree (usually the end of a music switch): u32 depth (up to 11 seen), u32 group ids [depth], u8 group
//! types [depth], u32 tree size, u8 mode, then 12-byte nodes: u32 key, u32 audio node id (leaf) or u16 first
//! child, u16 child count, u16 weight, u16 probability.

use crate::archive::Archive;
use crate::msg;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn fnv1(name: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in name.to_ascii_lowercase().bytes() {
        h = h.wrapping_mul(0x01000193) ^ b as u32;
    }
    h
}

struct Obj {
    kind: u8,
    data: Vec<u8>,
}

/// (state path keys, audio node)
type Leaves = Vec<(Vec<u32>, u32)>;

struct Graph {
    objs: HashMap<u32, Obj>,
    trees: HashMap<u32, (Vec<u32>, Leaves)>,
    children: HashMap<u32, Vec<u32>>,
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

/// A music switch's decision tree. It usually ends the data, but not always (the club room's switch has 3.5 KB
/// after it), so every plausible start is tried (depth 1..16 (the club room switch has 11), group types 0/1, mode 0/1, child ranges inside the
/// tree) and the one whose leaves most often name existing objects wins.
fn tree(d: &[u8], exists: &dyn Fn(u32) -> bool) -> Option<(Vec<u32>, Leaves)> {
    let mut best: Option<(usize, (Vec<u32>, Leaves))> = None;
    for i in 4..d.len().saturating_sub(12) {
        let Some(t) = tree_at(d, i) else { continue };
        let good = t.1.iter().filter(|(_, n)| exists(*n)).count();
        if good * 2 >= t.1.len() && good > best.as_ref().map_or(0, |b| b.0) {
            best = Some((good, t));
        }
    }
    best.map(|b| b.1)
}

fn tree_at(d: &[u8], i: usize) -> Option<(Vec<u32>, Leaves)> {
    {
        let depth = u32le(d, i)? as usize;
        if !(1..=16).contains(&depth) {
            return None;
        }
        if (0..depth).any(|j| d.get(i + 4 + 4 * depth + j).is_none_or(|t| *t > 1)) {
            return None;
        }
        let at = i + 4 + 5 * depth;
        let size = u32le(d, at)? as usize;
        if size == 0 || !size.is_multiple_of(12) || at + 5 + size > d.len() || d[at + 4] > 1 {
            return None;
        }
        let groups: Vec<u32> = (0..depth).filter_map(|j| u32le(d, i + 4 + 4 * j)).collect();
        let nodes: Vec<(u32, u32)> = (0..size / 12)
            .filter_map(|m| Some((u32le(d, at + 5 + 12 * m)?, u32le(d, at + 9 + 12 * m)?)))
            .collect();
        let mut leaves = Vec::new();
        let mut stack = vec![(0usize, 0usize, Vec::<u32>::new())];
        while let Some((ix, level, path)) = stack.pop() {
            let Some(&(key, val)) = nodes.get(ix) else {
                continue;
            };
            let mut path = path;
            if level > 0 {
                path.push(key);
            }
            if level == depth {
                leaves.push((path, val));
                continue;
            }
            let (first, count) = ((val & 0xffff) as usize, (val >> 16) as usize);
            if first + count > nodes.len() || first <= ix {
                return None; // not a tree: children lie after their parent, inside the tree
            }
            for c in first..first + count {
                stack.push((c, level + 1, path.clone()));
            }
        }
        (!leaves.is_empty()).then_some((groups, leaves))
    }
}

impl Graph {
    fn new(banks: &[(&str, Vec<u8>)]) -> Graph {
        let mut objs = HashMap::new();
        for (_, hirc) in banks {
            let count = u32le(hirc, 0).unwrap_or(0);
            let mut q = 4;
            for _ in 0..count {
                let (Some(&kind), Some(size)) = (hirc.get(q), u32le(hirc, q + 1)) else {
                    break;
                };
                let d = hirc.get(q + 5..q + 5 + size as usize).unwrap_or(&[]);
                if let Some(id) = u32le(d, 0) {
                    objs.entry(id).or_insert(Obj {
                        kind,
                        data: d.to_vec(),
                    });
                }
                q += 5 + size as usize;
            }
        }
        let mut trees = HashMap::new();
        let mut children = HashMap::new();
        for (&id, o) in &objs {
            if o.kind == 12
                && let Some(t) = tree(&o.data, &|n| objs.contains_key(&n))
            {
                trees.insert(id, t);
            }
            let want: &[u8] = match o.kind {
                13 | 12 => &[10, 12, 13],
                10 => &[11],
                5 | 6 => &[2, 5, 6],
                _ => continue,
            };
            let mut c: Vec<u32> = (4..o.data.len().saturating_sub(3))
                .filter_map(|i| u32le(&o.data, i))
                .filter(|w| *w != id && objs.get(w).is_some_and(|x| want.contains(&x.kind)))
                .collect();
            c.sort_unstable();
            c.dedup();
            children.insert(id, c);
        }
        Graph {
            objs,
            trees,
            children,
        }
    }

    fn kind(&self, id: u32) -> Option<u8> {
        self.objs.get(&id).map(|o| o.kind)
    }

    /// Media ids under `id`; nested music switches are entered only if `through_switches`.
    fn media(&self, id: u32, through_switches: bool) -> HashSet<u32> {
        self.media_and_switches(id, through_switches).0
    }

    /// Media ids under `id` and, unless `through_switches`, the music switches below it that were not entered
    /// (`id` itself if it is one).
    fn media_and_switches(&self, id: u32, through_switches: bool) -> (HashSet<u32>, Vec<u32>) {
        let mut out = HashSet::new();
        let mut switches = Vec::new();
        if !through_switches && self.trees.contains_key(&id) {
            return (out, vec![id]);
        }
        let mut seen = HashSet::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if !seen.insert(n) {
                continue;
            }
            let Some(o) = self.objs.get(&n) else { continue };
            match o.kind {
                11 => {
                    let count = u32le(&o.data, 5).unwrap_or(0).min(64) as usize;
                    out.extend((0..count).filter_map(|k| u32le(&o.data, 9 + 14 * k + 5)));
                }
                2 => out.extend(u32le(&o.data, 9)),
                12 if n != id && !through_switches => switches.push(n),
                _ => {
                    if let Some((_, leaves)) = self.trees.get(&n) {
                        stack.extend(leaves.iter().map(|(_, v)| *v));
                    }
                    if let Some(c) = self.children.get(&n) {
                        stack.extend(c);
                    }
                }
            }
        }
        (out, switches)
    }

    /// Labels the media under switch `sw` with `prefix` + the state names of each leaf's path, then goes on into
    /// the switches below each leaf with that leaf's own names as their prefix (only one level is carried, which
    /// keeps the walk to one visit per leaf and parent leaf).
    /// A parent leaf whose state group the child switch also tests only reaches the child's leaves with the same
    /// value; other combinations never play and are skipped.
    fn label(
        &self,
        sw: u32,
        prefix: &[(u32, u32)],
        state_name: &HashMap<u32, String>,
        seen: &mut HashSet<(u32, Vec<(u32, u32)>)>,
        path_of: &mut HashMap<u32, Vec<String>>,
    ) {
        if !seen.insert((sw, prefix.to_vec())) {
            return;
        }
        let Some((groups, leaves)) = self.trees.get(&sw) else {
            return;
        };
        'leaf: for (path, node) in leaves {
            // (group, value) of the leaf's non-wildcard keys
            let own: Vec<(u32, u32)> = path
                .iter()
                .zip(groups)
                .filter(|(k, _)| **k != 0)
                .map(|(k, g)| (*g, *k))
                .collect();
            let mut label: Vec<(u32, u32)> = Vec::new();
            for &(g, k) in prefix.iter().chain(&own) {
                match label.iter().find(|(lg, _)| *lg == g) {
                    Some(&(_, lk)) if lk != k => continue 'leaf,
                    Some(_) => {}
                    None => label.push((g, k)),
                }
            }
            let (media, below) = self.media_and_switches(*node, false);
            // the named values of the path (unnamed ones are left out)
            let names: Vec<&String> = label
                .iter()
                .filter_map(|(_, k)| state_name.get(k))
                .collect();
            if !names.is_empty() {
                // conditions such as WARP_OFF or POKECEN_OUT say little; leave them out when more is left
                let mut shown: Vec<&str> = names
                    .iter()
                    .map(|n| n.as_str())
                    .filter(|n| !n.ends_with("_OFF") && !n.ends_with("_OUT"))
                    .collect();
                if shown.is_empty() {
                    shown = names.iter().map(|n| n.as_str()).collect();
                }
                let text = shown.join(" / ");
                for m in media {
                    let v = path_of.entry(m).or_default();
                    if !v.contains(&text) {
                        v.push(text.clone());
                    }
                }
            }
            for b in below {
                self.label(b, &own, state_name, seen, path_of);
            }
        }
    }

    /// The (kind, target) of each action of event `id`: Play → node, Set State → (group, value).
    fn actions(&self, id: u32) -> Vec<Action> {
        let Some(e) = self.objs.get(&id) else {
            return Vec::new();
        };
        let mut ids: Vec<u32> = (4..e.data.len().saturating_sub(3))
            .filter_map(|i| u32le(&e.data, i))
            .filter(|w| self.kind(*w) == Some(3))
            .collect();
        ids.dedup();
        ids.iter()
            .filter_map(|a| {
                let d = &self.objs[a].data;
                let kind = u16le(d, 4)?;
                if kind >> 8 == 0x04 {
                    Some(Action::Play(u32le(d, 6)?))
                } else if kind == 0x1204 && d.len() >= 8 {
                    Some(Action::SetState(
                        u32le(d, d.len() - 8)?,
                        u32le(d, d.len() - 4)?,
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
}

enum Action {
    Play(u32),
    SetState(u32, u32),
}

/// Names found for media files, by media id.
#[derive(Default)]
pub struct Names {
    /// The game's own title (club room songs, species for cries).
    pub title: HashMap<u32, String>,
    /// The developers' name: a state path or an event name.
    pub dev: HashMap<u32, String>,
    pub notes: Vec<String>,
}

fn strings(b: &[u8], out: &mut HashSet<String>) {
    let mut start = None;
    for (i, &c) in b.iter().chain(std::iter::once(&0)).enumerate() {
        let ok = c.is_ascii_alphanumeric() || c == b'_';
        match (ok, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if (3..=100).contains(&(i - s)) {
                    out.insert(String::from_utf8_lossy(&b[s..i]).into_owned());
                }
                start = None;
            }
            _ => {}
        }
    }
}

const TABLES: [&str; 2] = [
    "audio/fb/env_poke_voice/env_poke_voice_lottery_settings/env_poke_voice_lottery_settings_data.bin",
    "audio/exl/fb/EnvPokeVoiceChainData.bin",
];

/// Finds names for the media of `banks` ((bank name, HIRC bytes)).
pub fn find(romfs: &Path, banks: &[(&str, Vec<u8>)], language: &str) -> Names {
    let mut names = Names::default();
    let g = Graph::new(banks);

    // candidate names: every string in the audio tables, and the cry events
    let mut cand = HashSet::new();
    let mut ar = match Archive::open(romfs) {
        Ok(a) => Some(a),
        Err(e) => {
            names.notes.push(format!("names: {e:#}"));
            None
        }
    };
    if let Some(ar) = ar.as_mut() {
        for t in TABLES {
            match ar.pack_files(t) {
                Ok(files) => {
                    for h in files {
                        if let Ok(b) = ar.read_hash(h) {
                            strings(&b, &mut cand);
                        }
                    }
                }
                Err(e) => names.notes.push(format!("names: {e:#}")),
            }
        }
    }
    for n in 0..=1100 {
        cand.insert(format!("PLAY_PV_{n:04}"));
    }
    let events: Vec<(u32, &String)> = cand
        .iter()
        .map(|s| (fnv1(s), s))
        .filter(|(h, _)| g.kind(*h) == Some(4))
        .collect();

    // state values: set-state event names without their prefix, if they are tree keys
    let keys: HashSet<u32> = g
        .trees
        .values()
        .flat_map(|(_, l)| l.iter().flat_map(|(p, _)| p.iter().copied()))
        .filter(|k| *k != 0)
        .collect();
    let mut state_name: HashMap<u32, String> = HashMap::new();
    for s in &cand {
        for name in [
            Some(s.as_str()),
            s.strip_prefix("SET_STATE_"),
            s.strip_prefix("SET_SWITCH_"),
        ]
        .into_iter()
        .flatten()
        {
            let h = fnv1(name);
            if keys.contains(&h) {
                state_name.insert(h, name.to_string());
            }
        }
    }

    // state paths: for each switch leaf whose keys are all named, its media without entering nested switches
    let mut path_of: HashMap<u32, Vec<String>> = HashMap::new();
    let nested: HashSet<u32> = g
        .trees
        .values()
        .flat_map(|(_, l)| l.iter().map(|(_, n)| *n))
        .chain(g.children.values().flatten().copied())
        .filter(|n| g.trees.contains_key(n))
        .collect();
    let mut seen = HashSet::new();
    let mut tops: Vec<u32> = g
        .trees
        .keys()
        .copied()
        .filter(|s| !nested.contains(s))
        .collect();
    tops.sort_unstable();
    // from the top switches down (paths through several switches), then every switch on its own
    let mut rest: Vec<u32> = g
        .trees
        .keys()
        .copied()
        .filter(|s| nested.contains(s))
        .collect();
    rest.sort_unstable();
    for sw in tops.into_iter().chain(rest) {
        g.label(sw, &[], &state_name, &mut seen, &mut path_of);
    }
    // the longest paths say the most
    for v in path_of.values_mut() {
        v.sort_by_key(|p| std::cmp::Reverse(p.matches(" / ").count()));
    }

    // events: what each reaches; each media file keeps the event(s) reaching the fewest files
    let mut by_state: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (groups, leaves) in g.trees.values() {
        for (path, node) in leaves {
            for (gi, k) in path.iter().enumerate() {
                if *k != 0 {
                    by_state.entry((groups[gi], *k)).or_default().push(*node);
                }
            }
        }
    }
    let mut best: HashMap<u32, (usize, Vec<&String>)> = HashMap::new();
    let mut cry: HashMap<u32, (usize, u32)> = HashMap::new();
    for (eid, name) in &events {
        // plays add up; of several state changes the one reaching the fewest files says the most (the club room
        // events set the song, 2 leaves, and an "inside the club room" state shared by all 28 songs)
        let mut reach = HashSet::new();
        let mut state_reach: Option<HashSet<u32>> = None;
        for a in g.actions(*eid) {
            match a {
                Action::Play(t) => reach.extend(g.media(t, true)),
                Action::SetState(gr, st) => {
                    let mut r = HashSet::new();
                    for n in by_state.get(&(gr, st)).into_iter().flatten() {
                        r.extend(g.media(*n, true));
                    }
                    if !r.is_empty() && state_reach.as_ref().is_none_or(|s| r.len() < s.len()) {
                        state_reach = Some(r);
                    }
                }
            }
        }
        reach.extend(state_reach.unwrap_or_default());
        let n = reach.len();
        let dex = name
            .strip_prefix("PLAY_PV_")
            .and_then(|d| d.parse::<u32>().ok());
        for m in reach {
            let b = best.entry(m).or_insert((usize::MAX, Vec::new()));
            if n < b.0 {
                *b = (n, vec![name]);
            } else if n == b.0 {
                b.1.push(name);
            }
            if let Some(d) = dex {
                let c = cry.entry(m).or_insert((usize::MAX, d));
                if n < c.0 {
                    *c = (n, d);
                }
            }
        }
    }
    let mut exact = HashSet::new();
    for (m, (n, mut ev)) in best {
        if n == 1 {
            exact.insert(m);
        }
        if n <= 4 {
            ev.sort();
            let shown: Vec<String> = ev
                .iter()
                .take(2)
                .map(|e| e.strip_prefix("PLAY_").unwrap_or(e).to_string())
                .collect();
            let more = if ev.len() > 2 { ", …" } else { "" };
            names.dev.insert(m, format!("{}{more}", shown.join(", ")));
        }
    }
    // a state path says more than an event shared by several files
    for (m, paths) in &path_of {
        if !exact.contains(m) {
            let more = if paths.len() > 2 { ", …" } else { "" };
            names.dev.insert(
                *m,
                format!("{}{more}", paths[..paths.len().min(2)].join(", ")),
            );
        }
    }

    // titles: club room songs, species
    if let Some(ar) = ar.as_mut() {
        match msg::read(ar, language, "common/clubroom_bgm") {
            Ok(lines) => {
                let title: HashMap<String, String> = lines
                    .into_iter()
                    .filter_map(|(l, t)| {
                        Some((l.strip_prefix("clubroom_bgm_name_")?.to_string(), t))
                    })
                    .filter(|(n, _)| n != "00") // "Turn the speaker off"
                    .collect();
                // the speaker's song is the state CLUBROOM_NN on the path to the file
                titles_from_paths(&path_of, "CLUBROOM_", &title, &mut names.title);
            }
            Err(e) => names.notes.push(format!("club room titles: {e:#}")),
        }
        // the battle music selector: VS_SELECT_TYPE_NN is its entry msg_ui_btl_bgm_select_vsNN
        match msg::read(ar, language, "common/btl_bgm_select") {
            Ok(lines) => {
                let title: HashMap<String, String> = lines
                    .into_iter()
                    .filter_map(|(l, t)| {
                        let n = l.strip_prefix("msg_ui_btl_bgm_select_vs")?;
                        (n.len() == 2 && !t.is_empty())
                            .then(|| (n.to_string(), t.trim_end_matches('♪').trim().to_string()))
                    })
                    .collect();
                titles_from_paths(&path_of, "VS_SELECT_TYPE_", &title, &mut names.title);
            }
            Err(e) => names.notes.push(format!("battle music titles: {e:#}")),
        }
        match msg::read(ar, language, "common/monsname") {
            Ok(lines) => {
                let species: HashMap<u32, String> = lines
                    .into_iter()
                    .filter_map(|(l, t)| Some((l.strip_prefix("MONSNAME_")?.parse().ok()?, t)))
                    .collect();
                for (m, (_, dex)) in cry {
                    if let Some(s) = species.get(&dex) {
                        names.title.insert(m, format!("{s} ({dex:04})"));
                    }
                }
            }
            Err(e) => names.notes.push(format!("species names: {e:#}")),
        }
    }
    names
}

/// Titles for files whose state paths hold `<state><NN>` for one or two numbers NN that `title` has (the
/// in-game music menus); a file under more selector entries is a shared part and gets none. Titles already set
/// are kept.
fn titles_from_paths(
    path_of: &HashMap<u32, Vec<String>>,
    state: &str,
    title: &HashMap<String, String>,
    out: &mut HashMap<u32, String>,
) {
    for (m, paths) in path_of {
        if out.contains_key(m) {
            continue;
        }
        let mut ts: Vec<&str> = Vec::new();
        for t in paths
            .iter()
            .flat_map(|p| p.split(" / "))
            .filter_map(|s| title.get(s.strip_prefix(state)?))
            .map(|t| t.trim_end_matches('♪').trim())
            .filter(|t| !t.is_empty())
        {
            if !ts.contains(&t) {
                ts.push(t);
            }
        }
        if (1..=2).contains(&ts.len()) {
            out.insert(*m, ts.join(" / "));
        }
    }
}
