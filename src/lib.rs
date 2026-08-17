//! ledaw — Rust port of the Zig groovebox engine.
//!
//! The modules below are the portable, dependency-free DSP/data core and are
//! compiled + tested here. The FFI/UI layers (interface, oled, plugin, project, main)
//! live in their own files and depend on external crates (raylib, soundio, spidev/gpiod);
//! see notes in those files.

pub mod audio;
pub mod audio_out;
pub mod engine;
pub mod input;
pub mod midi;
pub mod plugin;
pub mod synth;

pub mod interface;
pub mod ui;

#[cfg(test)]
mod tests {
    use crate::audio::Context;
    use crate::midi::{self, Note};
    use crate::synth::Uni;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 256;

    #[test]
    fn synth_produces_finite_audio_and_releases() {
        let ctx = Context::new(SR, 120.0);
        let mut synth = Uni::new();
        let mut out = vec![0.0; BLOCK];

        synth.note_on(69); // A4
        synth.note_on(72);

        let mut peak = 0.0f32;
        for _ in 0..50 {
            synth.process(&ctx, &mut out);
            for &s in out.iter() {
                assert!(s.is_finite(), "non-finite sample");
                peak = peak.max(s.abs());
            }
        }
        assert!(peak > 0.0, "synth was silent");

        synth.note_off(69);
        synth.note_off(72);
        // run long enough for the 0.6s release to finish
        for _ in 0..(SR as usize / BLOCK + 200) {
            synth.process(&ctx, &mut out);
        }
        assert!(synth.is_idle(), "voices never returned to idle");
    }

    #[test]
    fn track_owns_its_notes() {
        use crate::engine::Track;

        let notes = [Note {
            start: 10,
            end: 100,
            note: 60,
        }];
        let track = Track::new(&notes);

        assert_eq!(track.notes.len(), 1);
        assert_eq!(track.notes[0].start, 10);
        assert_eq!(track.notes[0].end, 100);
        assert_eq!(track.notes[0].note, 60);
    }

    #[test]
    fn beats_frames_round_trip() {
        let f = midi::beats_to_frames(4.0, 120.0, SR);
        let b = midi::frames_to_beats(f, 120.0, SR);
        assert!((b - 4.0).abs() < 1e-3);
    }

    #[test]
    fn engine_two_track_demo_renders() {
        use crate::engine::{Engine, Plugin, PluginTag, Track};
        use crate::midi::{Note, beats_to_frames};
        let sr = SR;
        let mk = |b0: f32, b1: f32, n: u8| Note {
            start: beats_to_frames(b0, 120.0, sr),
            end: beats_to_frames(b1, 120.0, sr),
            note: n,
        };
        let lead = [mk(0.0, 0.9, 60), mk(1.0, 1.9, 67), mk(2.0, 2.9, 69)];
        let bass = [mk(0.0, 2.0, 48), mk(2.0, 4.0, 43)];

        let mut eng = Engine::new(sr, 120.0);
        eng.add_track(Track::new(&lead));
        eng.add_track(Track::new(&bass));
        eng.add_plugin(0, Plugin::new(PluginTag::Lpf));
        eng.add_plugin(1, Plugin::new(PluginTag::Delay));
        assert_eq!(eng.track_count(), 2);

        eng.toggle_play();
        let mut peak = 0.0f32;
        for _ in 0..400 {
            let out = ctx_block();
            // use engine-owned ctx via process_block
            let mut buf = vec![0.0f32; BLOCK];
            eng.process_block(&mut buf);
            for &s in &buf {
                assert!(s.is_finite());
                peak = peak.max(s.abs());
            }
            let _ = out;
        }
        assert!(peak > 0.0, "demo timeline produced silence");
    }

    fn ctx_block() -> usize {
        BLOCK
    }
}
