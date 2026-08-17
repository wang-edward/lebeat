use std::path::Path;

use raylib::prelude::*;

use crate::audio::{Context, Sample};
use crate::input::{Event, Key};
use crate::ui::Action;

const NUM_VOICES: usize = 8;

pub struct SampleBuffer {
    samples: Vec<Sample>,
    sample_rate: f32,
}
impl SampleBuffer {
    pub fn load_wav(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self::decode_wav(&std::fs::read(path)?))
    }

    pub fn decode_wav(wav: &[u8]) -> Self {
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");

        let mut offset = 12;
        let mut format = None;
        while offset + 8 <= wav.len() {
            let chunk = &wav[offset..offset + 4];
            let len = u32::from_le_bytes(wav[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let start = offset + 8;

            match chunk {
                b"fmt " => {
                    let encoding = u16::from_le_bytes(wav[start..start + 2].try_into().unwrap());
                    assert_eq!(encoding, 1);
                    let channels =
                        u16::from_le_bytes(wav[start + 2..start + 4].try_into().unwrap());
                    assert!((1..=2).contains(&channels));
                    let sample_rate =
                        u32::from_le_bytes(wav[start + 4..start + 8].try_into().unwrap()) as f32;
                    let bits = u16::from_le_bytes(wav[start + 14..start + 16].try_into().unwrap());
                    assert_eq!(bits, 16);

                    format = Some((channels, sample_rate));
                }
                b"data" => {
                    let (channels, sample_rate) = format.expect("sampler WAV is missing format");
                    let frame_len = channels as usize * 2;
                    assert!(len.is_multiple_of(frame_len));
                    let samples: Vec<_> = wav[start..start + len]
                        .chunks_exact(frame_len)
                        .map(|frame| {
                            let left = i16::from_le_bytes([frame[0], frame[1]]) as f32;
                            if channels == 1 {
                                left / i16::MAX as f32
                            } else {
                                let right = i16::from_le_bytes([frame[2], frame[3]]) as f32;
                                (left + right) / (2.0 * i16::MAX as f32)
                            }
                        })
                        .collect();
                    return Self {
                        samples,
                        sample_rate,
                    };
                }
                _ => {}
            }

            offset = start + len + len % 2;
        }

        panic!("sampler WAV is missing data")
    }
}

pub struct Sampler {
    sample: SampleBuffer,
    root_note: u8,
    voices: Vec<SamplerVoice>,
}
struct SamplerVoice {
    note: Option<u8>,
    position: f32,
}

impl Sampler {
    pub fn new(sample: SampleBuffer) -> Self {
        Self {
            sample,
            root_note: 60, // C3
            voices: (0..NUM_VOICES).map(|_| SamplerVoice::new()).collect(),
        }
    }

    pub fn set_sample(&mut self, sample: SampleBuffer) {
        self.all_notes_off();
        self.sample = sample;
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
            let sample_rate_ratio = self.sample.sample_rate / ctx.sample_rate;
            let increment = pitch_ratio * sample_rate_ratio;

            let index = v.position.floor() as usize;
            if index + 1 >= self.sample.samples.len() {
                v.note = None;
                continue;
            }
            let fraction = v.position.fract();

            ans += self.sample.samples[index] * (1.0 - fraction)
                + self.sample.samples[index + 1] * fraction;
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
        let sample = SampleBuffer::decode_wav(include_bytes!("../../assets/samples/perfect.wav"));
        Self::new(sample)
    }
}

impl SamplerVoice {
    pub fn new() -> Self {
        SamplerVoice {
            note: None,
            position: 0f32,
        }
    }
}

pub enum SamplerScreen {
    Overview,
    Picker,
}
pub struct SamplerUi {
    screen: SamplerScreen,
}

impl SamplerUi {
    pub fn new() -> Self {
        SamplerUi {
            screen: SamplerScreen::Overview,
        }
    }

    pub fn handle_event(&mut self, _sampler: &mut Sampler, event: Event) -> Action {
        match self.screen {
            SamplerScreen::Overview => match event.key {
                Key::Backspace => return Action::GoBack,
                Key::R => self.screen = SamplerScreen::Picker,
                _ => {}
            },
            SamplerScreen::Picker => match event.key {
                Key::Backspace => self.screen = SamplerScreen::Overview,
                _ => {}
            },
        }
        Action::None
    }

    pub fn render<D: RaylibDraw>(&self, _sampler: &Sampler, d: &mut D) {
        match self.screen {
            SamplerScreen::Overview => {
                d.draw_text("OVERVIEW", 0, 0, 10, Color::WHITE);
            }
            SamplerScreen::Picker => {
                d.draw_text("PICKER", 0, 0, 10, Color::WHITE);
            }
        }
    }
}

impl Default for SamplerUi {
    fn default() -> Self {
        Self::new()
    }
}
