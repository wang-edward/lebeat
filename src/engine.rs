//! The audio engine: a raylib-free, `Send` graph that owns everything the audio thread
//! touches — timeline, tracks, synths, DSP plugins, players, and playback/record state.
//! It lives behind `Arc<Mutex<Engine>>`; the UI locks it to apply input and to read state
//! for rendering, and the cpal callback locks it to fill each audio block.
//!
//! This folds together `project.zig` (Timeline/Track) and the audio-thread logic from
//! `main.zig` (note/op handling, playhead advance, recording). The Zig version used a
//! lock-free op queue so the audio thread was the sole mutator; under the mutex model that
//! indirection is unnecessary — the UI mutates the engine directly while holding the lock.
//! (The tested `queue::SpscQueue` is still available if you want the lock-free split back.)

use crate::audio::{Context, Delay, Lpf, Sample};
use crate::midi::{self, Frame, Note, NoteMsg, Player};
use crate::synth::Uni;

pub const MAX_TRACKS: usize = 8;
pub const MAX_PLUGINS: usize = 8;

// ------------------------------------------------------------------ Plugin (DSP only)

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PluginTag {
    Lpf,
    Delay,
}

pub const PLUGIN_LIST: [PluginTag; 2] = [PluginTag::Lpf, PluginTag::Delay];

impl PluginTag {
    pub fn name(self) -> &'static str {
        match self {
            PluginTag::Lpf => "lpf",
            PluginTag::Delay => "delay",
        }
    }
}

/// Which knob a key tweaks, and the result of a plugin key event.
pub enum PluginAction {
    None,
    GoBack,
}

pub enum Plugin {
    Lpf(Lpf),
    Delay(Delay),
}

impl Plugin {
    pub fn new(tag: PluginTag) -> Self {
        match tag {
            PluginTag::Lpf => Plugin::Lpf(Lpf::new()),
            PluginTag::Delay => Plugin::Delay(Delay::new(48_000)),
        }
    }

    pub fn tag(&self) -> PluginTag {
        match self {
            Plugin::Lpf(_) => PluginTag::Lpf,
            Plugin::Delay(_) => PluginTag::Delay,
        }
    }

    /// In-place transform of the track buffer.
    pub fn process(&mut self, ctx: &Context, buf: &mut [Sample]) {
        match self {
            Plugin::Lpf(p) => p.render(ctx, buf),
            Plugin::Delay(p) => p.render(ctx, buf),
        }
    }

    /// The three (param, label) pairs the UI draws as knobs, in normalized [0,1].
    pub fn knobs(&self) -> [(f32, &'static str); 3] {
        match self {
            Plugin::Lpf(p) => [
                (p.drive.get_norm(), "drive"),
                (p.resonance.get_norm(), "resonance"),
                (p.cutoff.get_norm(), "cutoff"),
            ],
            Plugin::Delay(p) => [
                (p.delay_time.get_norm(), "delay_time"),
                (p.feedback.get_norm(), "feedback"),
                (p.mix.get_norm(), "mix"),
            ],
        }
    }

    /// Keys 1..6 nudge the three knobs down/up by 0.1; backspace goes back.
    /// (Mirrors `plugin.zig`'s handle_event, raylib-free.)
    pub fn handle_key(&mut self, key: crate::input::Key) -> PluginAction {
        use crate::input::Key::*;
        macro_rules! nudge {
            ($p:expr, $d:expr) => {{
                let n = $p.get_norm();
                $p.set_norm(n + $d);
            }};
        }
        match self {
            Plugin::Lpf(p) => match key {
                Backspace => return PluginAction::GoBack,
                One => nudge!(p.drive, -0.1),
                Two => nudge!(p.drive, 0.1),
                Three => nudge!(p.resonance, -0.1),
                Four => nudge!(p.resonance, 0.1),
                Five => nudge!(p.cutoff, -0.1),
                Six => nudge!(p.cutoff, 0.1),
                _ => {}
            },
            Plugin::Delay(p) => match key {
                Backspace => return PluginAction::GoBack,
                One => nudge!(p.delay_time, -0.1),
                Two => nudge!(p.delay_time, 0.1),
                Three => nudge!(p.feedback, -0.1),
                Four => nudge!(p.feedback, 0.1),
                Five => nudge!(p.mix, -0.1),
                Six => nudge!(p.mix, 0.1),
                _ => {}
            },
        }
        PluginAction::None
    }
}

// ------------------------------------------------------------------ Track

pub struct Track {
    pub synth: Uni,
    pub player: Player,
    pub notes: Vec<Note>,
    pub plugins: Vec<Plugin>,
}

impl Track {
    pub fn new(notes: &[Note]) -> Self {
        let mut plugins = Vec::new();
        plugins.reserve_exact(MAX_PLUGINS); // never reallocs while audio holds the lock
        Self {
            synth: Uni::new(),
            player: Player::new(notes),
            plugins,
        }
    }

    /// synth (source) then each plugin (in place).
    pub fn process(&mut self, ctx: &Context, out: &mut [Sample]) {
        self.synth.render(ctx, out);
        for p in &mut self.plugins {
            p.process(ctx, out);
        }
    }

    pub fn add_plugin(&mut self, p: Plugin) {
        if self.plugins.len() < MAX_PLUGINS {
            self.plugins.push(p);
        }
    }

    pub fn remove_plugin(&mut self, idx: usize) {
        if idx < self.plugins.len() {
            self.plugins.remove(idx);
        }
    }
}

// ------------------------------------------------------------------ Engine

pub struct Engine {
    ctx: Context,
    tracks: Vec<Track>,
    pub active_track: usize,
    pub playhead: u64,
    pub playing: bool,
    pub recording: bool,

    held_notes: [Option<Frame>; 128],
    record_buffer: Vec<Note>,
}

impl Engine {
    pub fn new(sample_rate: f32, bpm: f32) -> Self {
        let mut tracks = Vec::new();
        tracks.reserve_exact(MAX_TRACKS);
        Self {
            ctx: Context::new(sample_rate, bpm),
            tracks,
            active_track: 0,
            playhead: 0,
            playing: false,
            recording: false,
            held_notes: [None; 128],
            record_buffer: Vec::new(),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.ctx.sample_rate
    }
    pub fn bpm(&self) -> f32 {
        self.ctx.bpm
    }
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
    pub fn track(&self, i: usize) -> &Track {
        &self.tracks[i]
    }
    pub fn track_mut(&mut self, i: usize) -> &mut Track {
        &mut self.tracks[i]
    }
    pub fn active(&self) -> &Track {
        &self.tracks[self.active_track]
    }
    pub fn active_mut(&mut self) -> &mut Track {
        &mut self.tracks[self.active_track]
    }

    pub fn add_track(&mut self, track: Track) {
        if self.tracks.len() < MAX_TRACKS {
            self.tracks.push(track);
        }
    }

    pub fn remove_track(&mut self, idx: usize) {
        if idx < self.tracks.len() && self.tracks.len() > 1 {
            self.tracks.remove(idx);
            if self.active_track >= self.tracks.len() {
                self.active_track = self.tracks.len() - 1;
            }
        }
    }

    pub fn set_active_track(&mut self, idx: usize) {
        let n = self.tracks.len();
        self.active_track = if n > 0 { idx.min(n - 1) } else { 0 };
    }

    pub fn add_plugin(&mut self, track_idx: usize, plugin: Plugin) {
        if track_idx < self.tracks.len() {
            self.tracks[track_idx].add_plugin(plugin);
        }
    }

    // --- live keyboard notes (UI calls these under the lock) ---

    pub fn note_on(&mut self, note: u8) {
        let at = self.active_track;
        self.tracks[at].synth.note_on(note);
        if self.recording && self.playing {
            self.held_notes[note as usize] = Some(self.playhead);
        }
    }

    pub fn note_off(&mut self, note: u8) {
        let at = self.active_track;
        self.tracks[at].synth.note_off(note);
        if self.recording && self.playing {
            if let Some(start) = self.held_notes[note as usize] {
                self.record_buffer.push(Note {
                    start,
                    end: self.playhead,
                    note,
                });
                self.held_notes[note as usize] = None;
            }
        }
    }

    fn all_notes_off(&mut self) {
        for t in &mut self.tracks {
            t.synth.all_notes_off();
        }
    }

    // --- transport (ported from main.zig op handlers) ---

    pub fn toggle_play(&mut self) {
        self.all_notes_off();
        if self.recording && self.playing {
            self.recording = false;
            self.playing = false;
            self.held_notes = [None; 128];
            self.record_buffer.clear();
        } else {
            self.playing = !self.playing;
        }
    }

    pub fn reset(&mut self) {
        self.all_notes_off();
        self.playhead = 0;
    }

    pub fn set_playhead(&mut self, frame: u64) {
        self.playhead = frame;
    }

    pub fn toggle_record(&mut self) {
        if self.recording {
            if !self.record_buffer.is_empty() {
                let at = self.active_track;
                let buf = std::mem::take(&mut self.record_buffer);
                self.tracks[at].player.append_notes(&buf);
            }
            self.held_notes = [None; 128];
            self.recording = false;
            self.all_notes_off();
            self.playing = false;
        } else if !self.playing {
            self.playing = true;
            self.recording = true;
        } else {
            self.recording = true;
        }
    }

    // --- audio block (cpal callback calls this under the lock) ---

    /// Fill `out` (mono) with one block. Asserts finiteness in debug builds.
    pub fn process_block(&mut self, out: &mut [Sample]) {
        self.ctx.begin_block();

        // mix all tracks
        let n = out.len();
        out.fill(0.0);
        for t in &mut self.tracks {
            let tmp = self.ctx.tmp(n);
            t.process(&self.ctx, tmp);
            for (o, s) in out.iter_mut().zip(tmp.iter()) {
                *o += *s;
            }
        }

        debug_assert!(out.iter().all(|s| s.is_finite()), "NaN/inf in audio block");

        // advance playhead + fire sequenced notes for the span we just rendered
        if self.playing {
            let start = self.playhead;
            self.playhead += n as u64;
            let end = self.playhead;

            let mut msgs = [NoteMsg::On(0); midi::MAX_NOTES_PER_BLOCK];
            for t in &mut self.tracks {
                let count = t.player.advance(start, end, &mut msgs);
                for m in &msgs[..count] {
                    match *m {
                        NoteMsg::On(note) => t.synth.note_on(note),
                        NoteMsg::Off(note) => t.synth.note_off(note),
                    }
                }
            }
        }
    }
}
