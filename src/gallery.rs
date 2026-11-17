//! The Textures tab: browse every texture and sprite in the archive by group (UI & icons, Pokédex, Pokémon, ...)
//! and folder, as a thumbnail grid or one at a time with zoom, channels and array layers.
//!
//! A worker thread opens its own `Archive` on the same RomFS, builds the catalogue (`textures::catalogue`),
//! lists folders' other pack files (`textures::expand`) and decodes textures, so the window never waits on a
//! decode. Thumbnails are asked for only when their cell is on screen; the newest requests go first.

use crate::archive::Archive;
use crate::texture;
use crate::textures::{self, Folder, Group, TexFile};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use three_d::egui;

const THUMB_MAX: u32 = 128;

enum Request {
    Expand(usize, Folder),
    Thumb(u64),
    Full(u64, u32),
}

enum Reply {
    Catalogue(Result<(Vec<Folder>, usize), String>),
    Expanded(usize, Vec<TexFile>),
    Thumb(u64, Result<(egui::ColorImage, String), String>),
    Full(
        u64,
        u32,
        Box<Result<(texture::Image, texture::Info), String>>,
    ),
}

fn worker(romfs: PathBuf, rx: Receiver<Request>, tx: Sender<Reply>) {
    let mut ar = match Archive::open(&romfs) {
        Ok(a) => a,
        Err(e) => {
            tx.send(Reply::Catalogue(Err(format!("{e:#}")))).ok();
            return;
        }
    };
    if tx
        .send(Reply::Catalogue(Ok(textures::catalogue(&ar))))
        .is_err()
    {
        return;
    }
    let mut urgent: VecDeque<Request> = VecDeque::new();
    let mut thumbs: Vec<u64> = Vec::new();
    let queue = |r: Request, urgent: &mut VecDeque<Request>, thumbs: &mut Vec<u64>| match r {
        Request::Thumb(h) => {
            thumbs.retain(|x| *x != h);
            thumbs.push(h);
        }
        r => urgent.push_back(r),
    };
    loop {
        if urgent.is_empty() && thumbs.is_empty() {
            match rx.recv() {
                Ok(r) => queue(r, &mut urgent, &mut thumbs),
                Err(_) => return, // the tab was closed
            }
        }
        while let Ok(r) = rx.try_recv() {
            queue(r, &mut urgent, &mut thumbs);
        }
        let reply = if let Some(r) = urgent.pop_front() {
            match r {
                Request::Expand(i, folder) => {
                    Reply::Expanded(i, textures::expand(&mut ar, &folder).0)
                }
                Request::Full(h, layer) => {
                    let r = ar.read_hash(h).and_then(|b| {
                        let info = texture::info(&b)?;
                        Ok((texture::decode_bntx_layer(&b, layer)?, info))
                    });
                    Reply::Full(h, layer, Box::new(r.map_err(|e| format!("{e:#}"))))
                }
                Request::Thumb(_) => continue,
            }
        } else if let Some(h) = thumbs.pop() {
            let r = ar.read_hash(h).and_then(|b| {
                let info = texture::info(&b)?;
                let img = texture::decode_bntx(&b)?;
                let (w, h, px) = textures::shrink(&img, THUMB_MAX);
                let bytes: Vec<u8> = px.into_iter().flatten().collect();
                let desc = describe(&info, &img.format);
                Ok((
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &bytes),
                    desc,
                ))
            });
            Reply::Thumb(h, r.map_err(|e| format!("{e:#}")))
        } else {
            continue;
        };
        if tx.send(reply).is_err() {
            return;
        }
    }
}

fn describe(info: &texture::Info, format: &str) -> String {
    let mut s = format!("{} x {}  {format}", info.width, info.height);
    if info.layers > 1 {
        s += &format!(", {} layers", info.layers);
    }
    s
}

#[derive(Clone, Copy, PartialEq)]
enum Channel {
    Rgba,
    Rgb,
    R,
    G,
    B,
    A,
}

impl Channel {
    const ALL: [(Channel, &'static str); 6] = [
        (Channel::Rgba, "RGBA"),
        (Channel::Rgb, "RGB"),
        (Channel::R, "R"),
        (Channel::G, "G"),
        (Channel::B, "B"),
        (Channel::A, "A"),
    ];

    fn apply(self, [r, g, b, a]: [u8; 4]) -> [u8; 4] {
        match self {
            Channel::Rgba => [r, g, b, a],
            Channel::Rgb => [r, g, b, 255],
            Channel::R => [r, r, r, 255],
            Channel::G => [g, g, g, 255],
            Channel::B => [b, b, b, 255],
            Channel::A => [a, a, a, 255],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Background {
    Checker,
    Dark,
    Light,
}

/// The texture open in the details panel and the single view.
struct Full {
    hash: u64,
    layer: u32,
    result: Result<(texture::Image, texture::Info), String>,
    /// Uploaded for the channel and filter it was made with.
    tex: Option<(egui::TextureHandle, Channel, bool)>,
}

pub struct Gallery {
    tx: Sender<Request>,
    rx: Receiver<Reply>,
    error: Option<String>,
    folders: Vec<Folder>,
    /// None until the worker sent the catalogue.
    ready: bool,
    group: Group,
    folder: Option<usize>,
    find: String,
    file_filter: String,
    expanding: HashSet<usize>,
    thumbs: HashMap<u64, Result<(egui::TextureHandle, String), String>>,
    requested: HashSet<u64>,
    selected: Option<u64>,
    full: Option<Full>,
    full_requested: Option<(u64, u32)>,
    channel: Channel,
    background: Background,
    smooth: bool,
    layer: u32,
    cell: f32,
    single: bool,
    zoom: f32,
    fit: bool,
    checker: Option<egui::TextureHandle>,
    /// A folder to open once the catalogue is in (`--textures <part of a folder path>`).
    start: Option<String>,
}

impl Gallery {
    /// Starts the worker on the RomFS at `romfs` (opened a second time, read-only).
    pub fn new(romfs: PathBuf, start: Option<String>) -> Gallery {
        let (tx, worker_rx) = channel();
        let (worker_tx, rx) = channel();
        std::thread::spawn(move || worker(romfs, worker_rx, worker_tx));
        Gallery {
            tx,
            rx,
            error: None,
            folders: Vec::new(),
            ready: false,
            group: Group::Ui,
            folder: None,
            find: String::new(),
            file_filter: String::new(),
            expanding: HashSet::new(),
            thumbs: HashMap::new(),
            requested: HashSet::new(),
            selected: None,
            full: None,
            full_requested: None,
            channel: Channel::Rgba,
            background: Background::Checker,
            smooth: false,
            layer: 0,
            cell: 112.0,
            single: false,
            zoom: 1.0,
            fit: true,
            checker: None,
            start,
        }
    }

    /// Takes the worker's replies; uploads thumbnails.
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Reply::Catalogue(Ok((folders, _lost))) => {
                    self.folders = folders;
                    self.ready = true;
                    self.folder = self.folders.iter().position(|f| f.group == self.group);
                    if let Some(q) = self.start.take()
                        && let Some(i) = self.folders.iter().position(|f| f.path.contains(&q))
                    {
                        self.group = self.folders[i].group;
                        self.folder = Some(i);
                    }
                }
                Reply::Catalogue(Err(e)) => self.error = Some(e),
                Reply::Expanded(i, more) => {
                    self.expanding.remove(&i);
                    if let Some(f) = self.folders.get_mut(i) {
                        f.files.extend(more);
                        f.files
                            .sort_by(|a, b| a.name.cmp(&b.name).then(a.hash.cmp(&b.hash)));
                        f.expanded = true;
                    }
                }
                Reply::Thumb(h, r) => {
                    let r = r.map(|(img, desc)| {
                        let tex =
                            ctx.load_texture(format!("thumb{h:016x}"), img, Default::default());
                        (tex, desc)
                    });
                    self.thumbs.insert(h, r);
                }
                Reply::Full(h, layer, r) => {
                    if self.full_requested == Some((h, layer)) {
                        self.full = Some(Full {
                            hash: h,
                            layer,
                            result: *r,
                            tex: None,
                        });
                    }
                }
            }
        }
    }

    fn select_folder(&mut self, i: usize) {
        if self.folder == Some(i) {
            return;
        }
        self.folder = Some(i);
        // thumbnails of other folders: drop them when many piled up (each is up to 64 KB)
        if self.thumbs.len() > 1500 {
            self.thumbs.clear();
            self.requested.clear();
        }
    }

    fn select(&mut self, h: u64) {
        if self.selected != Some(h) {
            self.selected = Some(h);
            self.layer = 0;
            self.fit = true;
        }
    }

    /// Files of the open folder that pass the filter.
    fn shown(&self) -> Vec<&TexFile> {
        let q = self.file_filter.to_lowercase();
        self.folder
            .and_then(|i| self.folders.get(i))
            .map(|f| {
                f.files
                    .iter()
                    .filter(|t| q.is_empty() || t.name.to_lowercase().contains(&q))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The left panel: groups and folders.
    pub fn left(&mut self, ui: &mut egui::Ui) {
        self.poll(ui.ctx());
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
            return;
        }
        if !self.ready {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Finding textures…");
            });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for g in Group::ALL {
                let n = self.folders.iter().filter(|f| f.group == g).count();
                if n == 0 {
                    continue;
                }
                if ui.selectable_label(self.group == g, g.label()).clicked() && self.group != g {
                    self.group = g;
                    if let Some(i) = self.folders.iter().position(|f| f.group == g) {
                        self.select_folder(i);
                    }
                }
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.find)
                .hint_text("find folder or file, e.g. pm0025"),
        );
        let q = self.find.to_lowercase();
        let listed: Vec<usize> = (0..self.folders.len())
            .filter(|&i| {
                let f = &self.folders[i];
                f.group == self.group
                    && (q.is_empty()
                        || f.path.to_lowercase().contains(&q)
                        || f.files.iter().any(|t| t.name.to_lowercase().contains(&q)))
            })
            .collect();
        let textures: usize = listed.iter().map(|&i| self.folders[i].files.len()).sum();
        ui.small(format!(
            "{} folders, {textures} textures listed",
            listed.len()
        ));
        ui.separator();
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let mut pick = None;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() * 0.75)
            .auto_shrink([false, false])
            .show_rows(ui, row_h, listed.len(), |ui, range| {
                for &i in &listed[range] {
                    let f = &self.folders[i];
                    let n = if f.expanded {
                        format!("{}", f.files.len())
                    } else {
                        format!("{}+", f.files.len())
                    };
                    let text = format!("{}  ({n})", f.label());
                    let r = ui.selectable_label(self.folder == Some(i), text);
                    if r.clicked() {
                        pick = Some(i);
                    }
                    r.on_hover_text(if f.path.is_empty() { "?" } else { &f.path });
                }
            });
        if let Some(i) = pick {
            self.select_folder(i);
            // a search for a file name also filters the files shown
            if !q.is_empty() && !self.folders[i].path.to_lowercase().contains(&q) {
                self.file_filter = self.find.clone();
            }
        }
        ui.separator();
        ui.small(
            "A folder's count ends in + until its packs were read: a pack is named after one texture and can hold many more.",
        );
    }

    /// The right panel: what the selected texture is, and how it is shown.
    pub fn details(&mut self, ui: &mut egui::Ui) {
        let Some(h) = self.selected else {
            ui.label("Click a texture to see it here; double-click opens it large.");
            return;
        };
        let file = self
            .folder
            .and_then(|i| self.folders.get(i))
            .and_then(|f| f.files.iter().find(|t| t.hash == h))
            .cloned();
        if let Some(f) = &file {
            ui.heading(&f.name);
            match &f.path {
                Some(p) => ui.small(p),
                None => ui.small("path unknown (named by the texture inside)"),
            };
        }
        ui.small(format!("hash {h:016x}"));
        let Some(full) = self.full.as_ref().filter(|f| f.hash == h) else {
            ui.spinner();
            return;
        };
        let (img, info) = match &full.result {
            Ok(x) => (&x.0, &x.1),
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                return;
            }
        };
        ui.label(format!("{} x {}  {}", img.width, img.height, img.format))
            .on_hover_text(format!("format code {:#06x}", info.format));
        ui.label(format!(
            "{} mip level{}, {} layer{}{}",
            info.mips,
            if info.mips == 1 { "" } else { "s" },
            info.layers,
            if info.layers == 1 { "" } else { "s" },
            if info.textures > 1 {
                format!(
                    " (file holds {} textures; showing the first)",
                    info.textures
                )
            } else {
                String::new()
            }
        ));
        if info.name
            != file
                .as_ref()
                .map(|f| f.name.trim_end_matches(".bntx"))
                .unwrap_or("")
        {
            ui.small(format!("texture name: {}", info.name));
        }
        if img.has_alpha() {
            ui.small("has transparency");
        }
        let layers = info.layers;
        ui.separator();
        self.options(ui, layers);
        ui.separator();
        // the preview, as wide as the panel
        let w = ui.available_width().min(512.0);
        if let Some(tex) = self.full_texture(ui.ctx()) {
            let [tw, th] = tex.size();
            let scale = w / tw.max(th) as f32;
            let size = egui::vec2(tw as f32 * scale, th as f32 * scale);
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
            self.paint_background(ui, rect, 16.0);
            ui.painter().image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if resp.double_clicked() {
                self.single = true;
            }
        }
        if ui.button("Open large").clicked() {
            self.single = true;
        }
    }

    fn options(&mut self, ui: &mut egui::Ui, layers: u32) {
        ui.horizontal(|ui| {
            ui.label("Show");
            for (c, label) in Channel::ALL {
                ui.selectable_value(&mut self.channel, c, label);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Behind");
            ui.selectable_value(&mut self.background, Background::Checker, "Checker");
            ui.selectable_value(&mut self.background, Background::Dark, "Dark");
            ui.selectable_value(&mut self.background, Background::Light, "Light");
        });
        ui.checkbox(&mut self.smooth, "Smooth when enlarged")
            .on_hover_text("Off: every texel is a sharp square, as pixel art and icons are made");
        if layers > 1 {
            ui.add(egui::Slider::new(&mut self.layer, 0..=layers - 1).text("layer"));
        }
    }

    /// The selected texture uploaded with the current channel and filter (re-uploaded when they change).
    fn full_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let (channel, smooth) = (self.channel, self.smooth);
        let full = self.full.as_mut()?;
        let (img, _) = full.result.as_ref().ok()?;
        match &full.tex {
            Some((t, c, s)) if *c == channel && *s == smooth => Some(t.clone()),
            _ => {
                let bytes: Vec<u8> = img.rgba.iter().flat_map(|p| channel.apply(*p)).collect();
                let ci = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &bytes,
                );
                let opts = if smooth {
                    egui::TextureOptions::LINEAR
                } else {
                    egui::TextureOptions::NEAREST
                };
                let t =
                    ctx.load_texture(format!("full{:016x}_{}", full.hash, full.layer), ci, opts);
                full.tex = Some((t.clone(), channel, smooth));
                Some(t)
            }
        }
    }

    fn paint_background(&mut self, ui: &egui::Ui, rect: egui::Rect, square: f32) {
        let p = ui.painter();
        match self.background {
            Background::Dark => {
                p.rect_filled(rect, 0.0, egui::Color32::from_gray(24));
            }
            Background::Light => {
                p.rect_filled(rect, 0.0, egui::Color32::from_gray(235));
            }
            Background::Checker => {
                let tex = self.checker.get_or_insert_with(|| {
                    let (a, b) = (egui::Color32::from_gray(200), egui::Color32::from_gray(150));
                    let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
                    let opts = egui::TextureOptions {
                        wrap_mode: egui::TextureWrapMode::Repeat,
                        ..egui::TextureOptions::NEAREST
                    };
                    ui.ctx().load_texture("checker", img, opts)
                });
                let uv = egui::Rect::from_min_max(
                    egui::pos2(0.0, 0.0),
                    egui::pos2(
                        rect.width() / (2.0 * square),
                        rect.height() / (2.0 * square),
                    ),
                );
                p.image(tex.id(), rect, uv, egui::Color32::WHITE);
            }
        }
    }

    /// The middle of the window: the grid of the open folder, or one texture large.
    pub fn central(&mut self, ui: &mut egui::Ui) {
        self.poll(ui.ctx());
        if !self.ready {
            return;
        }
        // read the open folder's other pack files once
        if let Some(i) = self.folder
            && let Some(f) = self.folders.get(i)
            && !f.expanded
            && self.expanding.insert(i)
        {
            self.tx.send(Request::Expand(i, f.clone())).ok();
        }
        // the selected texture, at the layer asked for
        if let Some(h) = self.selected
            && self.full_requested != Some((h, self.layer))
        {
            self.full_requested = Some((h, self.layer));
            self.tx.send(Request::Full(h, self.layer)).ok();
        }

        let shown: Vec<TexFile> = self.shown().into_iter().cloned().collect();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.single, false, "Grid");
            ui.selectable_value(&mut self.single, true, "Single");
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.file_filter)
                    .hint_text("filter files")
                    .desired_width(160.0),
            );
            if !self.file_filter.is_empty() && ui.small_button("x").clicked() {
                self.file_filter.clear();
            }
            ui.separator();
            if self.single {
                if ui.button("Fit").clicked() {
                    self.fit = true;
                }
                if ui.button("1:1").clicked() {
                    self.fit = false;
                    self.zoom = 1.0;
                }
                let mut pct = self.zoom * 100.0;
                if ui
                    .add(
                        egui::Slider::new(&mut pct, 5.0..=3200.0)
                            .logarithmic(true)
                            .suffix("%")
                            .max_decimals(0),
                    )
                    .changed()
                {
                    self.fit = false;
                    self.zoom = pct / 100.0;
                }
            } else {
                ui.add(egui::Slider::new(&mut self.cell, 48.0..=256.0).text("size"));
            }
            if let Some(i) = self.folder.and_then(|i| self.folders.get(i)) {
                ui.separator();
                if self.expanding.contains(&self.folder.unwrap_or(usize::MAX)) {
                    ui.spinner();
                }
                ui.small(format!("{}: {} shown", i.label(), shown.len()));
            }
        });
        ui.separator();

        // arrow keys step through the folder, unless a text field has the keyboard
        if !ui.ctx().egui_wants_keyboard_input() && !shown.is_empty() {
            let step = ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown) {
                    1
                } else if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            });
            if step != 0 {
                let at = self
                    .selected
                    .and_then(|h| shown.iter().position(|t| t.hash == h))
                    .map_or(0, |i| {
                        (i as i64 + step).rem_euclid(shown.len() as i64) as usize
                    });
                self.select(shown[at].hash);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.single = false;
            }
        }

        if self.single {
            self.single_view(ui);
        } else {
            self.grid(ui, &shown);
        }
    }

    fn grid(&mut self, ui: &mut egui::Ui, shown: &[TexFile]) {
        if shown.is_empty() {
            ui.label(if self.expanding.is_empty() {
                "No textures here."
            } else {
                "Reading the folder's packs…"
            });
            return;
        }
        let cell = self.cell;
        let label_h = ui.text_style_height(&egui::TextStyle::Small) + 2.0;
        let gap = 6.0;
        let cols = (((ui.available_width() + gap) / (cell + gap)).floor() as usize).max(1);
        let rows = shown.len().div_ceil(cols);
        let mut clicked = None;
        let mut opened = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, cell + label_h + gap, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for t in shown.iter().skip(row * cols).take(cols) {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(cell, cell + label_h),
                                egui::Sense::click(),
                            );
                            let square =
                                egui::Rect::from_min_size(rect.min, egui::vec2(cell, cell));
                            self.paint_cell(ui, t, square);
                            let mut name = t.name.trim_end_matches(".bntx").to_string();
                            let max_chars = (cell / 6.5) as usize;
                            if name.chars().count() > max_chars {
                                name = format!(
                                    "…{}",
                                    name.chars()
                                        .skip(name.chars().count() - max_chars + 1)
                                        .collect::<String>()
                                );
                            }
                            ui.painter().text(
                                egui::pos2(rect.center().x, square.max.y + 1.0),
                                egui::Align2::CENTER_TOP,
                                name,
                                egui::FontId::proportional(11.0),
                                ui.visuals().text_color(),
                            );
                            if self.selected == Some(t.hash) {
                                ui.painter().rect_stroke(
                                    square.expand(2.0),
                                    2.0,
                                    egui::Stroke::new(2.0_f32, ui.visuals().selection.stroke.color),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if resp.clicked() {
                                clicked = Some(t.hash);
                            }
                            if resp.double_clicked() {
                                clicked = Some(t.hash);
                                opened = true;
                            }
                            let tip = match self.thumbs.get(&t.hash) {
                                Some(Ok((_, d))) => format!("{}\n{d}", t.name),
                                Some(Err(e)) => format!("{}\n{e}", t.name),
                                None => t.name.clone(),
                            };
                            resp.on_hover_text(tip);
                        }
                    });
                    ui.add_space(gap);
                }
            });
        if let Some(h) = clicked {
            self.select(h);
            self.single |= opened;
        }
    }

    /// One grid cell's picture: the thumbnail fitted into `square`, asked for on first sight.
    fn paint_cell(&mut self, ui: &egui::Ui, t: &TexFile, square: egui::Rect) {
        if self.requested.insert(t.hash) {
            self.tx.send(Request::Thumb(t.hash)).ok();
        }
        match self.thumbs.get(&t.hash) {
            Some(Ok((tex, _))) => {
                let tex = tex.clone();
                let [w, h] = tex.size();
                let s = square.width() / w.max(h) as f32;
                let r = egui::Rect::from_center_size(
                    square.center(),
                    egui::vec2(w as f32 * s, h as f32 * s),
                );
                self.paint_background(ui, r, 8.0);
                ui.painter().image(
                    tex.id(),
                    r,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            Some(Err(_)) => {
                ui.painter()
                    .rect_filled(square, 2.0, egui::Color32::from_rgb(90, 40, 40));
                ui.painter().text(
                    square.center(),
                    egui::Align2::CENTER_CENTER,
                    "can't decode",
                    egui::FontId::proportional(11.0),
                    egui::Color32::WHITE,
                );
            }
            None => {
                ui.painter()
                    .rect_filled(square, 2.0, ui.visuals().faint_bg_color);
            }
        }
    }

    fn single_view(&mut self, ui: &mut egui::Ui) {
        let Some(tex) = self.full_texture(ui.ctx()) else {
            match self.full.as_ref().map(|f| &f.result) {
                Some(Err(e)) if self.full.as_ref().map(|f| f.hash) == self.selected => {
                    ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                }
                _ if self.selected.is_none() => {
                    ui.label("Pick a texture in the grid first.");
                }
                _ => {
                    ui.spinner();
                }
            }
            return;
        };
        let [w, h] = tex.size();
        let avail = ui.available_size();
        if self.fit {
            self.zoom = (avail.x / w as f32)
                .min(avail.y / h as f32)
                .clamp(0.05, 32.0);
        }
        // Ctrl + wheel (or pinch) zooms
        let z = ui.input(|i| i.zoom_delta());
        if z != 1.0 && ui.rect_contains_pointer(ui.max_rect()) {
            self.fit = false;
            self.zoom = (self.zoom * z).clamp(0.05, 32.0);
        }
        let size = egui::vec2(w as f32 * self.zoom, h as f32 * self.zoom);
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // centred when smaller than the view
                let pad = ((avail - size) * 0.5).max(egui::vec2(0.0, 0.0));
                ui.add_space(pad.y);
                ui.horizontal(|ui| {
                    ui.add_space(pad.x);
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    self.paint_background(ui, rect, 16.0);
                    ui.painter().image(
                        tex.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                });
            });
    }
}
