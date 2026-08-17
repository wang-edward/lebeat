//! Polyphonic synth with an owned `Osc -> Lpf -> Adsr` chain per voice.

pub mod juno;

use crate::audio::Context;
use juno::Juno;

pub enum Instrument {
    Juno(Juno),
}

impl Instrument {
    pub fn process(&mut self, ctx: &Context, out: &mut [f32]) {
        match self {
            Instrument::Juno(x) => x.process(ctx, out),
        }
    }

    pub fn note_on(&mut self, note: u8) {
        match self {
            Instrument::Juno(x) => x.note_on(note),
        }
    }

    pub fn note_off(&mut self, note: u8) {
        match self {
            Instrument::Juno(x) => x.note_off(note),
        }
    }

    pub fn all_notes_off(&mut self) {
        match self {
            Instrument::Juno(x) => x.all_notes_off(),
        }
    }
}
