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
pub mod synth;

pub mod interface;
pub mod ui;

#[cfg(test)]
mod tests {
    use crate::audio::Context;
    use crate::midi::{self, Note, NoteMsg, Player};
    use crate::synth::Uni;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 256;

    #[test]
    fn synth_produces_finite_audio_and_releases() {
        let mut ctx = Context::new(SR, 120.0);
        let mut synth = Uni::new();

        synth.note_on(69); // A4
        synth.note_on(72);

        let mut peak = 0.0f32;
        for _ in 0..50 {
            ctx.begin_block();
            let out = ctx.tmp(BLOCK);
            synth.render(&ctx, out);
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
            ctx.begin_block();
            let out = ctx.tmp(BLOCK);
            synth.render(&ctx, out);
        }
        assert!(synth.is_idle(), "voices never returned to idle");
    }

    #[test]
    fn bump_hands_out_disjoint_scratch() {
        // The crux of the arena port: multiple tmp() slices from one &Context must not alias.
        let ctx = Context::new(SR, 120.0);
        let a = ctx.tmp(4);
        let b = ctx.tmp(4);
        a[0] = 1.0;
        b[0] = 2.0;
        assert_eq!(a[0], 1.0);
        assert_eq!(b[0], 2.0);
    }

    #[test]
    fn player_advance_emits_on_then_off() {
        let notes = [Note {
            start: 10,
            end: 100,
            note: 60,
        }];
        let p = Player::new(&notes);
        let mut out = [NoteMsg::On(0); 8];

        let n = p.advance(0, 50, &mut out);
        assert_eq!(n, 1);
        assert!(matches!(out[0], NoteMsg::On(60)));

        let n = p.advance(50, 150, &mut out);
        assert_eq!(n, 1);
        assert!(matches!(out[0], NoteMsg::Off(60)));
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
