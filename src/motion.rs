//! Body animations of the human characters: trainers and NPCs in chara/model_tr, model_uq and model_vr.
//! Checked against the game's files (`probe --trainer-anims`, 133 human models):
//!
//! Their clips are not next to the model. They live in motion folders chara/motion_<group>/<folder>/, found from
//! pack names (`folders`):
//! - chara/motion_cc_base/cc<NNNN>_<type>/: 25 shared body types (cc0001_kid .. cc0014_fat_f, cc2001 .. cc2011),
//!   each with a clip list <type>.tracr (cc0004_adl_m: 127 clips) and clips <type>_<number>_<name>.tranm, e.g.
//!   cc0004_adl_m_00030_walk01_loop.tranm. The list is the same kind of .tracr as a Pokémon's _base.tracr.
//! - chara/motion_cc_base/<model folder>/ and chara/motion_uq/<model folder>/: a trainer's own full list
//!   <code>_00.tracr (80 models: all of model_uq but tr9999 and tr1370, and tr0420, 0430, 0460, 0510, 0600,
//!   0610, 0880, 0900), and/or per-clip files <code>_00_<number>_<name>.tracr/.tranm/.trmdt of special actions
//!   (karate02_loop, art01_loop, ...; 52 models). tr1370's list has no idle: it is extra clips only.
//! - chara/motion_tr|uq|vr/<model folder>/: face animations (<code>_00_head00.tracr/.tracp, mouth and eye
//!   clips) and spring bones (.trspb); no body clips.
//! - <name>.trmdt names a "motion detector" <name>_motion_detector.trmdd. Three trainers with their own list name a
//!   body type there (tr0420, tr0430: cc0005_adl_f; tr0900: cc0010_cla_m); the others name themselves.
//!
//! The body types are motion styles, not proportions: every body type's idle clip lands on 57 of 58 tracks of each
//! model_tr skeleton (adl_f: 117-121 of 139) and its bone offsets match the rest pose within 3 mm. So any type plays
//! on any trainer; models without their own list get a guessed default (`default_body`) the user can change.
//!
//! The NPC parts in chara/model_vr name no skeleton: they skin onto chara/model_cc_base/model/cc_base.trskl (74
//! bones), every vertex of all 28 parts on a bone. tr0601 and tr0611 ("vat") have no skeleton at all.

use crate::anim;
use crate::archive::Archive;

/// The base skeleton of the NPC parts in chara/model_vr (their models name no skeleton of their own).
pub const VR_BASE_SKELETON: &str = "chara/model_cc_base/model/cc_base.trskl";

/// Motion groups of the human characters.
const GROUPS: [&str; 4] = ["cc_base", "uq", "tr", "vr"];

/// The groups that hold body clips (motion_tr and motion_vr hold face clips and spring bones only).
const BODY_GROUPS: [&str; 2] = ["cc_base", "uq"];

/// The idle clip: every body type and every full list of a trainer has it.
pub const IDLE_CLIP: &str = "00000_defaultwait01_loop";

/// A motion folder chara/motion_<group>/<folder>/ found in the pack names, with the file names its packs give
/// (a pack is named after one of its files only).
pub struct Folder {
    pub group: String,
    pub folder: String,
    pub files: Vec<String>,
}

/// Every motion folder of the human characters, from pack names such as
/// arc/charamotion_cc_basecc0004_adl_mcc0004_adl_m.trmdt.trpak (group cc_base, folder cc0004_adl_m).
pub fn folders(ar: &Archive) -> Vec<Folder> {
    let mut out: Vec<Folder> = Vec::new();
    for n in &ar.pack_names {
        let Some(rest) = n
            .strip_prefix("arc/charamotion_")
            .and_then(|r| r.strip_suffix(".trpak"))
        else {
            continue;
        };
        let Some(group) = GROUPS.iter().find(|g| rest.starts_with(*g)) else {
            continue;
        };
        let Some((folder, file)) = split_folder(&rest[group.len()..]) else {
            continue;
        };
        match out
            .iter_mut()
            .find(|f| f.group == *group && f.folder == folder)
        {
            Some(f) => f.files.push(file.to_string()),
            None => out.push(Folder {
                group: group.to_string(),
                folder: folder.to_string(),
                files: vec![file.to_string()],
            }),
        }
    }
    out
}

/// "tr0420_mountaintr0420_00.trmdt" -> ("tr0420_mountain", "tr0420_00.trmdt"). The folder is two letters, four
/// digits, '_' and a name; the file starts where two letters, 3 or 4 digits and '_' come again (one file of the
/// game writes "tr350_00" for tr0350).
fn split_folder(body: &str) -> Option<(&str, &str)> {
    let b = body.as_bytes();
    if b.len() < 8 || !b[..2].iter().all(u8::is_ascii_lowercase) {
        return None;
    }
    if !b[2..6].iter().all(u8::is_ascii_digit) || b[6] != b'_' {
        return None;
    }
    let starts_code = |s: &[u8]| {
        s.len() > 6
            && s.starts_with(&b[..2])
            && ((s[2..6].iter().all(u8::is_ascii_digit) && s[6] == b'_')
                || (s[2..5].iter().all(u8::is_ascii_digit) && s[5] == b'_'))
    };
    let split = (7..b.len()).find(|&i| starts_code(&b[i..]))?;
    Some((&body[..split], &body[split..]))
}

/// A pack's file name without its extension and without a clip's "_<5 digits>_<name>" tail:
/// "tr0420_00_04501_karate02_loop.tranm" -> "tr0420_00".
fn file_stem(file: &str) -> &str {
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let b = stem.as_bytes();
    let cut = (0..b.len()).find(|&i| {
        b[i] == b'_'
            && b.len() > i + 6
            && b[i + 1..i + 6].iter().all(u8::is_ascii_digit)
            && b[i + 6] == b'_'
    });
    cut.map_or(stem, |i| &stem[..i])
}

/// What is known about one human model's motions.
#[derive(Default)]
pub struct Found {
    /// The model's own full clip list (a .tracr that has the idle clip), if it has one.
    pub own: Option<String>,
    /// Its special clips (karate, painting, ...) from per-clip files in its folder, played on top of a body type.
    pub extra: Vec<(String, String)>,
    /// The body type its .trmdt names (only when that is one of the shared types).
    pub named_body: Option<String>,
}

/// The shared body types (folders cc<NNNN>_<type> in motion_cc_base), sorted.
pub fn body_types(folders: &[Folder]) -> Vec<String> {
    let mut out: Vec<String> = folders
        .iter()
        .filter(|f| f.group == "cc_base" && f.folder.starts_with("cc"))
        .map(|f| f.folder.clone())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// True for the models whose motions are looked up here (trainers and NPCs, not props or the player's parts).
pub fn is_human(model_path: &str) -> bool {
    ["chara/model_tr/", "chara/model_uq/", "chara/model_vr/"]
        .iter()
        .any(|p| model_path.starts_with(p))
}

/// True for the NPC parts that go on `VR_BASE_SKELETON`.
pub fn is_vr_part(model_path: &str) -> bool {
    model_path.starts_with("chara/model_vr/")
}

/// The body type whose clips a model plays at first: `None` (its own list) when it has one, else the type its
/// .trmdt names, else a guess from its folder name (the game's choice is not known yet): children and infants
/// cc0001_kid, names ending in "f" (tribef, villagerf, youthf, ...) cc0005_adl_f, the rest cc0004_adl_m.
pub fn default_body(found: &Found, model_path: &str, bodies: &[String]) -> Option<String> {
    if found.own.is_some() {
        return None;
    }
    if let Some(b) = &found.named_body {
        return Some(b.clone());
    }
    let folder = model_path.rsplit('/').nth(1).unwrap_or("");
    let name = folder.get(7..).unwrap_or("");
    let guess = if ["child", "infant", "kid"].iter().any(|k| name.contains(k)) {
        "cc0001_kid"
    } else if name.ends_with('f') {
        "cc0005_adl_f"
    } else {
        "cc0004_adl_m"
    };
    bodies
        .iter()
        .find(|b| *b == guess)
        .or(bodies.first())
        .cloned()
}

/// The clip list of a body type.
pub fn body_list_path(body: &str) -> String {
    format!("chara/motion_cc_base/{body}/{body}.tracr")
}

/// Looks up the motions of the model at `model_path` (chara/model_xx/<folder>/<stem>.trmdl) in its folders in
/// motion_cc_base and motion_uq. File names tried: the model's stem, <code>_00, and every file a pack is named
/// after (without its extension). Lists of the head (face clips) are skipped.
pub fn find(ar: &mut Archive, folders: &[Folder], model_path: &str) -> Found {
    let mut found = Found::default();
    let mut dirs = model_path.rsplit('/');
    let stem = dirs.next().unwrap_or("").trim_end_matches(".trmdl");
    let folder = dirs.next().unwrap_or("");
    let Some(code) = folder.get(..6) else {
        return found;
    };
    let bodies = body_types(folders);
    let mut lists: Vec<Vec<(String, String)>> = Vec::new();
    for group in BODY_GROUPS {
        let Some(f) = folders
            .iter()
            .find(|f| f.group == group && f.folder == folder)
        else {
            continue;
        };
        let dir = format!("chara/motion_{group}/{folder}/");
        let code00 = format!("{code}_00");
        let owned: Vec<String> = f
            .files
            .iter()
            .flat_map(|file| {
                let name = file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s);
                [name.to_string(), file_stem(file).to_string()]
            })
            .collect();
        let mut names: Vec<&str> = vec![stem, &code00];
        names.extend(owned.iter().map(String::as_str));
        let mut seen: Vec<&str> = Vec::new();
        for &name in &names {
            if seen.contains(&name) || name.contains("_head") {
                continue;
            }
            seen.push(name);
            let label_stem = file_stem(name);
            let tracr = format!("{dir}{name}.tracr");
            if ar.contains(&tracr) {
                let list = anim::list_at(ar, &tracr, label_stem);
                if found.own.is_none() && list.iter().any(|(l, _)| l.ends_with(IDLE_CLIP)) {
                    found.own = Some(tracr);
                } else {
                    lists.push(list);
                }
            }
            let tranm = format!("{dir}{name}.tranm");
            if name != label_stem && ar.contains(&tranm) {
                let label = name[label_stem.len()..].trim_start_matches('_').to_string();
                lists.push(vec![(label, tranm)]);
            }
            let trmdt = format!("{dir}{name}.trmdt");
            if found.named_body.is_none()
                && let Ok(buf) = ar.read(&trmdt)
                && let Some(n) = detector_name(&buf)
                && bodies.contains(&n)
            {
                found.named_body = Some(n);
            }
        }
    }
    for (label, path) in lists.into_iter().flatten() {
        if !found.extra.iter().any(|(_, p)| *p == path) {
            found.extra.push((label, path));
        }
    }
    found.extra.sort();
    found
}

/// The "<name>" of the "<name>_motion_detector.trmdd" string in a .trmdt.
fn detector_name(buf: &[u8]) -> Option<String> {
    const TAIL: &[u8] = b"_motion_detector.trmdd";
    let end = buf.windows(TAIL.len()).position(|w| w == TAIL)?;
    // the name runs back to the string's length prefix
    let start = buf[..end]
        .iter()
        .rposition(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
        .map_or(0, |i| i + 1);
    std::str::from_utf8(&buf[start..end])
        .ok()
        .map(str::to_string)
}

/// The clips a model plays, as (label, .tranm path) with labels like "00030_walk01_loop": the body type's list
/// when `body` is given, else the model's own list; then its extra clips.
pub fn clips(ar: &mut Archive, found: &Found, body: Option<&str>) -> Vec<(String, String)> {
    let mut out = match (body, &found.own) {
        (Some(b), _) => anim::list_at(ar, &body_list_path(b), b),
        (None, Some(tracr)) => {
            let stem = tracr
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".tracr");
            anim::list_at(ar, tracr, file_stem(stem))
        }
        (None, None) => Vec::new(),
    };
    for (label, path) in &found.extra {
        if !out.iter().any(|(_, p)| p == path) {
            out.push((label.clone(), path.clone()));
        }
    }
    out.sort();
    out
}
