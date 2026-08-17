use raylib::prelude::*;

use crate::audio::{Adsr, AdsrStage, Context, Lpf, Osc, OscKind, Sample};
use crate::input::{Event, Key};
use crate::ui::Action;

pub struct Sampler {}
pub struct SamplerUi;

impl Sampler {
    pub fn note_on(&mut self, note: u8) {}

    pub fn note_off(&mut self, note: u8) {}

    pub fn all_notes_off(&mut self) {}

    pub fn process(&mut self, ctx: &Context, out: &mut [Sample]) {}
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
