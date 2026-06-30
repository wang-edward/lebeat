//! Port of `audio.zig`.
//!
//! The Zig version used a manual vtable (`Node{ ptr: *anyopaque, v: *const VTable }`)
//! with each node storing a pointer to its *input* node and pulling from it. That is a
//! self-referential graph and does not translate to safe Rust. Instead we split the two
//! roles the Zig code conflated:
//!
//!   * a **source** fills a buffer (`Osc`, `synth::Uni`, `Track`, `Timeline`)
//!   * an **effect** transforms a buffer **in place** (`Lpf`, `Delay`, `Adsr`)
//!
//! so the synth -> plugin chain is just a sequence of in-place passes (see `project::Track`).
//! The Zig `ArenaAllocator` (per-block scratch) maps to `bumpalo::Bump`: it allocates
//! through `&self`, so recursive `ctx.tmp()` calls hand out disjoint `&mut [f32]` slices
//! without tripping the borrow checker.

use bumpalo::Bump;

pub type Sample = f32;

pub struct Context {
    pub sample_rate: f32,
    pub bpm: f32,
    pub bump: Bump,
}

impl Context {
    pub fn new(sample_rate: f32, bpm: f32) -> Self {
        Self {
            sample_rate,
            bpm,
            bump: Bump::new(),
        }
    }

    /// Reset the scratch arena. Call once at the top of each audio block.
    pub fn begin_block(&mut self) {
        self.bump.reset();
    }

    /// Hand out a zeroed scratch buffer that lives until the next `begin_block`.
    pub fn tmp(&self, n: usize) -> &mut [Sample] {
        self.bump.alloc_slice_fill_copy(n, 0.0)
    }
}

// ------------------------------------------------------------------ Param

#[derive(Clone, Copy)]
pub struct Param {
    val: f32,
    pub min: f32,
    pub max: f32,
}

impl Param {
    pub fn new(val: f32, min: f32, max: f32) -> Self {
        Self { val, min, max }
    }
    pub fn set(&mut self, new_val: f32) {
        debug_assert!(self.min < self.max);
        self.val = new_val.clamp(self.min, self.max);
    }
    pub fn get(&self) -> f32 {
        debug_assert!(self.min < self.max && self.min <= self.val && self.val <= self.max);
        self.val
    }
    pub fn get_norm(&self) -> f32 {
        debug_assert!(self.min < self.max);
        (self.val - self.min) / (self.max - self.min)
    }
    pub fn set_norm(&mut self, norm: f32) {
        let n = norm.clamp(0.0, 1.0);
        self.set(self.min + n * (self.max - self.min));
    }
}

// ------------------------------------------------------------------ Osc (source)

#[derive(Clone, Copy)]
pub enum OscKind {
    Sine,
    Pwm { duty: f32 },
    Saw,
    Sub { duty: f32, offset: f32 },
}

pub struct Osc {
    pub phase: f32,
    pub freq: f32,
    pub kind: OscKind,
}

impl Osc {
    pub fn new(freq: f32, kind: OscKind) -> Self {
        Self {
            phase: 0.0,
            freq,
            kind,
        }
    }

    pub fn reset_phase(&mut self) {
        self.phase = 0.0;
    }

    /// Source: fills `out`.
    pub fn render(&mut self, ctx: &Context, out: &mut [Sample]) {
        let base_inc = self.freq / ctx.sample_rate;
        let inc = match self.kind {
            OscKind::Sub { offset, .. } => base_inc * (offset / 12.0).exp2(),
            _ => base_inc,
        };
        for s in out.iter_mut() {
            *s = match self.kind {
                OscKind::Sine => (self.phase * 2.0 * std::f32::consts::PI).sin(),
                OscKind::Pwm { duty } => {
                    if self.phase < duty {
                        1.0
                    } else {
                        -1.0
                    }
                }
                OscKind::Saw => 2.0 * self.phase - 1.0,
                OscKind::Sub { duty, .. } => {
                    if self.phase < duty {
                        1.0
                    } else {
                        -1.0
                    }
                }
            };
            self.phase += inc;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

// ------------------------------------------------------------------ Lpf (in-place effect)

/// Moog ladder, D'Angelo/Valimaki "An Improved Virtual Analog Model".
pub struct Lpf {
    v: [f32; 4],
    dv: [f32; 4],
    tv: [f32; 4],
    pub drive: Param,
    pub resonance: Param,
    pub cutoff: Param,
}

impl Lpf {
    pub const THERMAL_VOLTAGE: f32 = 0.312;

    pub fn new() -> Self {
        Self {
            v: [0.0; 4],
            dv: [0.0; 4],
            tv: [0.0; 4],
            drive: Param::new(1.0, 0.0, 2.0),
            resonance: Param::new(0.5, 0.0, 2.0),
            // Moog ladder needs cutoff < sr/pi (~15278 Hz @ 48k); above 10k sounds odd, so cap there.
            cutoff: Param::new(5_000.0, 0.0, 10_000.0),
        }
    }

    /// In-place: reads and overwrites `buf`.
    pub fn render(&mut self, ctx: &Context, buf: &mut [Sample]) {
        let vt = Self::THERMAL_VOLTAGE;
        let two_sr = 2.0 * ctx.sample_rate;
        let cutoff = self.cutoff.get();
        let drive = self.drive.get();
        let res = self.resonance.get();

        let x = (std::f32::consts::PI * cutoff) / ctx.sample_rate;
        let g = 4.0 * std::f32::consts::PI * vt * cutoff * (1.0 - x) / (1.0 + x);

        for s in buf.iter_mut() {
            let input = *s;

            let dv0 = -g * ((drive * input + res * self.v[3] / (2.0 * vt)) + self.tv[0]).tanh();
            self.v[0] += (dv0 + self.dv[0]) / two_sr;
            self.dv[0] = dv0;
            self.tv[0] = (self.v[0] / (2.0 * vt)).tanh();

            let dv1 = g * (self.tv[0] - self.tv[1]);
            self.v[1] += (dv1 + self.dv[1]) / two_sr;
            self.dv[1] = dv1;
            self.tv[1] = (self.v[1] / (2.0 * vt)).tanh();

            let dv2 = g * (self.tv[1] - self.tv[2]);
            self.v[2] += (dv2 + self.dv[2]) / two_sr;
            self.dv[2] = dv2;
            self.tv[2] = (self.v[2] / (2.0 * vt)).tanh();

            let dv3 = g * (self.tv[2] - self.tv[3]);
            self.v[3] += (dv3 + self.dv[3]) / two_sr;
            self.dv[3] = dv3;
            self.tv[3] = (self.v[3] / (2.0 * vt)).tanh();

            *s = self.v[3];
        }
    }
}

impl Default for Lpf {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------ Delay (in-place effect)

pub struct Delay {
    buffer: Vec<Sample>,
    write_pos: usize,
    pub delay_time: Param, // seconds
    pub feedback: Param,
    pub mix: Param, // [0, 1]
}

impl Delay {
    pub fn new(buffer_size: usize) -> Self {
        Self {
            buffer: vec![0.0; buffer_size],
            write_pos: 0,
            delay_time: Param::new(0.25, 0.0, 1.0),
            feedback: Param::new(0.6, 0.0, 1.0),
            mix: Param::new(0.5, 0.0, 1.0),
        }
    }

    pub fn render(&mut self, ctx: &Context, buf: &mut [Sample]) {
        let delay_samples = (self.delay_time.get() * ctx.sample_rate) as usize;
        let buffer_len = self.buffer.len();
        debug_assert!(delay_samples <= buffer_len);

        let fb = self.feedback.get();
        let mix = self.mix.get();

        for s in buf.iter_mut() {
            let dry = *s;
            let read_pos = if self.write_pos >= delay_samples {
                self.write_pos - delay_samples
            } else {
                buffer_len - (delay_samples - self.write_pos)
            };
            let delayed = self.buffer[read_pos];

            self.buffer[self.write_pos] = dry + delayed * fb;
            *s = dry * (1.0 - mix) + delayed * mix;
            self.write_pos = (self.write_pos + 1) % buffer_len;
        }
    }
}

// ------------------------------------------------------------------ Adsr (in-place gate)

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct Adsr {
    pub value: f32,
    pub stage: AdsrStage,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Adsr {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            stage: AdsrStage::Idle,
            attack: 0.01,
            decay: 0.1,
            sustain: 0.4,
            release: 0.6,
        }
    }

    pub fn note_on(&mut self) {
        self.stage = AdsrStage::Attack;
    }
    pub fn note_off(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.stage = AdsrStage::Release;
        }
    }

    /// In-place gate: multiplies `buf` by the envelope. Zeroes `buf` if idle.
    pub fn render(&mut self, ctx: &Context, buf: &mut [Sample]) {
        if self.stage == AdsrStage::Idle {
            buf.fill(0.0);
            return;
        }
        let sr = ctx.sample_rate;
        for s in buf.iter_mut() {
            match self.stage {
                AdsrStage::Idle => self.value = 0.0,
                AdsrStage::Attack => {
                    self.value += 1.0 / (self.attack * sr);
                    if self.value >= 1.0 {
                        self.value = 1.0;
                        self.stage = AdsrStage::Decay;
                    }
                }
                AdsrStage::Decay => {
                    self.value -= (1.0 - self.sustain) / (self.decay * sr);
                    if self.value <= self.sustain {
                        self.value = self.sustain;
                        self.stage = AdsrStage::Sustain;
                    }
                }
                AdsrStage::Sustain => {}
                AdsrStage::Release => {
                    self.value -= self.sustain / (self.release * sr);
                    if self.value <= 0.0 {
                        self.value = 0.0;
                        self.stage = AdsrStage::Idle;
                    }
                }
            }
            *s *= self.value;
        }
    }
}

impl Default for Adsr {
    fn default() -> Self {
        Self::new()
    }
}
