use raylib::prelude::*;

use crate::audio::{Adsr, AdsrStage, Context, Lpf, Osc, OscKind, Sample};
use crate::input::{Event, Key};
use crate::ui::Action;

const NUM_VOICES: usize = 8;

pub struct Sampler {
    sample: Vec<Sample>,
    sample_rate: f32,
    root_note: u8,
    voices: Vec<SamplerVoice>,
}
struct SamplerVoice {
    note: Option<u8>,
    position: f32,
}
pub struct SamplerUi;

impl Sampler {
    pub fn new(sample: Vec<Sample>) -> Self {
        Sampler {
            sample: sample,
            sample_rate: 48_000f32, // TODO
            root_note: 60,          // C3
            voices: (0..NUM_VOICES).map(|_| SamplerVoice::new()).collect(),
        }
    }

    pub fn note_on(&mut self, note: u8) {
        let Some(voice) = self.voices.iter_mut().find(|v| v.note == None) else {
            return;
        };
        voice.note = Some(note);
        voice.position = 0.0;
    }
    pub fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == Some(note) {
                v.note = None;
            }
        }
    }
    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.note = None;
        }
    }
    pub fn process(&mut self, ctx: &Context, out: &mut [Sample]) {}
}

impl SamplerVoice {
    pub fn new() -> Self {
        SamplerVoice {
            note: None,
            position: 0f32,
        }
    }
}

impl SamplerUi {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_event(&mut self, sampler: &mut Sampler, event: Event) -> Action {
        match event.key {
            Key::Backspace => Action::GoBack,
            _ => Action::None,
        }
    }

    pub fn render<D: RaylibDraw>(&self, sampler: &Sampler, d: &mut D) {}
}
