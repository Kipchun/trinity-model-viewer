//! The Effects tab: the effect catalogue (`effects.rs`) as a table that can be filtered and sorted. A particle
//! effect opens in the 3D view (double-click, or **View**), played by `particles.rs`; **Back to list** returns.

use crate::archive::Archive;
use crate::effects::{self, Effect, Group, Kind};
use crate::particles::EffectView;
use three_d::{Context, egui};

#[derive(Clone, Copy, PartialEq)]
enum Column {
    Name,
    Kind,
    Group,
    Folder,
    Size,
}

pub struct EffectsTab {
    list: Vec<Effect>,
    lost: usize,
    find: String,
    show_particles: bool,
    show_events: bool,
    /// `None` = every group.
    group: Option<Group>,
    sort: Column,
    descending: bool,
    selected: Option<usize>,
    /// The effect playing in the 3D view.
    pub view: Option<EffectView>,
    /// Row to open on the next `load`.
    open_request: Option<usize>,
    /// Why the last effect could not be opened.
    open_error: Option<String>,
}

impl EffectsTab {
    pub fn new(ar: &Archive) -> EffectsTab {
        let (list, lost) = effects::catalogue(ar);
        EffectsTab {
            list,
            lost,
            find: String::new(),
            show_particles: true,
            show_events: true,
            group: None,
            sort: Column::Group,
            descending: false,
            selected: None,
            view: None,
            open_request: None,
            open_error: None,
        }
    }

    /// Asks to open the first particle effect whose name contains `q` (the `--effects <name>` option).
    pub fn request(&mut self, q: &str) {
        let q = q.to_lowercase();
        self.open_request = self
            .list
            .iter()
            .position(|e| e.kind == Kind::Particles && e.name.to_lowercase().contains(&q));
        self.selected = self.open_request;
    }

    /// True while an effect is shown in the 3D view (instead of the table).
    pub fn viewing(&self) -> bool {
        self.view.is_some()
    }

    /// Opens the effect asked for, if any. Returns where its particles go in the first seconds, to frame the
    /// camera on.
    pub fn load(
        &mut self,
        ar: &mut Archive,
        context: &Context,
    ) -> Option<(three_d::Vec3, three_d::Vec3)> {
        let i = self.open_request.take()?;
        let path = self.list.get(i)?.path.clone()?;
        let opened = ar
            .read(&path)
            .and_then(|b| EffectView::new(context, &path, &b));
        match opened {
            Ok(mut v) => {
                self.open_error = None;
                // run a moment unseen to see where the particles go, then start over
                let mut bounds: Option<(three_d::Vec3, three_d::Vec3)> = None;
                for _ in 0..8 {
                    v.advance(15);
                    if let Some((lo, hi)) = v.bounds() {
                        bounds = Some(match bounds {
                            None => (lo, hi),
                            Some((a, b)) => (a.zip(lo, f32::min), b.zip(hi, f32::max)),
                        });
                    }
                }
                v.restart();
                self.view = Some(v);
                Some(
                    bounds
                        .unwrap_or((three_d::vec3(-0.5, 0.0, -0.5), three_d::vec3(0.5, 1.0, 0.5))),
                )
            }
            Err(err) => {
                self.open_error = Some(format!("{path}: {err:#}"));
                None
            }
        }
    }

    /// Advances the playing effect by `seconds` of wall time.
    pub fn update(&mut self, seconds: f32) {
        if let Some(v) = &mut self.view {
            v.update(seconds);
        }
    }

    /// Left panel while an effect plays: playback, emitter sets and emitters.
    fn player_panel(&mut self, ui: &mut egui::Ui, context: &Context) {
        if ui.button("< Back to list").clicked() {
            self.view = None;
            return;
        }
        let Some(v) = &mut self.view else { return };
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(if v.playing { "Pause" } else { "Play" })
                .clicked()
            {
                v.playing = !v.playing;
            }
            if ui.button("Restart").clicked() {
                v.restart();
                v.playing = true;
            }
            ui.checkbox(&mut v.looping, "Loop");
        });
        ui.add(egui::Slider::new(&mut v.speed, 0.05..=2.0).text("speed"));
        ui.small(format!(
            "frame {:.0} of {:.0} · {} particles",
            v.frame,
            v.length,
            v.particle_count()
        ));
        if v.ptcl.sets.len() > 1 {
            let mut set = v.set;
            egui::ComboBox::from_label("Effect")
                .selected_text(v.ptcl.sets[set].name.clone())
                .show_ui(ui, |ui| {
                    for (i, s) in v.ptcl.sets.iter().enumerate() {
                        ui.selectable_value(&mut set, i, &s.name);
                    }
                });
            if set != v.set {
                v.pick_set(context, set);
            }
        }
        ui.separator();
        ui.label("Emitters");
        ui.horizontal(|ui| {
            if ui.small_button("All").clicked() {
                v.nodes.iter_mut().for_each(|n| n.enabled = true);
            }
            if ui.small_button("None").clicked() {
                v.nodes.iter_mut().for_each(|n| n.enabled = false);
            }
        });
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() * 0.7)
            .show(ui, |ui| {
                for n in &mut v.nodes {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0 * n.depth as f32);
                        let blend = match n.em.blend {
                            1 => " (add)",
                            2 => " (subtract)",
                            3 => " (screen)",
                            4 => " (multiply)",
                            _ => "",
                        };
                        let timing = if n.em.one_time {
                            format!("for {} frames", n.em.duration)
                        } else {
                            "without end".to_string()
                        };
                        ui.checkbox(&mut n.enabled, format!("{}{blend}", n.em.name))
                            .on_hover_text(format!(
                                "life {} frames, {} per emission every {} frames, {timing}
                                 keys: colour {}, alpha {}, scale {}",
                                n.em.life,
                                n.em.rate,
                                n.em.interval.max(0) + 1,
                                n.em.key_counts[0],
                                n.em.key_counts[1],
                                n.em.key_counts[4],
                            ));
                    });
                }
            });
        for w in &v.warnings {
            ui.colored_label(egui::Color32::from_rgb(255, 180, 90), w);
        }
    }
    fn passes(&self, e: &Effect, q: &str) -> bool {
        let kind_ok = match e.kind {
            Kind::Particles => self.show_particles,
            Kind::Events => self.show_events,
        };
        kind_ok
            && self.group.is_none_or(|g| g == e.group)
            && (q.is_empty() || e.name.to_lowercase().contains(q) || e.folder.contains(q))
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
            let by_name = || (&a.folder, &a.name).cmp(&(&b.folder, &b.name));
            let o = match self.sort {
                Column::Name => a.name.cmp(&b.name).then_with(by_name),
                Column::Kind => a.kind.cmp(&b.kind).then_with(by_name),
                Column::Group => a.group.cmp(&b.group).then_with(by_name),
                Column::Folder => by_name(),
                Column::Size => a.size.cmp(&b.size).then_with(by_name),
            };
            if self.descending { o.reverse() } else { o }
        });
        rows
    }

    pub fn left(&mut self, ui: &mut egui::Ui, context: &Context) {
        if self.viewing() {
            self.player_panel(ui, context);
            return;
        }
        let particles = self
            .list
            .iter()
            .filter(|e| e.kind == Kind::Particles)
            .count();
        ui.small(format!(
            "{} particle effects, {} animation effect lists. Double-click a particle effect to play it.",
            particles,
            self.list.len() - particles
        ));
        if self.lost > 0 {
            ui.small(format!(
                "{} names could not be rebuilt into paths",
                self.lost
            ));
        }
        ui.separator();
        ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("find, e.g. smoke or raid"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_particles, "Particles (.ptcl)");
            ui.checkbox(&mut self.show_events, "Anim events (.traef)");
        });
        ui.separator();
        ui.label("Group");
        if ui
            .selectable_label(self.group.is_none(), format!("All ({})", self.list.len()))
            .clicked()
        {
            self.group = None;
        }
        for g in Group::ALL {
            let n = self.list.iter().filter(|e| e.group == g).count();
            if n == 0 {
                continue;
            }
            if ui
                .selectable_label(self.group == Some(g), format!("{} ({n})", g.label()))
                .clicked()
            {
                self.group = Some(g);
            }
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
                self.descending = false;
            }
        }
    }

    pub fn central(&mut self, ui: &mut egui::Ui) {
        let rows = self.shown();
        ui.horizontal(|ui| {
            ui.label(format!("{} of {} shown", rows.len(), self.list.len()));
            ui.separator();
            ui.small("Click a column title to sort; click it again to reverse.");
        });
        ui.separator();
        const NAME: f32 = 300.0;
        const KIND: f32 = 90.0;
        const GROUP: f32 = 90.0;
        const SIZE: f32 = 80.0;
        let spacing = ui.spacing().item_spacing.x;
        // the scroll bar takes some width on the right
        let folder =
            (ui.available_width() - NAME - KIND - GROUP - SIZE - spacing * 5.0 - 16.0).max(120.0);
        ui.horizontal(|ui| {
            self.header(ui, Column::Name, "Name", NAME, false);
            self.header(ui, Column::Kind, "Kind", KIND, false);
            self.header(ui, Column::Group, "Group", GROUP, false);
            self.header(ui, Column::Folder, "Folder", folder, false);
            self.header(ui, Column::Size, "Size", SIZE, true);
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
                    // the whole row is one click target, highlighted when hovered or picked
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
                    if resp.double_clicked() && e.kind == Kind::Particles && e.path.is_some() {
                        open = Some(i);
                    }
                    let mut row = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    let f = if e.folder.is_empty() {
                        "(path not found)"
                    } else {
                        &e.folder
                    };
                    let size = size_text(e.size);
                    for (w, text, right) in [
                        (NAME, e.name.as_str(), false),
                        (KIND, e.kind.label(), false),
                        (GROUP, e.group.label(), false),
                        (folder, f, false),
                        (SIZE, size.as_str(), true),
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
        if open.is_some() {
            self.open_request = open;
        }
    }

    pub fn details(&mut self, ui: &mut egui::Ui) {
        if let Some(err) = &self.open_error {
            ui.colored_label(egui::Color32::from_rgb(255, 120, 120), err);
        }
        if let Some(v) = &self.view {
            ui.heading(v.path.rsplit('/').next().unwrap_or(&v.path));
            ui.small(&v.path);
            ui.separator();
            for s in &v.ptcl.sets {
                let n: usize = s.emitters.iter().map(|e| e.count()).sum();
                ui.label(format!("{}: {n} emitters", s.name));
            }
            ui.label(format!(
                "{} textures · VFXB version {:#x}",
                v.ptcl.texture_names.len(),
                v.ptcl.version
            ));
            ui.separator();
            ui.small(
                "Played from the emitters' settings: timing, volume, speed, gravity, life, scale, colour, \
                 rotation, the first texture and its sprite sheet. The game's colour combiners, second and third \
                 textures, mesh particles and fields are not read yet, so effects look close, not exact.",
            );
            return;
        }
        let Some(e) = self.selected.and_then(|i| self.list.get(i)) else {
            ui.label("Pick an effect in the list.");
            return;
        };
        ui.heading(&e.name);
        match &e.path {
            Some(p) => {
                ui.small(p);
                if ui.small_button("Copy path").clicked() {
                    ui.ctx().copy_text(p.clone());
                }
            }
            None => {
                ui.small("path not rebuilt from the pack name");
            }
        }
        ui.separator();
        ui.label(format!("kind: {} ({})", e.kind.label(), e.kind.ext()));
        ui.label(format!("group: {}", e.group.label()));
        ui.label(format!("pack size: {}", size_text(e.size)))
            .on_hover_text(format!("archive pack #{}", e.pack));
        ui.separator();
        let can_view = e.kind == Kind::Particles && e.path.is_some();
        if ui
            .add_enabled(can_view, egui::Button::new("View"))
            .clicked()
        {
            self.open_request = self.selected;
        }
        match e.kind {
            Kind::Particles => ui.small(
                "Nintendo VFXB particle file: emitter sets and emitters, with their own textures (BNTX), \
                 primitive meshes and compiled shaders. View plays it (double-clicking a row does the same).",
            ),
            Kind::Events => ui.small(
                "Effect events for one animation clip (guess: which effect starts on which frame and bone). \
                 Not decoded yet.",
            ),
        };
    }
}

/// One table cell: `width` wide, content left- or right-aligned and cut short to fit.
fn cell(ui: &mut egui::Ui, width: f32, height: f32, right: bool, add: impl FnOnce(&mut egui::Ui)) {
    let layout = if right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.allocate_ui_with_layout(egui::vec2(width, height), layout, |ui| {
        ui.set_width(width);
        add(ui);
    });
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}
