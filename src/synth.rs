//! Port of `synth.zig`.
//!
//! Zig's `Voice` wired osc -> lpf -> adsr via stored input `Node`s (raw sibling pointers).
//! Here the chain is concrete in-place processing: the osc fills a scratch buffer, then the
//! lpf and adsr transform it in place. No vtable, no boxing, no self-reference.

use crate::audio::{Adsr, AdsrStage, Context, Lpf, Osc, OscKind, Sample};

const SYNTH_TUNING: f32 = 440.0;
const NUM_VOICES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum NoteState {
    Off,
    On(u8),
}

struct Voice {
    osc: Osc,
    lpf: Lpf,
    adsr: Adsr,
    note_state: NoteState,
}

impl Voice {
    fn new(freq: f32) -> Self {
        Self {
            osc: Osc::new(freq, OscKind::Saw),
            lpf: Lpf::new(),
            adsr: Adsr::new(),
            note_state: NoteState::Off,
        }
    }

    /// osc (source) -> lpf (in place) -> adsr (in place). Writes into `out`.
    fn process(&mut self, ctx: &Context, out: &mut [Sample]) {
        self.osc.render(ctx, out);
        self.lpf.render(ctx, out);
        self.adsr.render(ctx, out);
    }

    fn set_note_on(&mut self, note: u8) {
        self.note_state = NoteState::On(note);
        self.osc.reset_phase();
        self.osc.freq = note_to_freq(note);
        self.adsr.note_on();
    }

    fn set_note_off(&mut self, note: u8) {
        if let NoteState::On(on) = self.note_state {
            if on == note {
                self.note_state = NoteState::Off;
                self.adsr.note_off();
            }
        }
    }
}

pub struct Uni {
    pub cutoff: f32,
    voices: Vec<Voice>,
    next_idx: usize,
}

impl Uni {
    pub fn new() -> Self {
        Self {
            cutoff: 5000.0,
            voices: (0..NUM_VOICES).map(|_| Voice::new(0.0)).collect(),
            next_idx: 0,
        }
    }

    fn find_free_voice(&mut self) -> Option<&mut Voice> {
        self.voices
            .iter_mut()
            .find(|v| v.note_state == NoteState::Off)
    }

    pub fn note_on(&mut self, note: u8) {
        if let Some(v) = self.find_free_voice() {
            v.set_note_on(note);
        } else {
            let idx = self.next_idx;
            self.next_idx = (self.next_idx + 1) % self.voices.len();
            self.voices[idx].set_note_on(note);
        }
    }

    pub fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            v.set_note_off(note);
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            if let NoteState::On(note) = v.note_state {
                v.set_note_off(note);
            }
        }
    }

    /// Source: sums all voices into `out`.
    pub fn render(&mut self, ctx: &Context, out: &mut [Sample]) {
        out.fill(0.0);
        let cutoff = self.cutoff;
        for v in &mut self.voices {
            v.lpf.cutoff.set(cutoff);
            let tmp = ctx.tmp(out.len());
            v.process(ctx, tmp);
            for (o, t) in out.iter_mut().zip(tmp.iter()) {
                *o += *t;
            }
        }
    }

    /// True if every voice has fully released (used by tests/diagnostics).
    pub fn is_idle(&self) -> bool {
        self.voices.iter().all(|v| v.adsr.stage == AdsrStage::Idle)
    }
}

impl Default for Uni {
    fn default() -> Self {
        Self::new()
    }
}

fn note_to_freq(note: u8) -> f32 {
    let semitone_offset = (note as i16 - 69) as f32;
    SYNTH_TUNING * (semitone_offset / 12.0).exp2()
}
