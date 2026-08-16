//! The audio engine: a `Send` graph that owns everything the audio thread
//! touches — timeline, tracks, synths, DSP plugins, and playback/record state.
//! It lives behind `Arc<Mutex<Engine>>`; the UI locks it to apply input and to read state
//! for rendering, and the cpal callback locks it to fill each audio block.
//!
//! This folds together `project.zig` (Timeline/Track) and the audio-thread logic from
//! `main.zig` (note/op handling, playhead advance, recording). The Zig version used a
//! lock-free op queue so the audio thread was the sole mutator; under the mutex model that
//! indirection is unnecessary — the UI mutates the engine directly while holding the lock.
//! (The tested `queue::SpscQueue` is still available if you want the lock-free split back.)

use crate::audio::{Context, Sample};
use crate::midi::{self, Frame, Note, NoteMsg};
use crate::synth::Uni;

pub use crate::plugin::{LIST as PLUGIN_LIST, Plugin, PluginUi, Tag as PluginTag};

pub const MAX_TRACKS: usize = 8;
pub const MAX_PLUGINS: usize = 8;

pub struct Engine {
    ctx: Context,
    pub timeline: Timeline,
    pub playing: bool,
    pub recording: bool,

    held_notes: [Option<Frame>; 128],
    record_buffer: Vec<Note>,
}

pub struct Timeline {
    tracks: Vec<Track>,
    pub active_track: usize,
    pub playhead: u64,
}

pub struct Track {
    pub synth: Uni,
    pub notes: Vec<Note>,
    pub plugins: Vec<Plugin>,
}

impl Engine {
    pub fn new(sample_rate: f32, bpm: f32) -> Self {
        Self {
            ctx: Context::new(sample_rate, bpm),
            timeline: Timeline::new(),
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
        self.timeline.tracks.len()
    }
    pub fn tracks(&self) -> &[Track] {
        &self.timeline.tracks
    }
    pub fn track(&self, i: usize) -> &Track {
        &self.timeline.tracks[i]
    }
    pub fn track_mut(&mut self, i: usize) -> &mut Track {
        &mut self.timeline.tracks[i]
    }
    pub fn add_track(&mut self, track: Track) -> bool {
        if self.timeline.tracks.len() < MAX_TRACKS {
            self.timeline.tracks.push(track);
            true
        } else {
            false
        }
    }

    pub fn remove_track(&mut self, idx: usize) -> bool {
        if idx < self.timeline.tracks.len() && self.timeline.tracks.len() > 1 {
            self.timeline.tracks.remove(idx);
            if self.timeline.active_track >= self.timeline.tracks.len() {
                self.timeline.active_track = self.timeline.tracks.len() - 1;
            }
            true
        } else {
            false
        }
    }

    pub fn set_active_track(&mut self, idx: usize) {
        let n = self.timeline.tracks.len();
        self.timeline.active_track = if n > 0 { idx.min(n - 1) } else { 0 };
    }

    pub fn add_plugin(&mut self, track_idx: usize, plugin: Plugin) -> bool {
        self.timeline
            .tracks
            .get_mut(track_idx)
            .is_some_and(|track| track.add_plugin(plugin))
    }

    pub fn remove_plugin(&mut self, track_idx: usize, plugin_idx: usize) -> bool {
        self.timeline
            .tracks
            .get_mut(track_idx)
            .is_some_and(|track| track.remove_plugin(plugin_idx))
    }

    pub fn note_on(&mut self, note: u8) {
        let at = self.timeline.active_track;
        self.timeline.tracks[at].synth.note_on(note);
        if self.recording && self.playing {
            self.held_notes[note as usize] = Some(self.timeline.playhead);
        }
    }

    pub fn note_off(&mut self, note: u8) {
        let at = self.timeline.active_track;
        self.timeline.tracks[at].synth.note_off(note);
        if self.recording
            && self.playing
            && let Some(start) = self.held_notes[note as usize]
        {
            self.record_buffer.push(Note {
                start,
                end: self.timeline.playhead,
                note,
            });
            self.held_notes[note as usize] = None;
        }
    }

    fn all_notes_off(&mut self) {
        for t in &mut self.timeline.tracks {
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
        self.timeline.playhead = 0;
    }

    pub fn set_playhead(&mut self, frame: u64) {
        self.timeline.playhead = frame;
    }

    pub fn toggle_record(&mut self) {
        if self.recording {
            if !self.record_buffer.is_empty() {
                let at = self.timeline.active_track;
                let buf = std::mem::take(&mut self.record_buffer);
                self.timeline.tracks[at].notes.extend_from_slice(&buf);
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
        for t in &mut self.timeline.tracks {
            let tmp = self.ctx.tmp(n);
            t.process(&self.ctx, tmp);
            for (o, s) in out.iter_mut().zip(tmp.iter()) {
                *o += *s;
            }
        }

        debug_assert!(out.iter().all(|s| s.is_finite()), "NaN/inf in audio block");

        // advance playhead + fire sequenced notes for the span we just rendered
        if self.playing {
            let start = self.timeline.playhead;
            self.timeline.playhead += n as u64;
            let end = self.timeline.playhead;

            let mut msgs = [NoteMsg::On(0); midi::MAX_NOTES_PER_BLOCK];
            for t in &mut self.timeline.tracks {
                let mut count = 0;
                for note in &t.notes {
                    if start <= note.start && note.start < end && count < msgs.len() {
                        msgs[count] = NoteMsg::On(note.note);
                        count += 1;
                    }
                    if start <= note.end && note.end < end && count < msgs.len() {
                        msgs[count] = NoteMsg::Off(note.note);
                        count += 1;
                    }
                }
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

impl Timeline {
    fn new() -> Self {
        let mut tracks = Vec::new();
        tracks.reserve_exact(MAX_TRACKS);
        Self {
            tracks,
            active_track: 0,
            playhead: 0,
        }
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
}

impl Track {
    pub fn new(notes: &[Note]) -> Self {
        let mut plugins = Vec::new();
        plugins.reserve_exact(MAX_PLUGINS); // never reallocs while audio holds the lock
        Self {
            synth: Uni::new(),
            notes: notes.to_vec(),
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

    pub fn add_plugin(&mut self, p: Plugin) -> bool {
        if self.plugins.len() < MAX_PLUGINS {
            self.plugins.push(p);
            true
        } else {
            false
        }
    }

    pub fn remove_plugin(&mut self, idx: usize) -> bool {
        if idx < self.plugins.len() {
            self.plugins.remove(idx);
            true
        } else {
            false
        }
    }
}
