//! The audio engine owns the timeline, tracks, synths, DSP plugins, and transport state.
//! It lives behind `Arc<Mutex<Engine>>`; the UI locks it to apply input and to read state
//! for rendering, and the cpal callback locks it to fill each audio block.
//!
use crate::audio::{AudioBuffer, Context, Sample};
use crate::instrument::Instrument;
use crate::midi::{Frame, Note};

pub use crate::plugin::{LIST as PLUGIN_LIST, Plugin, PluginKind, PluginUi, create};

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
    pub source: TrackSource,
    pub plugins: Vec<Plugin>,
    buffer: Vec<Sample>,
}

pub enum TrackSource {
    Instrument {
        instrument: Instrument,
        notes: Vec<Note>,
    },
    Audio {
        clips: Vec<AudioClip>,
    },
}

pub struct AudioClip {
    pub(crate) start: Frame,
    pub(crate) audio: AudioBuffer,
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
        self.timeline.tracks[at].note_on(note);
        if self.recording && self.playing {
            self.held_notes[note as usize] = Some(self.timeline.playhead);
        }
    }

    pub fn note_off(&mut self, note: u8) {
        let at = self.timeline.active_track;
        self.timeline.tracks[at].note_off(note);
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
            t.all_notes_off();
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
                if let TrackSource::Instrument { notes, .. } = &mut self.timeline.tracks[at].source
                {
                    notes.extend(buf);
                }
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

    /// Fill `out` (mono) with one block. Asserts finiteness in debug builds.
    /// This is block-accurate midi, not sample accurate.
    pub fn process_block(&mut self, out: &mut [Sample]) {
        let start = self.timeline.playhead;
        let end = start + out.len() as u64;
        out.fill(0.0);

        for t in &mut self.timeline.tracks {
            if self.playing
                && let TrackSource::Instrument { instrument, notes } = &mut t.source
            {
                for note in notes {
                    if start <= note.start && note.start < end {
                        instrument.note_on(note.note);
                    }
                    if start <= note.end && note.end < end {
                        instrument.note_off(note.note);
                    }
                }
            }
            t.process(&self.ctx, start, self.playing, out);
        }

        if self.playing {
            self.timeline.playhead = end;
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
    pub fn new(source: TrackSource) -> Self {
        let mut plugins = Vec::new();
        plugins.reserve_exact(MAX_PLUGINS); // never reallocs while audio holds the lock
        Self {
            source,
            plugins,
            buffer: Vec::new(),
        }
    }

    pub fn note_on(&mut self, note: u8) {
        if let TrackSource::Instrument { instrument, .. } = &mut self.source {
            instrument.note_on(note);
        }
    }
    pub fn note_off(&mut self, note: u8) {
        if let TrackSource::Instrument { instrument, .. } = &mut self.source {
            instrument.note_off(note);
        }
    }
    pub fn all_notes_off(&mut self) {
        if let TrackSource::Instrument { instrument, .. } = &mut self.source {
            instrument.all_notes_off();
        }
    }

    /// Process this track into its owned buffer, then mix it into `out`.
    pub fn process(&mut self, ctx: &Context, playhead: Frame, playing: bool, out: &mut [Sample]) {
        // resize is not a big problem since expected # of allocs is constant
        if self.buffer.len() < out.len() {
            self.buffer.resize(out.len(), 0.0);
        }
        let buffer = &mut self.buffer[..out.len()];
        buffer.fill(0.0);

        match &mut self.source {
            TrackSource::Instrument { instrument, .. } => {
                instrument.process(ctx, buffer);
            }
            TrackSource::Audio { clips } if playing => {
                let block_start = playhead;

                for clip in clips {
                    let ratio = clip.audio.sample_rate / ctx.sample_rate;
                    for (i, sample) in buffer.iter_mut().enumerate() {
                        let frame = block_start + i as Frame;
                        if frame < clip.start {
                            continue;
                        }

                        let src_pos = (frame - clip.start) as f32 * ratio;
                        let src_idx = src_pos as usize;

                        if src_idx >= clip.audio.samples.len() {
                            break;
                        }

                        *sample += clip.audio.samples[src_idx];
                    }
                }
            }
            TrackSource::Audio { .. } => {}
        }

        for p in &mut self.plugins {
            p.process(ctx, buffer);
        }
        for (out, sample) in out.iter_mut().zip(buffer) {
            *out += *sample;
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

impl AudioClip {
    pub fn new(start: Frame, audio: AudioBuffer) -> Self {
        Self { start, audio }
    }
}
