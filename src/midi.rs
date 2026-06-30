//! Port of `midi.zig`.

use crate::input::Key;

pub type Frame = u64;
pub const MAX_NOTES_PER_BLOCK: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub start: Frame,
    pub end: Frame,
    pub note: u8,
}

#[derive(Clone, Copy, Debug)]
pub enum NoteMsg {
    On(u8),
    Off(u8),
}

pub fn beats_to_frames(beats: f32, tempo: f32, sample_rate: f32) -> Frame {
    ((60.0 / tempo) * sample_rate * beats) as Frame
}

pub fn frames_to_beats(frames: Frame, tempo: f32, sample_rate: f32) -> f32 {
    frames as f32 / (sample_rate * 60.0 / tempo)
}

/// QWERTY -> MIDI note (one octave + black keys), mirrors `midi.keyToMidi`.
pub fn key_to_midi(key: Key) -> Option<u8> {
    use Key::*;
    Some(match key {
        // white keys (A–F row)
        A => 48,          // C3
        S => 50,          // D3
        D => 52,          // E3
        F => 53,          // F3
        G => 55,          // G3
        H => 57,          // A3
        J => 59,          // B3
        K => 60,          // C4
        L => 62,          // D4
        Semicolon => 64,  // E4
        Apostrophe => 65, // F4
        // black keys (W–P row)
        W => 49, // C#3
        E => 51, // D#3
        T => 54, // F#3
        Y => 56, // G#3
        U => 58, // A#3
        O => 61, // C#4
        P => 63, // D#4
        _ => return None,
    })
}

pub struct Player {
    pub notes: Vec<Note>,
}

impl Player {
    pub fn new(notes_in: &[Note]) -> Self {
        Self {
            notes: notes_in.to_vec(),
        }
    }

    pub fn clear(&mut self) {
        self.notes.clear();
    }

    pub fn append_notes(&mut self, new_notes: &[Note]) {
        self.notes.extend_from_slice(new_notes);
    }

    /// Emit note on/off messages whose boundary falls in `[start, end)`.
    /// Returns the number of messages written to `out`.
    pub fn advance(&self, start: Frame, end: Frame, out: &mut [NoteMsg]) -> usize {
        debug_assert!(end >= start);
        debug_assert!(end - start < 8192);

        let mut count = 0usize;
        for n in &self.notes {
            if start <= n.start && n.start < end && count < out.len() {
                out[count] = NoteMsg::On(n.note);
                count += 1;
            }
            if start <= n.end && n.end < end && count < out.len() {
                out[count] = NoteMsg::Off(n.note);
                count += 1;
            }
        }
        count
    }
}
