//! The Sound tab: every music track and sound effect (see `wwise.rs`), by group, with a player.
//!
//! A worker thread reads the catalogue and decodes the sound picked (`wem::decode`), and also works out the
//! waveform; only the newest request is decoded. Playback goes through rodio: `Track` plays the decoded samples
//! from a shared buffer, so the window can show and move the play position and switch looping at any time.
//! Sounds with loop points (`smpl`) loop between them, others loop whole.

use crate::config;
use crate::wem;
use crate::wwise::{self, Group, Sound};
use std::collections::HashMap;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
use three_d::egui;

const PEAKS: usize = 1200;

enum Request {
    Decode(usize, Box<Sound>),
}

struct Decoded {
    pcm: wem::Pcm,
    /// (min, max) of the channels' mix per column of the waveform.
    peaks: Vec<(f32, f32)>,
}

enum Reply {
    Catalogue(Result<(Vec<Sound>, Vec<String>), String>),
    Decoded(usize, Box<Result<Decoded, String>>),
}

fn worker(romfs: PathBuf, rx: Receiver<Request>, tx: Sender<Reply>) {
    let cat = wwise::catalogue(&romfs).map_err(|e| format!("{e:#}"));
    if tx.send(Reply::Catalogue(cat)).is_err() {
        return;
    }
    while let Ok(mut r) = rx.recv() {
        // only the newest pick matters
        while let Ok(newer) = rx.try_recv() {
            r = newer;
        }
        let Request::Decode(i, s) = r;
        let result = wwise::read(&romfs, &s)
            .and_then(|b| wem::decode(&b))
            .map(|pcm| {
                let peaks = peaks(&pcm);
                Decoded { pcm, peaks }
            })
            .map_err(|e| format!("{e:#}"));
        if tx.send(Reply::Decoded(i, Box::new(result))).is_err() {
            return;
        }
    }
}

fn peaks(pcm: &wem::Pcm) -> Vec<(f32, f32)> {
    let ch = pcm.channels.max(1) as usize;
    let frames = pcm.samples.len() / ch;
    let per = frames.div_ceil(PEAKS).max(1);
    (0..frames.div_ceil(per))
        .map(|k| {
            let (mut lo, mut hi) = (0f32, 0f32);
            for f in k * per..((k + 1) * per).min(frames) {
                let v = pcm.samples[f * ch..f * ch + ch]
                    .iter()
                    .map(|s| *s as f32)
                    .sum::<f32>()
                    / (ch as f32 * 32768.0);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            (lo, hi)
        })
        .collect()
}

/// What the player and the window share.
struct Shared {
    /// Next sample to play (interleaved index).
    pos: AtomicUsize,
    looping: AtomicBool,
    finished: AtomicBool,
}

/// A decoded sound as a rodio source.
struct Track {
    samples: Arc<Vec<i16>>,
    channels: u16,
    rate: u32,
    /// Loop start and end, interleaved indices.
    span: (usize, usize),
    shared: Arc<Shared>,
}

impl Iterator for Track {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let mut p = self.shared.pos.load(Relaxed);
        if p >= self.span.1 && self.shared.looping.load(Relaxed) {
            p = self.span.0;
        }
        let Some(v) = self.samples.get(p) else {
            self.shared.finished.store(true, Relaxed);
            return None;
        };
        self.shared.pos.store(p + 1, Relaxed);
        Some(*v as f32 / 32768.0)
    }
}

impl rodio::Source for Track {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.channels.max(1)).expect("at least one")
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate.max(1)).expect("non-zero")
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Output {
    // the device stops when this is dropped
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
}

/// The sound loaded in the player.
struct Loaded {
    index: usize,
    samples: Arc<Vec<i16>>,
    channels: u16,
    rate: u32,
    looping: Option<(u32, u32)>,
    codec: wem::Codec,
    peaks: Vec<(f32, f32)>,
    shared: Arc<Shared>,
}

impl Loaded {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }
    fn position(&self) -> f32 {
        (self.shared.pos.load(Relaxed) / self.channels.max(1) as usize) as f32
            / self.rate.max(1) as f32
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Id,
    Length,
}

pub struct AudioTab {
    tx: Sender<Request>,
    rx: Receiver<Reply>,
    error: Option<String>,
    notes: Vec<String>,
    sounds: Vec<Sound>,
    ready: bool,
    group: Group,
    /// Only sounds in this file (`Stream.pck`, ...), or all.
    file: Option<&'static str>,
    find: String,
    sort: Sort,
    selected: Option<usize>,
    /// Decoding this one.
    waiting: Option<usize>,
    loaded: Option<Loaded>,
    decode_error: Option<(usize, String)>,
    output: Option<Result<Output, String>>,
    volume: f32,
    looping: bool,
    play_on_pick: bool,
    /// Start playing as soon as the decode arrives.
    autoplay: bool,
    status: Option<String>,
    start_group: Option<String>,
    /// Names the user gave sounds (saved in the config folder), by id; they win over the game's titles.
    names: HashMap<u32, String>,
    /// The name being typed for the selected sound.
    name_edit: String,
    name_edit_for: Option<u32>,
    named_only: bool,
}

fn time(s: f32) -> String {
    let s = s.max(0.0);
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

impl AudioTab {
    pub fn new(romfs: PathBuf, start_group: Option<String>) -> AudioTab {
        let (tx, worker_rx) = channel();
        let (worker_tx, rx) = channel();
        std::thread::spawn(move || worker(romfs, worker_rx, worker_tx));
        AudioTab {
            tx,
            rx,
            error: None,
            notes: Vec::new(),
            sounds: Vec::new(),
            ready: false,
            group: Group::Music,
            file: None,
            find: String::new(),
            sort: Sort::Id,
            selected: None,
            waiting: None,
            loaded: None,
            decode_error: None,
            output: None,
            volume: 0.7,
            looping: true,
            play_on_pick: true,
            autoplay: false,
            status: None,
            start_group,
            names: config::load_sound_names(),
            name_edit: String::new(),
            name_edit_for: None,
            named_only: false,
        }
    }

    fn poll(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Reply::Catalogue(Ok((sounds, notes))) => {
                    self.sounds = sounds;
                    self.notes = notes;
                    self.ready = true;
                    if let Some(q) = self.start_group.take() {
                        let q = q.to_lowercase();
                        if let Some(g) = Group::ALL
                            .iter()
                            .find(|g| g.label().to_lowercase().contains(&q))
                        {
                            self.group = *g;
                        }
                    }
                }
                Reply::Catalogue(Err(e)) => self.error = Some(e),
                Reply::Decoded(i, r) => {
                    if self.waiting == Some(i) {
                        self.waiting = None;
                    }
                    match *r {
                        Ok(d) => {
                            let ch = d.pcm.channels.max(1);
                            let loaded = Loaded {
                                index: i,
                                samples: Arc::new(d.pcm.samples),
                                channels: ch,
                                rate: d.pcm.rate,
                                looping: d.pcm.looping,
                                codec: d.pcm.codec,
                                peaks: d.peaks,
                                shared: Arc::new(Shared {
                                    pos: AtomicUsize::new(0),
                                    looping: AtomicBool::new(self.looping),
                                    finished: AtomicBool::new(false),
                                }),
                            };
                            self.loaded = Some(loaded);
                            self.decode_error = None;
                            if std::mem::take(&mut self.autoplay) {
                                self.play_from(0.0);
                            }
                        }
                        Err(e) => self.decode_error = Some((i, e)),
                    }
                }
            }
        }
    }

    fn output(&mut self) -> Option<&Output> {
        let out = self.output.get_or_insert_with(|| {
            let sink = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| format!("{e}"))?;
            let player = rodio::Player::connect_new(sink.mixer());
            Ok(Output {
                _sink: sink,
                player,
            })
        });
        out.as_ref().ok()
    }

    /// Plays the loaded sound from `seconds`.
    fn play_from(&mut self, seconds: f32) {
        let volume = self.volume;
        let looping = self.looping;
        let Some(l) = &self.loaded else { return };
        let ch = l.channels as usize;
        let frame = ((seconds * l.rate as f32) as usize).min(l.frames());
        l.shared.pos.store(frame * ch, Relaxed);
        l.shared.looping.store(looping, Relaxed);
        l.shared.finished.store(false, Relaxed);
        let span = l
            .looping
            .map(|(s, e)| (s as usize * ch, (e as usize * ch).min(l.samples.len())))
            .unwrap_or((0, l.samples.len()));
        let track = Track {
            samples: l.samples.clone(),
            channels: l.channels,
            rate: l.rate,
            span,
            shared: l.shared.clone(),
        };
        if let Some(out) = self.output() {
            out.player.clear();
            out.player.set_volume(volume);
            out.player.append(track);
            out.player.play();
        }
    }

    fn is_playing(&self) -> bool {
        match (&self.output, &self.loaded) {
            (Some(Ok(o)), Some(l)) => {
                !o.player.is_paused() && !o.player.empty() && !l.shared.finished.load(Relaxed)
            }
            _ => false,
        }
    }

    fn toggle_play(&mut self) {
        if self.loaded.is_none() {
            return;
        }
        let playing = self.is_playing();
        let finished = self
            .loaded
            .as_ref()
            .is_some_and(|l| l.shared.finished.load(Relaxed));
        let empty = !matches!(&self.output, Some(Ok(o)) if !o.player.empty());
        if playing {
            if let Some(Ok(o)) = &self.output {
                o.player.pause();
            }
        } else if finished || empty {
            self.play_from(0.0);
        } else if let Some(Ok(o)) = &self.output {
            o.player.play();
        }
    }

    fn stop(&mut self) {
        if let Some(Ok(o)) = &self.output {
            o.player.clear();
        }
        if let Some(l) = &self.loaded {
            l.shared.pos.store(0, Relaxed);
        }
    }

    fn pick(&mut self, i: usize) {
        if self.selected == Some(i) && self.loaded.as_ref().is_some_and(|l| l.index == i) {
            if self.play_on_pick {
                self.play_from(0.0);
            }
            return;
        }
        self.selected = Some(i);
        self.waiting = Some(i);
        self.autoplay = self.play_on_pick;
        self.tx
            .send(Request::Decode(i, Box::new(self.sounds[i].clone())))
            .ok();
    }

    /// The sound's name: the user's, else the game's title, else the developers' name.
    fn name_of<'a>(&'a self, s: &'a Sound) -> Option<&'a str> {
        self.names
            .get(&s.id)
            .map(String::as_str)
            .or(s.title.as_deref())
            .or(s.dev_name.as_deref())
    }

    /// Indices of the sounds the list shows.
    fn shown(&self) -> Vec<usize> {
        let q = self.find.trim().to_lowercase();
        let mut v: Vec<usize> = (0..self.sounds.len())
            .filter(|&i| {
                let s = &self.sounds[i];
                s.group == self.group
                    && self.file.is_none_or(|f| s.file == f)
                    && (!self.named_only || self.name_of(s).is_some())
                    && (q.is_empty()
                        || s.id.to_string().contains(&q)
                        || format!("{:08x}", s.id).contains(&q)
                        || [self.names.get(&s.id), s.title.as_ref(), s.dev_name.as_ref()]
                            .into_iter()
                            .flatten()
                            .any(|n| n.to_lowercase().contains(&q)))
            })
            .collect();
        if self.sort == Sort::Length {
            let len = |i: &usize| self.sounds[*i].header.as_ref().map_or(0.0, |h| h.seconds());
            v.sort_by(|a, b| len(b).total_cmp(&len(a)));
        }
        v
    }

    /// The left panel: groups and files.
    pub fn left(&mut self, ui: &mut egui::Ui) {
        self.poll();
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
            return;
        }
        if !self.ready {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading the sound banks…");
            });
            return;
        }
        let named = self
            .sounds
            .iter()
            .filter(|s| self.name_of(s).is_some())
            .count();
        ui.small(format!(
            "{} sounds, {named} named: titles from the game's music menus and species for cries, the              developers' names where the game's tables have them, and the names you give.",
            self.sounds.len()
        ));
        ui.separator();
        for g in Group::ALL {
            let in_group: Vec<&Sound> = self.sounds.iter().filter(|s| s.group == g).collect();
            if in_group.is_empty() {
                continue;
            }
            let minutes: f32 = in_group
                .iter()
                .filter_map(|s| s.header.as_ref().ok())
                .map(|h| h.seconds())
                .sum::<f32>()
                / 60.0;
            let text = format!("{}  ({}, {minutes:.0} min)", g.label(), in_group.len());
            if ui.selectable_label(self.group == g, text).clicked() && self.group != g {
                self.group = g;
                self.file = None;
            }
        }
        ui.separator();
        // the files this group's sounds are in
        let mut files: Vec<(&'static str, usize)> = Vec::new();
        for s in self.sounds.iter().filter(|s| s.group == self.group) {
            match files.iter_mut().find(|(f, _)| *f == s.file) {
                Some((_, n)) => *n += 1,
                None => files.push((s.file, 1)),
            }
        }
        if files.len() > 1 {
            ui.label("In");
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.file, None, "all");
                for (f, n) in files {
                    ui.selectable_value(&mut self.file, Some(f), format!("{f} ({n})"));
                }
            })
            .response
            .on_hover_text(
                "Stream.pck holds the streamed sounds; SDC0 / SDC2.pck are two more packages; .bnk files keep \
                 short sounds inside the bank",
            );
        }
        if !self.notes.is_empty() {
            ui.separator();
            ui.collapsing(format!("Problems ({})", self.notes.len()), |ui| {
                for n in &self.notes {
                    ui.small(n);
                }
            });
        }
    }

    /// The right panel: the picked sound.
    pub fn details(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.selected else {
            ui.label("Pick a sound in the list.");
            return;
        };
        let s = self.sounds[i].clone();
        ui.heading(
            self.name_of(&s)
                .map_or_else(|| s.id.to_string(), str::to_string),
        );
        if let Some(t) = &s.title {
            ui.label(format!("title in the game's menus: {t}"));
        }
        if let Some(d) = &s.dev_name {
            ui.label(format!("developer name: {d}")).on_hover_text(
                "The game's own event or music state names that lead to this file,                  from its sound tables, matched by hash",
            );
        }
        // the user's own name for it, saved when Enter is pressed or the field is left
        if self.name_edit_for != Some(s.id) {
            self.name_edit_for = Some(s.id);
            self.name_edit = self.names.get(&s.id).cloned().unwrap_or_default();
        }
        ui.horizontal(|ui| {
            ui.label("Your name");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.name_edit)
                    .hint_text(s.title.as_deref().unwrap_or("e.g. Battle theme"))
                    .desired_width(ui.available_width()),
            );
            if r.lost_focus() {
                let new = self.name_edit.trim().to_string();
                let old = self.names.get(&s.id).cloned().unwrap_or_default();
                if new != old {
                    if new.is_empty() {
                        self.names.remove(&s.id);
                    } else {
                        self.names.insert(s.id, new);
                    }
                    self.status = Some(match config::save_sound_names(&self.names) {
                        Ok(()) => "name saved".to_string(),
                        Err(e) => format!("could not save the name: {e}"),
                    });
                }
            }
        });
        ui.small(format!("id {}", s.id));
        ui.small(format!("hex {:08X}, file name {}.wem", s.id, s.id));
        ui.label(format!("group: {}", s.group.label()));
        if !s.bank.is_empty() {
            ui.label(format!(
                "named by {}.bnk, as a {}",
                s.bank,
                if s.music { "music track" } else { "sound" }
            ));
        }
        ui.label(format!(
            "in {}, at {:#x}, {} KB",
            s.file,
            s.offset,
            s.size / 1024
        ));
        match &s.header {
            Ok(h) => {
                ui.label(format!(
                    "{}, {} channel{}, {} Hz",
                    h.codec.label(),
                    h.channels,
                    if h.channels == 1 { "" } else { "s" },
                    h.rate
                ));
                ui.label(format!("{} samples, {}", h.samples, time(h.seconds())));
                match h.looping {
                    Some((a, b)) => ui.label(format!(
                        "loop points: {} to {}",
                        time(a as f32 / h.rate as f32),
                        time(b as f32 / h.rate as f32)
                    )),
                    None => ui.label("no loop points"),
                };
            }
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
            }
        }
        if let Some((j, e)) = &self.decode_error
            && *j == i
        {
            ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
        }
        ui.separator();
        let ready = self.loaded.as_ref().is_some_and(|l| l.index == i);
        if ui
            .add_enabled(ready, egui::Button::new("Save as WAV…"))
            .on_hover_text(
                "16-bit PCM, as decoded; choose where (keep game files out of shared folders)",
            )
            .clicked()
        {
            self.save_wav();
        }
        if let Some(st) = &self.status {
            ui.small(st);
        }
    }

    fn save_wav(&mut self) {
        let Some(l) = &self.loaded else { return };
        let id = self.sounds[l.index].id;
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{id}.wav"))
            .add_filter("WAV", &["wav"])
            .save_file()
        else {
            return;
        };
        let mut b = Vec::with_capacity(44 + l.samples.len() * 2);
        let data_len = (l.samples.len() * 2) as u32;
        let block = l.channels as u32 * 2;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&l.channels.to_le_bytes());
        b.extend_from_slice(&l.rate.to_le_bytes());
        b.extend_from_slice(&(l.rate * block).to_le_bytes());
        b.extend_from_slice(&(block as u16).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in l.samples.iter() {
            b.extend_from_slice(&s.to_le_bytes());
        }
        self.status = Some(match std::fs::write(&path, b) {
            Ok(()) => format!("saved {}", path.display()),
            Err(e) => format!("could not save: {e}"),
        });
    }

    /// The middle: the player, then the list.
    pub fn central(&mut self, ui: &mut egui::Ui) {
        self.poll();
        if !self.ready {
            return;
        }
        let shown = self.shown();
        self.player(ui);
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.find)
                    .hint_text("find id or name")
                    .desired_width(140.0),
            );
            ui.separator();
            ui.label("Sort");
            ui.selectable_value(&mut self.sort, Sort::Id, "id");
            ui.selectable_value(&mut self.sort, Sort::Length, "longest first");
            ui.separator();
            ui.checkbox(&mut self.play_on_pick, "Play on click");
            ui.checkbox(&mut self.named_only, "Named only");
            ui.separator();
            ui.small(format!("{} shown", shown.len()));
        });
        ui.separator();

        // keys: Space plays / pauses, up / down step through the list
        if !ui.ctx().egui_wants_keyboard_input() {
            if ui.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle_play();
            }
            let step = ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowDown) {
                    1
                } else if i.key_pressed(egui::Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            });
            if step != 0 && !shown.is_empty() {
                let at = self
                    .selected
                    .and_then(|s| shown.iter().position(|x| *x == s))
                    .map_or(0, |p| {
                        (p as i64 + step).clamp(0, shown.len() as i64 - 1) as usize
                    });
                self.pick(shown[at]);
            }
        }

        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
        let mut clicked = None;
        let font = egui::TextStyle::Monospace;
        ui.label(
            egui::RichText::new(format!(
                "{:>10}  {:>8}  {:>2}  {:>6}  {:8}  {:4}  {:10}  {}",
                "id", "length", "ch", "Hz", "codec", "loop", "file", "name"
            ))
            .text_style(font.clone())
            .weak(),
        );
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, shown.len(), |ui, range| {
                for &i in &shown[range] {
                    let s = &self.sounds[i];
                    let line = match &s.header {
                        Ok(h) => format!(
                            "{:>10}  {:>8}  {:>2}  {:>6}  {:8}  {:4}  {:10}  {}",
                            s.id,
                            time(h.seconds()),
                            h.channels,
                            h.rate,
                            h.codec.label(),
                            if h.looping.is_some() { "yes" } else { "" },
                            s.file,
                            self.name_of(s).unwrap_or("")
                        ),
                        Err(e) => format!("{:>10}  {e}", s.id),
                    };
                    let playing_this = self.loaded.as_ref().is_some_and(|l| l.index == i);
                    let mut text = egui::RichText::new(line).text_style(font.clone());
                    if playing_this {
                        text = text.strong();
                    }
                    let r = ui.selectable_label(self.selected == Some(i), text);
                    if r.clicked() {
                        clicked = Some(i);
                    }
                    if self.selected == Some(i) && r.gained_focus() {
                        r.scroll_to_me(None);
                    }
                }
            });
        if let Some(i) = clicked {
            self.pick(i);
        }
    }

    fn player(&mut self, ui: &mut egui::Ui) {
        if let Some(Err(e)) = &self.output {
            ui.colored_label(
                egui::Color32::from_rgb(255, 120, 120),
                format!("No sound output: {e}"),
            );
        }
        let playing = self.is_playing();
        ui.horizontal(|ui| {
            let can = self.loaded.is_some();
            if ui
                .add_enabled(
                    can,
                    egui::Button::new(if playing { "Pause" } else { "Play" }),
                )
                .clicked()
            {
                self.toggle_play();
            }
            if ui.add_enabled(can, egui::Button::new("Stop")).clicked() {
                self.stop();
            }
            if ui
                .checkbox(&mut self.looping, "Loop")
                .on_hover_text(
                    "Between the sound's loop points if it has them, otherwise the whole sound",
                )
                .changed()
                && let Some(l) = &self.loaded
            {
                l.shared.looping.store(self.looping, Relaxed);
            }
            ui.separator();
            ui.label("Volume");
            if ui
                .add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false))
                .changed()
                && let Some(Some(o)) = self.output.as_ref().map(|o| o.as_ref().ok())
            {
                o.player.set_volume(self.volume);
            }
            ui.separator();
            if self.waiting.is_some() {
                ui.spinner();
                ui.label("decoding…");
            } else if let Some(l) = &self.loaded {
                let s = &self.sounds[l.index];
                ui.strong(
                    self.name_of(s)
                        .map_or_else(|| s.id.to_string(), str::to_string),
                );
                ui.label(format!(
                    "{}  {} / {}  ({}, {} Hz)",
                    s.group.label(),
                    time(l.position().min(l.seconds())),
                    time(l.seconds()),
                    l.codec.label(),
                    l.rate
                ));
            }
        });

        // waveform: click or drag to move the play position
        let h = 90.0;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), h),
            egui::Sense::click_and_drag(),
        );
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        let Some(l) = &self.loaded else {
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "pick a sound below",
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
            return;
        };
        let n = l.peaks.len().max(1);
        let mid = rect.center().y;
        let wave = ui.visuals().selection.bg_fill;
        for (k, (lo, hi)) in l.peaks.iter().enumerate() {
            let x = rect.left() + rect.width() * (k as f32 + 0.5) / n as f32;
            p.line_segment(
                [
                    egui::pos2(x, mid - hi * h * 0.48),
                    egui::pos2(x, mid - lo * h * 0.48 + 0.5),
                ],
                egui::Stroke::new(rect.width() / n as f32 + 0.3, wave),
            );
        }
        let total = l.seconds().max(1e-3);
        let x_of = |t: f32| rect.left() + rect.width() * (t / total).clamp(0.0, 1.0);
        if let Some((a, b)) = l.looping {
            let col = egui::Color32::from_rgba_unmultiplied(255, 200, 80, 160);
            for t in [a, b] {
                let x = x_of(t as f32 / l.rate as f32);
                p.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    (1.0, col),
                );
            }
        }
        let x = x_of(l.position().min(total));
        p.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            (2.0, ui.visuals().strong_text_color()),
        );
        if (resp.clicked() || resp.dragged())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let t = (pos.x - rect.left()) / rect.width() * total;
            if self.is_playing() {
                let ch = l.channels as usize;
                let frame = ((t * l.rate as f32) as usize).min(l.frames());
                l.shared.pos.store(frame * ch, Relaxed);
            } else {
                self.play_from(t);
            }
        }
        if playing {
            ui.ctx().request_repaint();
        }
    }
}
