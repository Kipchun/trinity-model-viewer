//! The Cutscenes tab: every cutscene timeline (`cutscenes.rs`) as a table that can be filtered and sorted. A
//! cutscene opens (double-click, or **Open timeline**) as its timeline: one row per track, its clips as bars on
//! a frame line, coloured by command. Nothing plays yet; this is for browsing what each cutscene does.
//!
//! **Play** shows the cutscene in the 3D view (`cutscene_play.rs`): its scenes and camera, with the current line
//! as a subtitle; the left panel then holds the playback controls.
//!
//! The right panel shows the picked cutscene's sub-scenes, files, sound events and message lines (read from the
//! game's text with `msg.rs`, in the language picked there), or the picked clip's parameters.

use crate::archive::Archive;
use crate::cutscene_play::CutscenePlayer;
use crate::cutscenes::{self, Cutscene, Group};
use crate::effects_tab::{cell, size_text};
use crate::msg;
use crate::timeline::{Clip, FPS, Timeline};
use std::collections::HashMap;
use three_d::egui;

#[derive(Clone, Copy, PartialEq)]
enum Column {
    Name,
    Group,
    Length,
    Tracks,
    Clips,
    Folder,
}

/// A message file's lines (label -> text), or why it could not be read.
type Lines = Result<HashMap<String, String>, String>;

struct Entry {
    c: Cutscene,
    timeline: Result<Timeline, String>,
}

impl Entry {
    fn frames(&self) -> u32 {
        self.timeline.as_ref().map_or(0, |t| t.frames())
    }
    fn tracks(&self) -> usize {
        self.timeline.as_ref().map_or(0, |t| t.tracks.len())
    }
    fn clips(&self) -> usize {
        self.timeline.as_ref().map_or(0, |t| t.clips().count())
    }
}

/// Languages of the game's text, as folder names under `message/dat/`.
const LANGUAGES: [&str; 10] = [
    "English",
    "JPN",
    "JPN_KANJI",
    "French",
    "German",
    "Italian",
    "Spanish",
    "Korean",
    "Simp_Chinese",
    "Trad_Chinese",
];

pub struct CutscenesTab {
    list: Vec<Entry>,
    lost: usize,
    find: String,
    /// `None` = every group.
    group: Option<Group>,
    sort: Column,
    descending: bool,
    selected: Option<usize>,
    /// The cutscene whose timeline fills the central panel.
    open: Option<usize>,
    /// Pixels per frame in the timeline view.
    zoom: f32,
    /// The clip picked in the timeline view: (track, group, clip).
    clip: Option<(usize, usize, usize)>,
    language: &'static str,
    /// Message files read so far, by `script/<name>` and language: label -> text, or why it failed.
    messages: HashMap<(String, &'static str), Lines>,
    /// The cutscene playing in the 3D view.
    pub player: Option<CutscenePlayer>,
    /// Row to play on the next `load`.
    play_request: Option<usize>,
    /// Frame the free camera on the scenes on the next `load`.
    reframe: bool,
}

impl CutscenesTab {
    pub fn new(ar: &mut Archive) -> CutscenesTab {
        let (list, lost) = cutscenes::catalogue(ar);
        let list = list
            .into_iter()
            .map(|c| {
                let timeline = ar
                    .read(&c.path)
                    .and_then(|b| Timeline::parse(&b))
                    .map_err(|e| format!("{e:#}"));
                Entry { c, timeline }
            })
            .collect();
        CutscenesTab {
            list,
            lost,
            find: String::new(),
            group: None,
            sort: Column::Group,
            descending: false,
            selected: None,
            open: None,
            zoom: 0.5,
            clip: None,
            language: "English",
            messages: HashMap::new(),
            player: None,
            play_request: None,
            reframe: false,
        }
    }

    /// Opens the timeline of the first cutscene whose name contains `q` (the `--cutscenes <name>` option).
    pub fn request(&mut self, q: &str) {
        let q = q.to_lowercase();
        if let Some(i) = self.list.iter().position(|e| e.c.name.contains(&q)) {
            self.selected = Some(i);
            self.open_timeline(i);
        }
    }

    /// Jumps the playing cutscene to `frame` and pauses (the `--frame` option with `--play`).
    pub fn seek(&mut self, frame: f32) {
        if let Some(p) = &mut self.player {
            p.frame = frame.clamp(0.0, p.frames as f32);
            p.playing = false;
        }
    }

    /// Asks to play the first cutscene whose name contains `q` (the `--play <name>` option).
    pub fn request_play(&mut self, q: &str) {
        let q = q.to_lowercase();
        if let Some(i) = self.list.iter().position(|e| e.c.name.contains(&q)) {
            self.selected = Some(i);
            self.play_request = Some(i);
        }
    }

    /// True while a cutscene plays in the 3D view (instead of the table or timeline).
    pub fn viewing(&self) -> bool {
        self.player.is_some()
    }

    /// Opens the cutscene asked for, if any. Returns a box to frame the free camera on.
    pub fn load(
        &mut self,
        ar: &mut Archive,
        context: &three_d::Context,
    ) -> Option<(three_d::Vec3, three_d::Vec3)> {
        if let Some(i) = self.play_request.take()
            && let Ok(t) = &self.list[i].timeline
        {
            let path = self.list[i].c.path.clone();
            self.player = Some(CutscenePlayer::new(ar, &path, t));
            self.reframe = true;
        }
        // one scene per frame, so the window keeps drawing; frame the free camera once the first is in
        if let Some(p) = &mut self.player
            && p.build_next(context, ar)
            && p.loading() + 1 == p.scene_states().len()
        {
            self.reframe = true;
        }
        if std::mem::take(&mut self.reframe) {
            return self.player.as_ref().and_then(|p| p.bounds());
        }
        None
    }

    /// Advances the playing cutscene by `seconds` and points `camera` along its shot. Returns whether the cutscene
    /// drives the camera (then the mouse does not).
    pub fn update(&mut self, ar: &mut Archive, seconds: f32, camera: &mut three_d::Camera) -> bool {
        let Some(p) = &mut self.player else {
            return false;
        };
        p.update(seconds);
        let driven = p.drive(camera);
        p.prepare(ar, camera.position());
        driven
    }

    /// Left panel while a cutscene plays.
    fn player_panel(&mut self, ui: &mut egui::Ui) {
        if ui.button("< Back to list").clicked() {
            self.player = None;
            return;
        }
        let Some(p) = &mut self.player else { return };
        ui.heading(&p.name);
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(if p.playing { "Pause" } else { "Play" })
                .clicked()
            {
                if !p.playing && p.frame >= p.frames as f32 {
                    p.frame = 0.0;
                }
                p.playing = !p.playing;
            }
            if ui.button("Restart").clicked() {
                p.restart();
            }
            ui.checkbox(&mut p.looping, "Loop");
        });
        let frames = p.frames as f32;
        ui.add(egui::Slider::new(&mut p.frame, 0.0..=frames).text("frame"));
        ui.add(egui::Slider::new(&mut p.speed, 0.1..=2.0).text("speed"));
        ui.small(format!(
            "{} of {}",
            time_text(p.frame as u32),
            time_text(p.frames)
        ));
        ui.separator();
        let had = p.use_camera;
        ui.checkbox(&mut p.use_camera, "Cutscene camera")
            .on_hover_text("Off: orbit freely (left drag, right drag, wheel)");
        if had && !p.use_camera {
            self.reframe = true;
        }
        match p.shot() {
            Some((name, s)) => ui.small(format!(
                "shot {name} · fov {:.0}° · at ({:.1}, {:.1}, {:.1})",
                s.fov.to_degrees(),
                s.position.x,
                s.position.y,
                s.position.z
            )),
            None => ui.small("no shot playing"),
        };
        ui.separator();
        ui.label("Scenes");
        if p.loading() > 0 {
            ui.small(format!(
                "loading {} of {} scenes{}",
                p.loading(),
                p.scene_states().len(),
                if p.waiting() {
                    " (waiting for this one)"
                } else {
                    ""
                }
            ));
        }
        for (name, path, shown, n) in p.scene_states() {
            let text = match n {
                Some(n) => format!("{name} ({n})"),
                None => format!("{name} (loading)"),
            };
            let l = if shown {
                egui::RichText::new(text)
            } else {
                egui::RichText::new(text).weak()
            };
            ui.label(l).on_hover_text(format!(
                "{path}\n{}",
                if shown { "shown now" } else { "not loaded now" }
            ));
        }
        let actors = p.actor_states();
        if !actors.is_empty() {
            ui.separator();
            egui::CollapsingHeader::new(format!("Cast ({})", actors.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for crate::cutscene_play::ActorState {
                        key,
                        built,
                        shown,
                        cut,
                        notes,
                    } in &actors
                    {
                        let state = match built {
                            None => "loading".to_string(),
                            Some(false) => "failed".to_string(),
                            Some(true) if *cut > 0 => format!("cut {cut}"),
                            Some(true) => "no cut".to_string(),
                        };
                        let t = format!("{key} ({state})");
                        let mut rt = egui::RichText::new(t);
                        if !shown {
                            rt = rt.weak();
                        }
                        if *built == Some(false) {
                            rt = rt.color(egui::Color32::from_rgb(255, 140, 120));
                        }
                        let r = ui.label(rt);
                        if !notes.is_empty() {
                            r.on_hover_text(notes.join("\n"));
                        }
                    }
                });
        }
        if !p.warnings.is_empty() {
            ui.separator();
            egui::CollapsingHeader::new(format!("Warnings ({})", p.warnings.len())).show(
                ui,
                |ui| {
                    for w in &p.warnings {
                        ui.colored_label(egui::Color32::from_rgb(255, 180, 90), w);
                    }
                },
            );
        }
        ui.separator();
        ui.small(
            "Shown: the cutscene's rooms and buildings, its camera, and its cast in their clips. Not yet: \
             turning and scaling, things held or attached, faces, effects, lights, fades and sound.",
        );
    }

    fn open_timeline(&mut self, i: usize) {
        self.open = Some(i);
        self.clip = None;
        // fit the whole cutscene in about 1200 pixels to start with
        let frames = self.list[i].frames().max(1) as f32;
        self.zoom = (1200.0 / frames).clamp(0.05, 8.0);
    }

    fn passes(&self, e: &Entry, q: &str) -> bool {
        self.group.is_none_or(|g| g == e.c.group)
            && (q.is_empty() || e.c.name.to_lowercase().contains(q) || e.c.folder.contains(q))
    }

    /// Indices of the rows shown, in the chosen order.
    fn shown(&self) -> Vec<usize> {
        let q = self.find.to_lowercase();
        let mut rows: Vec<usize> = (0..self.list.len())
            .filter(|&i| self.passes(&self.list[i], &q))
            .collect();
        let l = &self.list;
        rows.sort_by(|&a, &b| {
            let (a, b) = (&l[a], &l[b]);
            let by_name = || a.c.name.cmp(&b.c.name);
            let o = match self.sort {
                Column::Name => by_name(),
                Column::Group => a.c.group.cmp(&b.c.group).then_with(by_name),
                Column::Length => a.frames().cmp(&b.frames()).then_with(by_name),
                Column::Tracks => a.tracks().cmp(&b.tracks()).then_with(by_name),
                Column::Clips => a.clips().cmp(&b.clips()).then_with(by_name),
                Column::Folder => (&a.c.folder, &a.c.name).cmp(&(&b.c.folder, &b.c.name)),
            };
            if self.descending { o.reverse() } else { o }
        });
        rows
    }

    pub fn left(&mut self, ui: &mut egui::Ui) {
        if self.viewing() {
            self.player_panel(ui);
            return;
        }
        let failed = self.list.iter().filter(|e| e.timeline.is_err()).count();
        ui.small(format!(
            "{} cutscene timelines. Double-click one to see its timeline. Playback is not done yet.",
            self.list.len()
        ));
        if failed > 0 {
            ui.colored_label(
                egui::Color32::from_rgb(255, 180, 90),
                format!("{failed} could not be read"),
            );
        }
        if self.lost > 0 {
            ui.small(format!(
                "{} pack names could not be rebuilt into paths",
                self.lost
            ));
        }
        ui.separator();
        ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("find, e.g. d010 or botan"));
        ui.separator();
        ui.label("Group");
        if ui
            .selectable_label(self.group.is_none(), format!("All ({})", self.list.len()))
            .clicked()
        {
            self.group = None;
        }
        for g in Group::ALL {
            let n = self.list.iter().filter(|e| e.c.group == g).count();
            if n == 0 {
                continue;
            }
            if ui
                .selectable_label(self.group == Some(g), format!("{} ({n})", g.label()))
                .on_hover_text(group_hint(g))
                .clicked()
            {
                self.group = Some(g);
            }
        }
        ui.separator();
        egui::ComboBox::from_label("Text language")
            .selected_text(self.language)
            .show_ui(ui, |ui| {
                for l in LANGUAGES {
                    ui.selectable_value(&mut self.language, l, l);
                }
            });
        ui.separator();
        ui.label("Clip colours");
        for (fam, label) in FAMILIES {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, fam.color());
                ui.small(*label);
            });
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, col: Column, label: &str, width: f32, right: bool) {
        let arrow = match (self.sort == col, self.descending) {
            (true, false) => " ^",
            (true, true) => " v",
            _ => "",
        };
        let b =
            egui::Button::new(egui::RichText::new(format!("{label}{arrow}")).strong()).frame(false);
        let mut clicked = false;
        cell(ui, width, 18.0, right, |ui| {
            clicked = ui.add(b).on_hover_text("Sort by this column").clicked();
        });
        if clicked {
            if self.sort == col {
                self.descending = !self.descending;
            } else {
                self.sort = col;
                // numbers: largest first
                self.descending = matches!(col, Column::Length | Column::Tracks | Column::Clips);
            }
        }
    }

    pub fn central(&mut self, ui: &mut egui::Ui) {
        if self.open.is_some() {
            self.timeline_view(ui);
        } else {
            self.table(ui);
        }
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        let rows = self.shown();
        ui.horizontal(|ui| {
            ui.label(format!("{} of {} shown", rows.len(), self.list.len()));
            ui.separator();
            ui.small("Click a column title to sort; click it again to reverse.");
        });
        ui.separator();
        const NAME: f32 = 280.0;
        const GROUP: f32 = 140.0;
        const LENGTH: f32 = 70.0;
        const NUM: f32 = 60.0;
        let spacing = ui.spacing().item_spacing.x;
        let folder =
            (ui.available_width() - NAME - GROUP - LENGTH - NUM * 2.0 - spacing * 6.0 - 16.0)
                .max(120.0);
        ui.horizontal(|ui| {
            self.header(ui, Column::Name, "Name", NAME, false);
            self.header(ui, Column::Group, "Group", GROUP, false);
            self.header(ui, Column::Length, "Length", LENGTH, true);
            self.header(ui, Column::Tracks, "Tracks", NUM, true);
            self.header(ui, Column::Clips, "Clips", NUM, true);
            self.header(ui, Column::Folder, "Folder", folder, false);
        });
        ui.separator();
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let mut clicked = None;
        let mut open = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for &i in &rows[range] {
                    let e = &self.list[i];
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Sense::click(),
                    );
                    let v = ui.visuals();
                    if self.selected == Some(i) {
                        ui.painter().rect_filled(rect, 2.0, v.selection.bg_fill);
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(rect, 2.0, v.widgets.hovered.weak_bg_fill);
                    }
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                    if resp.double_clicked() && e.timeline.is_ok() {
                        open = Some(i);
                    }
                    let mut row = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    let (length, tracks, clips) = match &e.timeline {
                        Ok(_) => (
                            time_text(e.frames()),
                            e.tracks().to_string(),
                            e.clips().to_string(),
                        ),
                        Err(_) => ("failed".into(), String::new(), String::new()),
                    };
                    for (w, text, right) in [
                        (NAME, e.c.name.as_str(), false),
                        (GROUP, e.c.group.label(), false),
                        (LENGTH, length.as_str(), true),
                        (NUM, tracks.as_str(), true),
                        (NUM, clips.as_str(), true),
                        (folder, e.c.folder.as_str(), false),
                    ] {
                        cell(&mut row, w, row_h, right, |ui| {
                            ui.add(egui::Label::new(text).truncate().selectable(false));
                        });
                    }
                }
            });
        if clicked.is_some() {
            self.selected = clicked;
        }
        if let Some(i) = open {
            self.open_timeline(i);
        }
    }

    fn timeline_view(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.open else { return };
        ui.horizontal(|ui| {
            if ui.button("< Back to list").clicked() {
                self.open = None;
                self.clip = None;
            }
            ui.separator();
            ui.strong(&self.list[i].c.name);
            ui.label(format!(
                "{} frames ({})",
                self.list[i].frames(),
                time_text(self.list[i].frames())
            ));
            ui.separator();
            ui.add(
                egui::Slider::new(&mut self.zoom, 0.02..=8.0)
                    .logarithmic(true)
                    .text("zoom"),
            );
        });
        if self.open.is_none() {
            return;
        }
        ui.small(
            "Hover a clip for its command and parameters; click it to show them on the right.",
        );
        ui.separator();
        let Ok(t) = &self.list[i].timeline else {
            return;
        };
        const LABEL: f32 = 230.0;
        const RULER: f32 = 20.0;
        let row_h = 18.0;
        let zoom = self.zoom;
        let frames = t.frames().max(1);
        let width = LABEL + frames as f32 * zoom + 40.0;
        let height = RULER + row_h * t.tracks.len() as f32 + 8.0;
        let mut picked = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
                let clip = ui.clip_rect();
                let p = ui.painter();
                let v = ui.visuals();
                let text = v.text_color();
                let weak = v.weak_text_color();
                let font = egui::FontId::proportional(12.0);
                // the frame line starts right of the labels, which stay on screen when scrolled sideways
                let x0 = rect.left() + LABEL;
                let x_of = |f: u32| x0 + f as f32 * zoom;
                let top = rect.top() + RULER;
                // rows
                for (ti, track) in t.tracks.iter().enumerate() {
                    let y = top + ti as f32 * row_h;
                    if y + row_h < clip.top() || y > clip.bottom() {
                        continue;
                    }
                    if ti % 2 == 1 {
                        p.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(x0, y),
                                egui::vec2(width - LABEL, row_h),
                            ),
                            0.0,
                            v.faint_bg_color,
                        );
                    }
                    for (gi, g) in track.groups.iter().enumerate() {
                        for (ci, c) in g.clips.iter().enumerate() {
                            let (a, b) = (x_of(c.start), x_of(c.end.max(c.start + 1)));
                            if b < clip.left() + LABEL || a > clip.right() {
                                continue;
                            }
                            let r = egui::Rect::from_min_max(
                                egui::pos2(a, y + 2.0),
                                egui::pos2(b.max(a + 2.0), y + row_h - 2.0),
                            );
                            let sel = self.clip == Some((ti, gi, ci));
                            p.rect_filled(r, 2.0, Family::of(&c.command).color());
                            if sel {
                                p.rect_stroke(
                                    r.expand(1.0),
                                    2.0,
                                    egui::Stroke::new(2.0_f32, v.strong_text_color()),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if r.width() > 40.0 {
                                let short = c.command.rsplit('/').next().unwrap_or(&c.command);
                                p.with_clip_rect(r.intersect(clip)).text(
                                    egui::pos2(
                                        r.left().max(clip.left() + LABEL) + 3.0,
                                        r.center().y,
                                    ),
                                    egui::Align2::LEFT_CENTER,
                                    short,
                                    egui::FontId::proportional(11.0),
                                    egui::Color32::BLACK,
                                );
                            }
                            if let Some(pos) = resp.hover_pos()
                                && pos.x > clip.left() + LABEL
                                && r.expand2(egui::vec2(1.0, 0.0)).contains(pos)
                            {
                                picked = Some((ti, gi, ci));
                            }
                        }
                    }
                }
                // labels, kept at the left edge of the view
                let lx = clip.left();
                p.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(lx, rect.top()),
                        egui::pos2(lx + LABEL - 4.0, rect.bottom()),
                    ),
                    0.0,
                    v.panel_fill,
                );
                for (ti, track) in t.tracks.iter().enumerate() {
                    let y = top + ti as f32 * row_h;
                    if y + row_h < clip.top() || y > clip.bottom() {
                        continue;
                    }
                    let name = if track.name.is_empty() {
                        "(no name)"
                    } else {
                        &track.name
                    };
                    let cond: Vec<String> = track
                        .conditions
                        .iter()
                        .filter(|(_, v)| v != "None")
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    let label = if cond.is_empty() {
                        name.to_string()
                    } else {
                        format!("{name} [{}]", cond.join(", "))
                    };
                    p.with_clip_rect(egui::Rect::from_min_max(
                        egui::pos2(lx, y),
                        egui::pos2(lx + LABEL - 8.0, y + row_h),
                    ))
                    .text(
                        egui::pos2(lx + 4.0, y + row_h / 2.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        font.clone(),
                        text,
                    );
                }
                // ruler, kept at the top of the view: a tick every second, a number every few
                let ry = clip.top();
                p.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(clip.left(), ry),
                        egui::pos2(clip.right(), ry + RULER),
                    ),
                    0.0,
                    v.panel_fill,
                );
                let every = [1u32, 2, 5, 10, 15, 30, 60]
                    .into_iter()
                    .find(|s| *s as f32 * FPS * zoom >= 60.0)
                    .unwrap_or(120);
                let mut s = 0u32;
                while s as f32 * FPS <= frames as f32 {
                    let x = x_of((s as f32 * FPS) as u32);
                    if x >= clip.left() + LABEL && x <= clip.right() {
                        let major = s.is_multiple_of(every);
                        p.line_segment(
                            [
                                egui::pos2(x, ry + if major { 4.0 } else { 12.0 }),
                                egui::pos2(x, ry + RULER),
                            ],
                            (1.0, weak),
                        );
                        if major {
                            p.text(
                                egui::pos2(x + 2.0, ry + 2.0),
                                egui::Align2::LEFT_TOP,
                                format!("{}:{:02}", s / 60, s % 60),
                                egui::FontId::proportional(10.0),
                                weak,
                            );
                        }
                    }
                    s += if every >= 10 { every / 5 } else { 1 };
                }
                // end of the cutscene
                let xe = x_of(frames);
                if xe >= clip.left() + LABEL {
                    p.line_segment(
                        [egui::pos2(xe, ry), egui::pos2(xe, rect.bottom())],
                        (1.0, egui::Color32::from_rgb(220, 90, 90)),
                    );
                }
                if let Some((ti, gi, ci)) = picked {
                    let track = &t.tracks[ti];
                    let c = &track.groups[gi].clips[ci];
                    let resp = resp.on_hover_ui_at_pointer(|ui| {
                        clip_info(ui, &track.name, &track.groups[gi].name, c);
                    });
                    if resp.clicked() {
                        self.clip = Some((ti, gi, ci));
                    }
                } else if resp.clicked() {
                    self.clip = None;
                }
            });
    }

    /// The line the playing cutscene shows now, over the bottom of the window.
    fn subtitle(&mut self, ctx: &egui::Context, ar: &mut Archive) {
        let Some((file, label)) = self
            .player
            .as_ref()
            .and_then(|p| p.message())
            .map(|(f, l)| (f.to_string(), l.to_string()))
        else {
            return;
        };
        let text = match self.lines(ar, &file) {
            Ok(m) => m.get(&label).cloned().unwrap_or_default(),
            Err(_) => String::new(),
        };
        if text.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("cutscene_subtitle"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(170))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(text)
                                .size(18.0)
                                .color(egui::Color32::WHITE),
                        );
                    });
            });
    }

    /// The lines of a message file (`message/dat/JPN/script/x.dat` → `script/x`) in the picked language.
    fn lines(&mut self, ar: &mut Archive, data_path: &str) -> &Lines {
        let name = data_path
            .split_once("/dat/")
            .map(|(_, r)| r)
            .and_then(|r| r.split_once('/'))
            .map_or(data_path, |(_, r)| r)
            .trim_end_matches(".dat")
            .to_string();
        let lang = self.language;
        self.messages
            .entry((name.clone(), lang))
            .or_insert_with(|| {
                msg::read(ar, lang, &name)
                    .map(|v| v.into_iter().collect())
                    .map_err(|e| format!("{e:#}"))
            })
    }

    pub fn details(&mut self, ui: &mut egui::Ui, ar: &mut Archive) {
        self.subtitle(ui.ctx(), ar);
        // a picked clip in the open timeline
        if let (Some(i), Some((ti, gi, ci))) = (self.open, self.clip)
            && let Ok(t) = &self.list[i].timeline
        {
            let track = &t.tracks[ti];
            ui.heading("Clip");
            clip_info(
                ui,
                &track.name,
                &track.groups[gi].name,
                &track.groups[gi].clips[ci],
            );
            ui.separator();
            if ui.small_button("Back to the cutscene").clicked() {
                self.clip = None;
            }
            return;
        }
        let Some(i) = self.open.or(self.selected) else {
            ui.label("Pick a cutscene in the list.");
            return;
        };
        let e = &self.list[i];
        ui.heading(&e.c.name);
        ui.small(&e.c.path);
        if ui.small_button("Copy path").clicked() {
            ui.ctx().copy_text(e.c.path.clone());
        }
        ui.separator();
        ui.label(format!("group: {}", e.c.group.label()))
            .on_hover_text(group_hint(e.c.group));
        if let Some(p) = e.c.pack {
            ui.label(format!("pack size: {}", size_text(e.c.size)))
                .on_hover_text(format!(
                    "archive pack #{p} (shared with the cutscene's other files)"
                ));
        }
        let t = match &e.timeline {
            Ok(t) => t,
            Err(err) => {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), err);
                return;
            }
        };
        ui.label(format!(
            "length: {} frames ({}) · {} tracks · {} clips",
            t.frames(),
            time_text(t.frames()),
            t.tracks.len(),
            t.clips().count()
        ));
        let mut open = false;
        ui.horizontal(|ui| {
            if self.player.is_none()
                && ui
                    .button("Play")
                    .on_hover_text("Show it in the 3D view")
                    .clicked()
            {
                self.play_request = Some(i);
            }
            if self.open.is_none() && ui.button("Open timeline").clicked() {
                open = true;
            }
        });
        if open {
            self.open_timeline(i);
            return;
        }
        // gather what the cutscene uses
        let mut scenes = Vec::new();
        let mut sounds = Vec::new();
        // (frame, the track's conditions, message file, label); story events show each line on one track per
        // game version (`text_A` / `text_B`, PackageType A / B)
        let mut said: Vec<(u32, String, String, String)> = Vec::new();
        for (tr, c) in t.clips() {
            match c.command.as_str() {
                "Scene/LoadSubScene" => {
                    if let Some(s) = c.param("Path").and_then(|v| v.as_str()) {
                        scenes.push(s.to_string());
                    }
                }
                "Audio/PostEventGlobal"
                | "Audio/PostEventAtObject"
                | "Audio/PostEventAtObjectLocation"
                | "Audio/PostEventAtLocation" => {
                    if let Some(s) = c.param("EventName").and_then(|v| v.as_str()) {
                        sounds.push((c.start, s.to_string()));
                    }
                }
                "Title/Message/ShowMessageWindow" => {
                    if let (Some(f), Some(l)) = (
                        c.param("MessageDataPath").and_then(|v| v.as_str()),
                        c.param("Label").and_then(|v| v.as_str()),
                    ) {
                        let cond: Vec<String> = tr
                            .conditions
                            .iter()
                            .filter(|(_, v)| v != "None")
                            .map(|(_, v)| v.strip_prefix("PackageType").unwrap_or(v).to_string())
                            .collect();
                        said.push((c.start, cond.join(", "), f.to_string(), l.to_string()));
                    }
                }
                _ => {}
            }
        }
        scenes.sort();
        scenes.dedup();
        sounds.sort();
        said.sort();
        let files = t.files();
        let commands = t.command_counts();
        // message text, read on demand
        let mut texts = Vec::new();
        for (f, l) in said
            .iter()
            .map(|(_, _, f, l)| (f.clone(), l.clone()))
            .collect::<Vec<_>>()
        {
            let line = match self.lines(ar, &f) {
                Ok(m) => m
                    .get(&l)
                    .cloned()
                    .unwrap_or_else(|| format!("({l} not found)")),
                Err(err) => format!("({err})"),
            };
            texts.push(line);
        }
        let e = &self.list[i];
        let t = e.timeline.as_ref().ok();
        {
            if !said.is_empty() {
                egui::CollapsingHeader::new(format!("Message lines ({})", said.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for ((frame, cond, _, label), text) in said.iter().zip(&texts) {
                            ui.horizontal_wrapped(|ui| {
                                ui.small(time_text(*frame)).on_hover_text(label);
                                if !cond.is_empty() {
                                    ui.small(format!("[{cond}]")).on_hover_text(
                                        "Only shown when this matches (PackageType A / B: one game version each)",
                                    );
                                }
                                ui.label(text);
                            });
                        }
                    });
            }
            if !scenes.is_empty() {
                egui::CollapsingHeader::new(format!("Sub-scenes ({})", scenes.len())).show(
                    ui,
                    |ui| {
                        for s in &scenes {
                            ui.small(s);
                        }
                    },
                );
            }
            if !files.is_empty() {
                egui::CollapsingHeader::new(format!("Files used ({})", files.len())).show(
                    ui,
                    |ui| {
                        for f in &files {
                            ui.small(f);
                        }
                    },
                );
            }
            if !sounds.is_empty() {
                egui::CollapsingHeader::new(format!("Sound events ({})", sounds.len())).show(
                    ui,
                    |ui| {
                        for (frame, s) in &sounds {
                            ui.small(format!("{}  {s}", time_text(*frame)));
                        }
                    },
                );
            }
            egui::CollapsingHeader::new(format!("Commands ({})", commands.len())).show(ui, |ui| {
                for (k, n) in &commands {
                    ui.small(format!("{n:>5}  {k}"));
                }
            });
            if let Some(t) = t {
                egui::CollapsingHeader::new(format!("Tracks ({})", t.tracks.len())).show(
                    ui,
                    |ui| {
                        for tr in &t.tracks {
                            let n = tr.clips().count();
                            let name = if tr.name.is_empty() {
                                "(no name)"
                            } else {
                                &tr.name
                            };
                            ui.small(format!("{name}: {n} clips"));
                        }
                    },
                );
                ui.small(format!("root slots 3 and 4 (not known): {:?}", t.unknown));
            }
        }
    }
}

fn clip_info(ui: &mut egui::Ui, track: &str, group: &str, c: &Clip) {
    ui.strong(&c.command);
    ui.small(format!(
        "frames {}-{} ({} to {})",
        c.start,
        c.end,
        time_text(c.start),
        time_text(c.end)
    ));
    ui.small(format!("track {track} · group {group}"));
    for p in &c.params {
        ui.small(format!("{} = {}", p.name, p.value.text()));
    }
}

fn group_hint(g: Group) -> &'static str {
    match g {
        Group::Story => {
            "demo/ev/: the story's event cutscenes, with sub-scenes, characters and dialogue"
        }
        Group::Battle => "demo/ee/eeNNN: battle scenes (ball throws, captures, entries)",
        Group::TrainerBattle => {
            "demo/ee/ee_bk_*: a trainer's lines during a battle, named after the trainer"
        }
        Group::BattleWait => "demo/ee/ee_wait*: camera loops while a battle waits for input",
        Group::Tera => "demo/ej/: Terastal changes and Tera Raid Battle scenes",
        Group::System => "demo/sd/: title screen, trades, Mystery Gift, picnic and sandwich scenes",
        Group::Test => "test timelines, templates and shared cameras",
    }
}

/// `m:ss.f` of a frame count.
fn time_text(frames: u32) -> String {
    let s = frames as f32 / FPS;
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

/// Command families, for clip colours.
#[derive(Clone, Copy)]
enum Family {
    Camera,
    Animation,
    Object,
    Sound,
    Text,
    Scene,
    Effect,
    Light,
    Other,
}

const FAMILIES: &[(Family, &str)] = &[
    (Family::Camera, "Camera"),
    (Family::Animation, "Animation"),
    (Family::Object, "Objects and models"),
    (Family::Sound, "Sound and music"),
    (Family::Text, "Messages and UI"),
    (Family::Scene, "Scenes"),
    (Family::Effect, "Particles and post effects"),
    (Family::Light, "Lights and colour"),
    (Family::Other, "Other"),
];

impl Family {
    fn of(command: &str) -> Family {
        let c = command.strip_prefix("Title/").unwrap_or(command);
        let head = c.split('/').next().unwrap_or(c);
        match head {
            "CameraAnimation" | "Camera" | "OffScreen" | "MultiView" => Family::Camera,
            "Animation" | "Physics" | "CC" | "EyeBlink" | "LookAt" => Family::Animation,
            "GameObject" | "Model" | "Visibility" | "Pokemon" => Family::Object,
            "Audio" | "BGM" | "PokeVoice" => Family::Sound,
            "Message" | "Layout" | "Fade" | "Hid" | "Awaitable" => Family::Text,
            "Scene" | "Terrain" | "System" => Family::Scene,
            "Particle" | "PostEffect" => Family::Effect,
            "Light" | "BlendColor" | "Weather" => Family::Light,
            _ if c.starts_with("Battle/Camera") => Family::Camera,
            _ => Family::Other,
        }
    }

    fn color(self) -> egui::Color32 {
        let (r, g, b) = match self {
            Family::Camera => (110, 170, 255),
            Family::Animation => (120, 210, 120),
            Family::Object => (150, 200, 200),
            Family::Sound => (255, 170, 80),
            Family::Text => (240, 220, 100),
            Family::Scene => (190, 140, 255),
            Family::Effect => (255, 130, 190),
            Family::Light => (255, 240, 180),
            Family::Other => (170, 170, 170),
        };
        egui::Color32::from_rgb(r, g, b)
    }
}
