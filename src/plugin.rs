use raylib::prelude::*;

use crate::audio::{Context, Delay as DspDelay, Lpf as DspLpf, Param, Sample};
use crate::input::{Event, Key};
use crate::interface::draw_text_centered;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Lpf,
    Delay,
}

pub const LIST: [Tag; 2] = [Tag::Lpf, Tag::Delay];

impl Tag {
    pub fn name(self) -> &'static str {
        match self {
            Tag::Lpf => "lpf",
            Tag::Delay => "delay",
        }
    }
}

pub enum Action {
    None,
    GoBack,
}

pub struct Knob {
    param: KnobParam,
    pos: Vector2,
    radius: f32,
    color: Color,
    name: &'static str,
}

#[derive(Clone, Copy)]
enum KnobParam {
    First,
    Second,
    Third,
}

impl Knob {
    fn new(param: KnobParam, x: f32, y: f32, name: &'static str) -> Self {
        Self {
            param,
            pos: Vector2::new(x, y),
            radius: 10.0,
            color: Color::WHITE,
            name,
        }
    }

    fn render<D: RaylibDraw>(&self, d: &mut D, param: &Param) {
        let angle = param.get_norm() * 360.0;
        d.draw_circle_sector(self.pos, self.radius, 0.0, angle, 360, self.color);
        draw_text_centered(
            d,
            self.name,
            self.pos.x as i32,
            (self.pos.y + 20.0) as i32,
            10,
            self.color,
        );
    }
}

pub enum Plugin {
    Lpf(Lpf),
    Delay(Delay),
}

impl Plugin {
    pub fn new(tag: Tag) -> Self {
        match tag {
            Tag::Lpf => Self::Lpf(Lpf::new()),
            Tag::Delay => Self::Delay(Delay::new()),
        }
    }

    pub fn tag(&self) -> Tag {
        match self {
            Self::Lpf(_) => Tag::Lpf,
            Self::Delay(_) => Tag::Delay,
        }
    }

    pub fn process(&mut self, ctx: &Context, buf: &mut [Sample]) {
        match self {
            Self::Lpf(plugin) => plugin.process(ctx, buf),
            Self::Delay(plugin) => plugin.process(ctx, buf),
        }
    }

    pub fn handle_event(&mut self, event: Event) -> Action {
        match self {
            Self::Lpf(plugin) => plugin.handle_event(event),
            Self::Delay(plugin) => plugin.handle_event(event),
        }
    }

    pub fn render<D: RaylibDraw>(&self, d: &mut D) {
        match self {
            Self::Lpf(plugin) => plugin.render(d),
            Self::Delay(plugin) => plugin.render(d),
        }
    }
}

pub struct Lpf {
    dsp: DspLpf,
    knobs: [Knob; 3],
}

impl Lpf {
    fn new() -> Self {
        Self {
            dsp: DspLpf::new(),
            knobs: [
                Knob::new(KnobParam::First, 32.0, 32.0, "drive"),
                Knob::new(KnobParam::Second, 96.0, 32.0, "resonance"),
                Knob::new(KnobParam::Third, 32.0, 96.0, "cutoff"),
            ],
        }
    }

    fn process(&mut self, ctx: &Context, buf: &mut [Sample]) {
        self.dsp.render(ctx, buf);
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event.key {
            Key::Backspace => return Action::GoBack,
            Key::One => nudge(&mut self.dsp.drive, -0.1),
            Key::Two => nudge(&mut self.dsp.drive, 0.1),
            Key::Three => nudge(&mut self.dsp.resonance, -0.1),
            Key::Four => nudge(&mut self.dsp.resonance, 0.1),
            Key::Five => nudge(&mut self.dsp.cutoff, -0.1),
            Key::Six => nudge(&mut self.dsp.cutoff, 0.1),
            _ => {}
        }
        Action::None
    }

    fn render<D: RaylibDraw>(&self, d: &mut D) {
        for knob in &self.knobs {
            let param = match knob.param {
                KnobParam::First => &self.dsp.drive,
                KnobParam::Second => &self.dsp.resonance,
                KnobParam::Third => &self.dsp.cutoff,
            };
            knob.render(d, param);
        }
        draw_text_centered(d, "LPF", 64, 64, 10, Color::GREEN);
    }
}

pub struct Delay {
    dsp: DspDelay,
    knobs: [Knob; 3],
}

impl Delay {
    fn new() -> Self {
        Self {
            dsp: DspDelay::new(48_000),
            knobs: [
                Knob::new(KnobParam::First, 32.0, 32.0, "delay_time"),
                Knob::new(KnobParam::Second, 96.0, 32.0, "feedback"),
                Knob::new(KnobParam::Third, 32.0, 96.0, "mix"),
            ],
        }
    }

    fn process(&mut self, ctx: &Context, buf: &mut [Sample]) {
        self.dsp.render(ctx, buf);
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event.key {
            Key::Backspace => return Action::GoBack,
            Key::One => nudge(&mut self.dsp.delay_time, -0.1),
            Key::Two => nudge(&mut self.dsp.delay_time, 0.1),
            Key::Three => nudge(&mut self.dsp.feedback, -0.1),
            Key::Four => nudge(&mut self.dsp.feedback, 0.1),
            Key::Five => nudge(&mut self.dsp.mix, -0.1),
            Key::Six => nudge(&mut self.dsp.mix, 0.1),
            _ => {}
        }
        Action::None
    }

    fn render<D: RaylibDraw>(&self, d: &mut D) {
        for knob in &self.knobs {
            let param = match knob.param {
                KnobParam::First => &self.dsp.delay_time,
                KnobParam::Second => &self.dsp.feedback,
                KnobParam::Third => &self.dsp.mix,
            };
            knob.render(d, param);
        }
        draw_text_centered(d, "DELAY", 64, 64, 10, Color::PURPLE);
    }
}

fn nudge(param: &mut Param, delta: f32) {
    param.set_norm(param.get_norm() + delta);
}
