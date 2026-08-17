//! Polyphonic synth with an owned `Osc -> Lpf -> Adsr` chain per voice.

pub mod juno;
pub mod sampler;

use raylib::prelude::*;

use crate::audio::{Context, Sample};
use crate::input::Event;
use crate::ui::Action;
use juno::{Juno, JunoUi};
use sampler::{Sampler, SamplerUi};

pub enum Instrument {
    Juno(Juno),
    Sampler(Sampler),
}

impl Instrument {
    pub fn process(&mut self, ctx: &Context, out: &mut [Sample]) {
        match self {
            Instrument::Juno(x) => x.process(ctx, out),
            Instrument::Sampler(x) => x.process(ctx, out),
        }
    }

    pub fn note_on(&mut self, note: u8) {
        match self {
            Instrument::Juno(x) => x.note_on(note),
            Instrument::Sampler(x) => x.note_on(note),
        }
    }

    pub fn note_off(&mut self, note: u8) {
        match self {
            Instrument::Juno(x) => x.note_off(note),
            Instrument::Sampler(x) => x.note_off(note),
        }
    }

    pub fn all_notes_off(&mut self) {
        match self {
            Instrument::Juno(x) => x.all_notes_off(),
            Instrument::Sampler(x) => x.all_notes_off(),
        }
    }
}

pub enum InstrumentUi {
    Juno(JunoUi),
    Sampler(SamplerUi),
}

impl InstrumentUi {
    pub fn new(instrument: &Instrument) -> Self {
        match instrument {
            Instrument::Juno(_) => Self::Juno(JunoUi::new()),
            Instrument::Sampler(_) => Self::Juno(JunoUi::new()),
        }
    }

    pub fn matches(&self, instrument: &Instrument) -> bool {
        matches!((self, instrument), (Self::Juno(_), Instrument::Juno(_)))
    }

    pub fn handle_event(&mut self, instrument: &mut Instrument, event: Event) -> Action {
        match (self, instrument) {
            (Self::Juno(ui), Instrument::Juno(instrument)) => ui.handle_event(instrument, event),
            (Self::Sampler(ui), Instrument::Sampler(instrument)) => {
                ui.handle_event(instrument, event)
            }
        }
    }

    pub fn render<D: RaylibDraw>(&self, instrument: &Instrument, d: &mut D) {
        match (self, instrument) {
            (Self::Juno(ui), Instrument::Juno(instrument)) => ui.render(instrument, d),
            (Self::Sampler(ui), Instrument::Sampler(instrument)) => ui.render(instrument, d),
        }
    }
}
