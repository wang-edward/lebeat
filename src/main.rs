use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use raylib::prelude::*;

fn main() {
    // Frequency in Hz, shared with the audio thread as f32 bits.
    // Lock-free: the callback just does a Relaxed load, no mutex.
    let freq = Arc::new(AtomicU32::new(440.0f32.to_bits()));

    // ---- audio (cpal) ----
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let supported = device.default_output_config().unwrap();
    let sample_rate = supported.sample_rate().0 as f32;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    assert!(
        sample_format == cpal::SampleFormat::F32,
        "this demo assumes an f32 output format (got {sample_format:?})"
    );

    let freq_audio = freq.clone();
    let mut phase: f32 = 0.0; // normalized 0..1, kept across callbacks => no clicks

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let f = f32::from_bits(freq_audio.load(Ordering::Relaxed));
                    phase += f / sample_rate;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                    let s = (phase * std::f32::consts::TAU).sin() * 0.2;
                    for out in frame.iter_mut() {
                        *out = s;
                    }
                }
            },
            |e| eprintln!("audio error: {e}"),
            None,
        )
        .unwrap();
    stream.play().unwrap();

    // ---- window (raylib) ----
    let (mut rl, thread) = raylib::init().size(640, 360).title("sine demo").build();
    rl.set_target_fps(60);

    while !rl.window_should_close() {
        let dt = rl.get_frame_time();
        let mut f = f32::from_bits(freq.load(Ordering::Relaxed));

        // hold up/down for continuous change
        if rl.is_key_down(KeyboardKey::KEY_UP) {
            f += 200.0 * dt;
        }
        if rl.is_key_down(KeyboardKey::KEY_DOWN) {
            f -= 200.0 * dt;
        }
        f = f.clamp(20.0, 20_000.0);
        freq.store(f.to_bits(), Ordering::Relaxed);

        let mut d = rl.begin_drawing(&thread);
        d.clear_background(Color::BLACK);

        // visual waveform: cycles scale with pitch (not the real audio rate, just feedback)
        let w = 640;
        let mid = 220.0f32;
        let amp = 70.0f32;
        let cycles = f / 60.0;
        let mut prev_y = mid;
        for x in 0..w {
            let t = x as f32 / w as f32;
            let y = mid - (t * cycles * std::f32::consts::TAU).sin() * amp;
            if x > 0 {
                d.draw_line(x - 1, prev_y as i32, x, y as i32, Color::SKYBLUE);
            }
            prev_y = y;
        }

        d.draw_text(&format!("{f:.1} Hz"), 20, 20, 40, Color::LIME);
        d.draw_text("up / down to change", 20, 70, 20, Color::GRAY);
    }
}
