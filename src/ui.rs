//! UI layer — ports the App/Timeline/Track/MidiEditor state machine and rendering from
//! `project.zig`. All audio state lives in the shared `Engine`; this layer holds only
//! UI/navigation state (mode, current screen, cursor/zoom, selection) plus the plugin
//! icon textures, and it locks the engine to read for rendering or to mutate on input.
//!
//! The Zig version returned `Op`s up a call stack to be queued onto the audio thread;
//! under the mutex model we mutate the engine directly while holding the lock.
//!
//! Feature-gated behind `ui`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use raylib::prelude::*;

use crate::engine::{Engine, MAX_PLUGINS, PLUGIN_LIST, Plugin, PluginTag};
use crate::input::{Event, EventType, Key};
use crate::interface::{HEIGHT, WIDTH, draw_text_centered};
use crate::midi::{self, frames_to_beats};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TimelineScreen {
    Overview,
    MidiEditor,
    Track,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackScreen {
    Overview,
    Plugin,
    PluginSelector,
}

#[derive(Clone, Copy)]
struct BeatFrame {
    center: f32,
    radius: f32,
}
impl BeatFrame {
    fn left(&self) -> f32 {
        self.center - self.radius
    }
    fn right(&self) -> f32 {
        self.center + self.radius
    }
    fn width(&self) -> f32 {
        self.radius * 2.0
    }
}

#[derive(Clone, Copy)]
struct BeatWindow {
    start: f32,
    len: f32,
}
impl BeatWindow {
    fn left(&self) -> f32 {
        self.start
    }
    fn right(&self) -> f32 {
        self.start + self.len
    }
}

pub struct Icons {
    map: HashMap<&'static str, Texture2D>,
}
impl Icons {
    /// Best-effort load; missing assets just render without an icon.
    pub fn load(rl: &mut RaylibHandle, thread: &RaylibThread) -> Self {
        let mut map = HashMap::new();
        if let Ok(t) = rl.load_texture(thread, "assets/water_spell.png") {
            map.insert("lpf", t);
        }
        if let Ok(t) = rl.load_texture(thread, "assets/slowed.png") {
            map.insert("delay", t);
        }
        Self { map }
    }
    fn get(&self, tag: PluginTag) -> Option<&Texture2D> {
        self.map.get(tag.name())
    }
}

pub struct App {
    engine: Arc<Mutex<Engine>>,
    icons: Icons,

    mode: Mode,
    note_offset: i16,
    active_notes: HashMap<Key, u8>,
    timeline: TimelineUi,
}

struct TimelineUi {
    screen: TimelineScreen,
    frame: BeatFrame,
    bar_width: f32,
    cursor: BeatWindow,
    step_size: f32,
    track: TrackUi,
}

struct TrackUi {
    screen: TrackScreen,
    active_plugin: usize,
    selector_index: usize,
}

const HEADER_HEIGHT: i32 = 12;
const ROW_HEIGHT: i32 = 28;

impl App {
    pub fn new(engine: Arc<Mutex<Engine>>, icons: Icons) -> Self {
        Self {
            engine,
            icons,
            mode: Mode::Normal,
            note_offset: 0,
            active_notes: HashMap::new(),
            timeline: TimelineUi::new(),
        }
    }

    // -------------------------------------------------------------- input

    pub fn handle_event(&mut self, ev: Event) {
        match self.mode {
            Mode::Insert => self.handle_insert(ev),
            Mode::Normal => {
                if ev.ty != EventType::KeyPress {
                    return;
                }
                if ev.key == Key::I {
                    self.mode = Mode::Insert;
                    return;
                }
                let mut eng = self.engine.lock().unwrap();
                self.timeline.handle_event(ev, &mut eng);
            }
        }
    }

    fn handle_insert(&mut self, ev: Event) {
        if let Some(base) = midi::key_to_midi(ev.key) {
            let raw = base as i16 + self.note_offset;
            if !(0..=127).contains(&raw) {
                return;
            }
            let note = raw as u8;
            let mut eng = self.engine.lock().unwrap();
            match ev.ty {
                EventType::KeyPress => {
                    self.active_notes.insert(ev.key, note);
                    eng.note_on(note);
                }
                EventType::KeyRelease => {
                    if let Some(held) = self.active_notes.remove(&ev.key) {
                        eng.note_off(held);
                    }
                }
            }
            return;
        }

        if ev.ty == EventType::KeyPress {
            match ev.key {
                Key::Escape => {
                    let mut eng = self.engine.lock().unwrap();
                    for (_, note) in self.active_notes.drain() {
                        eng.note_off(note);
                    }
                    self.mode = Mode::Normal;
                }
                Key::Z => self.note_offset = (self.note_offset - 12).max(-48),
                Key::X => self.note_offset = (self.note_offset + 12).min(48),
                _ => {}
            }
        }
    }

    // -------------------------------------------------------------- render

    pub fn render<D: RaylibDraw>(&mut self, d: &mut D) {
        let eng = self.engine.lock().unwrap();
        self.timeline.draw(d, &eng, &self.icons);

        if self.mode == Mode::Insert {
            d.draw_rectangle_lines(0, 0, WIDTH, HEIGHT, Color::PURPLE);
        }
        if eng.recording {
            d.draw_rectangle_lines(1, 1, WIDTH - 2, HEIGHT - 2, Color::RED);
        }
    }
}

impl TimelineUi {
    fn new() -> Self {
        Self {
            screen: TimelineScreen::Overview,
            frame: BeatFrame {
                center: 0.0,
                radius: 8.0,
            },
            bar_width: 4.0,
            cursor: BeatWindow {
                start: 0.0,
                len: 4.0,
            },
            step_size: 4.0,
            track: TrackUi::new(),
        }
    }

    fn cursor_focus(&mut self) {
        if self.cursor.left() < self.frame.left() {
            self.frame.center -= self.frame.left() - self.cursor.left();
        } else if self.cursor.right() > self.frame.right() {
            self.frame.center += self.cursor.right() - self.frame.right();
        }
    }

    fn handle_event(&mut self, ev: Event, eng: &mut Engine) {
        match self.screen {
            TimelineScreen::Overview => self.handle_overview(ev, eng),
            TimelineScreen::MidiEditor => {
                if ev.key == Key::Backspace {
                    self.screen = TimelineScreen::Overview;
                }
            }
            TimelineScreen::Track => {
                if self.track.handle_event(ev, eng) {
                    self.screen = TimelineScreen::Overview;
                }
            }
        }
    }

    fn handle_overview(&mut self, ev: Event, eng: &mut Engine) {
        match ev.key {
            Key::Enter => self.screen = TimelineScreen::Track,
            Key::E => self.screen = TimelineScreen::MidiEditor,
            Key::H => {
                self.cursor.start -= self.step_size;
                self.cursor_focus();
            }
            Key::L => {
                self.cursor.start += self.step_size;
                self.cursor_focus();
            }
            Key::J => {
                let at = eng.timeline.active_track;
                if at + 1 < eng.track_count() {
                    eng.set_active_track(at + 1);
                }
            }
            Key::K => {
                let at = eng.timeline.active_track;
                if at > 0 {
                    eng.set_active_track(at - 1);
                }
            }
            Key::Space => eng.toggle_play(),
            Key::Backspace => eng.reset(),
            Key::R => eng.toggle_record(),
            Key::Equal if eng.track_count() < crate::engine::MAX_TRACKS => {
                eng.add_track(crate::engine::Track::new(&[]));
            }
            Key::Minus if eng.track_count() > 1 => {
                let idx = eng.timeline.active_track;
                eng.remove_track(idx);
            }
            Key::RightBracket => self.frame.radius = (self.frame.radius / 2.0).max(2.0),
            Key::LeftBracket => self.frame.radius = (self.frame.radius * 2.0).min(128.0),
            _ => {}
        }
    }

    fn draw<D: RaylibDraw>(&mut self, d: &mut D, eng: &Engine, icons: &Icons) {
        match self.screen {
            TimelineScreen::Overview => self.draw_overview(d, eng),
            TimelineScreen::MidiEditor => {
                d.draw_text("MIDI_EDITOR", 30, 30, 10, Color::LIGHTGRAY);
            }
            TimelineScreen::Track => self.track.draw(d, eng, icons),
        }
    }

    fn draw_overview<D: RaylibDraw>(&mut self, d: &mut D, eng: &Engine) {
        d.draw_text("TIMELINE_OVERVIEW", 30, 30, 10, Color::LIGHTGRAY);
        let w = WIDTH as f32;
        let num_rows = eng.track_count().min(crate::engine::MAX_TRACKS);
        let b = frames_to_beats(eng.timeline.playhead, eng.bpm(), eng.sample_rate());

        if b > self.frame.right() || b < self.frame.left() {
            self.frame.center = b;
        }

        // beat position text "bar.beat"
        let bar = (b / self.bar_width).floor() as i32;
        let beat_in_bar = (b % self.bar_width) as i32;
        let label = format!("{}.{}", bar + 1, beat_in_bar + 1);
        draw_text_centered(d, &label, WIDTH / 2, HEADER_HEIGHT / 2, 8, Color::WHITE);

        // bar lines
        let mut bar_pos = (self.frame.left() / self.bar_width).floor() * self.bar_width;
        while bar_pos < self.frame.right() {
            let pct = (bar_pos - self.frame.left()) / self.frame.width();
            let x = (pct * w) as i32;
            d.draw_line(
                x,
                HEADER_HEIGHT,
                x,
                HEADER_HEIGHT + num_rows as i32 * ROW_HEIGHT,
                Color::DARKGRAY,
            );
            bar_pos += self.bar_width;
        }

        // rows + notes
        for i in 0..num_rows {
            let row_y = i as i32 * ROW_HEIGHT + HEADER_HEIGHT;
            d.draw_rectangle_lines(0, row_y, WIDTH, ROW_HEIGHT, Color::DARKGRAY);

            for note in &eng.track(i).notes {
                let sb = frames_to_beats(note.start, eng.bpm(), eng.sample_rate());
                let eb = frames_to_beats(note.end, eng.bpm(), eng.sample_rate());
                if eb < self.frame.left() || sb > self.frame.right() {
                    continue;
                }
                let lp = (sb - self.frame.left()) / self.frame.width();
                let rp = (eb - self.frame.left()) / self.frame.width();
                let x1 = (lp * w).max(0.0) as i32;
                let x2 = (rp * w).min(w) as i32;
                let slot = 23 - (note.note % 24) as i32; // invert: higher = higher
                let ny = row_y + 2 + slot;
                d.draw_line(x1, ny, x2.max(x1 + 1), ny, Color::GREEN);
            }
        }

        // active track highlight
        let row = eng.timeline.active_track as i32;
        let y = row * ROW_HEIGHT + HEADER_HEIGHT;
        d.draw_rectangle_lines(0, y, WIDTH, ROW_HEIGHT, Color::RED);

        // playhead
        if self.frame.left() < b && b < self.frame.right() {
            let pct = (b - self.frame.left()) / self.frame.width();
            let x = (pct * w) as i32;
            d.draw_line(x, HEADER_HEIGHT, x, HEIGHT, Color::WHITE);
        }

        // cursor
        let lp = (self.cursor.left() - self.frame.left()) / self.frame.width();
        let rp = (self.cursor.right() - self.frame.left()) / self.frame.width();
        let lx = (lp * w) as i32;
        let rx = (rp * w) as i32;
        d.draw_rectangle_lines(lx, y, rx - lx, ROW_HEIGHT, Color::ORANGE);
    }
}

impl TrackUi {
    fn new() -> Self {
        Self {
            screen: TrackScreen::Overview,
            active_plugin: 0,
            selector_index: 0,
        }
    }

    fn handle_event(&mut self, ev: Event, eng: &mut Engine) -> bool {
        match self.screen {
            TrackScreen::Overview => self.handle_overview(ev, eng),
            TrackScreen::Plugin => {
                self.handle_plugin(ev, eng);
                false
            }
            TrackScreen::PluginSelector => {
                self.handle_plugin_selector(ev, eng);
                false
            }
        }
    }

    fn handle_overview(&mut self, ev: Event, eng: &Engine) -> bool {
        let plugin_count = eng.active().plugins.len();
        match ev.key {
            Key::Backspace => return true,
            Key::A => self.screen = TrackScreen::PluginSelector,
            Key::Enter if plugin_count > 0 => self.screen = TrackScreen::Plugin,
            Key::L if plugin_count > 0 => {
                self.active_plugin = (self.active_plugin + 1).min(plugin_count - 1);
            }
            Key::H if self.active_plugin > 0 => self.active_plugin -= 1,
            _ => {}
        }
        false
    }

    fn handle_plugin(&mut self, ev: Event, eng: &mut Engine) {
        if ev.key == Key::Backspace {
            self.screen = TrackScreen::Overview;
            return;
        }

        let plugin = match eng.active_mut().plugins.get_mut(self.active_plugin) {
            Some(plugin) => plugin,
            None => return,
        };
        macro_rules! nudge {
            ($param:expr, $delta:expr) => {{
                let norm = $param.get_norm();
                $param.set_norm(norm + $delta);
            }};
        }
        match plugin {
            Plugin::Lpf(p) => match ev.key {
                Key::One => nudge!(p.drive, -0.1),
                Key::Two => nudge!(p.drive, 0.1),
                Key::Three => nudge!(p.resonance, -0.1),
                Key::Four => nudge!(p.resonance, 0.1),
                Key::Five => nudge!(p.cutoff, -0.1),
                Key::Six => nudge!(p.cutoff, 0.1),
                _ => {}
            },
            Plugin::Delay(p) => match ev.key {
                Key::One => nudge!(p.delay_time, -0.1),
                Key::Two => nudge!(p.delay_time, 0.1),
                Key::Three => nudge!(p.feedback, -0.1),
                Key::Four => nudge!(p.feedback, 0.1),
                Key::Five => nudge!(p.mix, -0.1),
                Key::Six => nudge!(p.mix, 0.1),
                _ => {}
            },
        }
    }

    fn handle_plugin_selector(&mut self, ev: Event, eng: &mut Engine) {
        match ev.key {
            Key::Backspace => self.screen = TrackScreen::Overview,
            Key::K if self.selector_index > 0 => self.selector_index -= 1,
            Key::J if self.selector_index + 1 < PLUGIN_LIST.len() => self.selector_index += 1,
            Key::Enter => {
                eng.add_plugin(
                    eng.timeline.active_track,
                    Plugin::new(PLUGIN_LIST[self.selector_index]),
                );
                self.screen = TrackScreen::Overview;
            }
            _ => {}
        }
    }

    fn draw<D: RaylibDraw>(&self, d: &mut D, eng: &Engine, icons: &Icons) {
        match self.screen {
            TrackScreen::Overview => self.draw_overview(d, eng, icons),
            TrackScreen::Plugin => self.draw_plugin(d, eng),
            TrackScreen::PluginSelector => self.draw_plugin_selector(d),
        }
    }

    fn draw_overview<D: RaylibDraw>(&self, d: &mut D, eng: &Engine, icons: &Icons) {
        let at = eng.timeline.active_track;
        let track = eng.track(at);

        d.draw_rectangle(0, 0, 32, 32, Color::RED);
        draw_text_centered(d, &at.to_string(), 16, 16, 8, Color::LIGHTGRAY);

        // grid (bottom half)
        for i in 0..5 {
            d.draw_line(i * 32, 64, i * 32, 128, Color::WHITE);
        }
        for i in 0..3 {
            d.draw_line(0, 64 + i * 32, 128, 64 + i * 32, Color::WHITE);
        }

        for i in 0..track.plugins.len() {
            let x = (i as i32 % 4) * 32 + 16;
            let y = (i as i32 / 4) * 32 + 64 + 16;
            let tag = track.plugins[i].tag();
            if let Some(tex) = icons.get(tag) {
                d.draw_texture(tex, x - 8, y - 8, Color::WHITE);
            }
            let name = tag.name();
            let tw = crate::interface::measure_text(name, 10);
            d.draw_text(name, x - tw / 2, y + 6, 10, Color::WHITE);
            if i == self.active_plugin {
                d.draw_circle(x, y, 5.0, Color::GREEN);
            }
        }
        for i in track.plugins.len()..MAX_PLUGINS {
            let x = (i as i32 % 4) * 32 + 16;
            let y = (i as i32 / 4) * 32 + 64 + 16;
            d.draw_circle(x, y, 1.0, Color::RED);
            d.draw_text("none", x - 11, y + 4, 10, Color::WHITE);
        }

        d.draw_text("TRACK", 30, 30, 10, Color::LIGHTGRAY);
    }

    fn draw_plugin<D: RaylibDraw>(&self, d: &mut D, eng: &Engine) {
        let at = eng.timeline.active_track;
        let track = eng.track(at);
        if self.active_plugin >= track.plugins.len() {
            return;
        }
        let plugin = &track.plugins[self.active_plugin];
        let knobs = plugin.knobs();
        let positions = [(32.0f32, 32.0f32), (96.0, 32.0), (32.0, 96.0)];
        for ((norm, name), (px, py)) in knobs.iter().zip(positions.iter()) {
            let angle = norm * 360.0;
            d.draw_circle_sector(Vector2::new(*px, *py), 10.0, 0.0, angle, 360, Color::WHITE);
            draw_text_centered(d, name, *px as i32, (*py + 20.0) as i32, 10, Color::WHITE);
        }
        let (label, color) = match plugin.tag() {
            PluginTag::Lpf => ("LPF", Color::GREEN),
            PluginTag::Delay => ("DELAY", Color::PURPLE),
        };
        draw_text_centered(d, label, 64, 64, 10, color);
    }

    fn draw_plugin_selector<D: RaylibDraw>(&self, d: &mut D) {
        for (i, tag) in PLUGIN_LIST.iter().enumerate() {
            let y = i as i32 * 16;
            let color = if i == self.selector_index {
                Color::BLUE
            } else {
                Color::RED
            };
            d.draw_rectangle(0, y, 128, 16, Color::DARKGRAY);
            d.draw_text(tag.name(), 0, y, 5, color);
        }
    }
}
