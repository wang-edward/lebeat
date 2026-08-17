use raylib::prelude::*;

use crate::audio::{Context, Sample};
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
    pub fn new() -> Self {
        let (sample, sample_rate) = decode_wav(include_bytes!("../../assets/perfect.wav"));

        Sampler {
            sample,
            sample_rate,
            root_note: 60, // C3
            voices: (0..NUM_VOICES).map(|_| SamplerVoice::new()).collect(),
        }
    }

    pub fn note_on(&mut self, note: u8) {
        let Some(voice) = self.voices.iter_mut().find(|v| v.note.is_none()) else {
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
    fn next(&mut self, ctx: &Context) -> Sample {
        let mut ans: Sample = 0.0;
        for v in &mut self.voices {
            let Some(note) = v.note else { continue };
            let pitch_ratio = ((note as f32 - self.root_note as f32) / 12.0).exp2();
            let sample_rate_ratio = self.sample_rate / ctx.sample_rate;
            let increment = pitch_ratio * sample_rate_ratio;

            let index = v.position.floor() as usize;
            if index + 1 >= self.sample.len() {
                v.note = None;
                continue;
            }
            let fraction = v.position.fract();

            ans += self.sample[index] * (1.0 - fraction) + self.sample[index + 1] * fraction;
            v.position += increment;
        }
        ans
    }
    pub fn process(&mut self, ctx: &Context, out: &mut [Sample]) {
        for o in out.iter_mut() {
            *o = self.next(ctx);
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

fn decode_wav(wav: &[u8]) -> (Vec<Sample>, f32) {
    assert_eq!(&wav[..4], b"RIFF", "sampler asset must be a RIFF WAV");
    assert_eq!(&wav[8..12], b"WAVE", "sampler asset must be a WAVE file");

    let mut offset = 12;
    let mut sample_rate = None;
    let mut sample = None;
    while offset + 8 <= wav.len() {
        let chunk = &wav[offset..offset + 4];
        let len = u32::from_le_bytes(wav[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        let end = start.saturating_add(len).min(wav.len());

        match chunk {
            b"fmt " => {
                assert_eq!(
                    u16::from_le_bytes(wav[start..start + 2].try_into().unwrap()),
                    1
                );
                assert_eq!(
                    u16::from_le_bytes(wav[start + 2..start + 4].try_into().unwrap()),
                    2
                );
                assert_eq!(
                    u16::from_le_bytes(wav[start + 14..start + 16].try_into().unwrap()),
                    16
                );
                sample_rate =
                    Some(u32::from_le_bytes(wav[start + 4..start + 8].try_into().unwrap()) as f32);
            }
            b"data" => {
                let samples: Vec<_> = wav[start..end]
                    .chunks_exact(4)
                    .map(|frame| {
                        let left = i16::from_le_bytes([frame[0], frame[1]]) as f32;
                        let right = i16::from_le_bytes([frame[2], frame[3]]) as f32;
                        (left + right) / (2.0 * i16::MAX as f32)
                    })
                    .collect();
                sample = Some(samples);
            }
            _ => {}
        }

        offset = start.saturating_add(len).saturating_add(len % 2);
    }

    (
        sample.expect("sampler WAV is missing data"),
        sample_rate.expect("sampler WAV is missing format"),
    )
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

    pub fn handle_event(&mut self, _sampler: &mut Sampler, event: Event) -> Action {
        match event.key {
            Key::Backspace => Action::GoBack,
            _ => Action::None,
        }
    }

    pub fn render<D: RaylibDraw>(&self, _sampler: &Sampler, _d: &mut D) {}
}

impl Default for SamplerUi {
    fn default() -> Self {
        Self::new()
    }
}
