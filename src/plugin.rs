use raylib::prelude::*;

use crate::audio::{Context, Delay, Lpf, Param, Sample};
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

pub enum Plugin {
    Lpf(LpfPlugin),
    Delay(DelayPlugin),
}

impl Plugin {
    pub fn new(tag: Tag) -> Self {
        match tag {
            Tag::Lpf => Self::Lpf(LpfPlugin::new()),
            Tag::Delay => Self::Delay(DelayPlugin::new()),
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
}

pub struct LpfPlugin {
    dsp: Lpf,
}

impl LpfPlugin {
    fn new() -> Self {
        Self { dsp: Lpf::new() }
    }

    fn process(&mut self, ctx: &Context, buf: &mut [Sample]) {
        self.dsp.render(ctx, buf);
    }
}

pub struct DelayPlugin {
    dsp: Delay,
}

impl DelayPlugin {
    fn new() -> Self {
        Self {
            dsp: Delay::new(48_000),
        }
    }

    fn process(&mut self, ctx: &Context, buf: &mut [Sample]) {
        self.dsp.render(ctx, buf);
    }
}

pub struct Knob {
    pos: Vector2,
    radius: f32,
    color: Color,
    name: &'static str,
}

impl Knob {
    fn new(x: f32, y: f32, name: &'static str) -> Self {
        Self {
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

pub enum PluginUi {
    Lpf(LpfPluginUi),
    Delay(DelayPluginUi),
}

impl PluginUi {
    pub fn new(tag: Tag) -> Self {
        match tag {
            Tag::Lpf => Self::Lpf(LpfPluginUi::new()),
            Tag::Delay => Self::Delay(DelayPluginUi::new()),
        }
    }

    pub fn handle_event(&mut self, plugin: &mut Plugin, event: Event) -> Action {
        match self {
            Self::Lpf(ui) => {
                let Plugin::Lpf(plugin) = plugin else {
                    return Action::None;
                };
                ui.handle_event(plugin, event)
            }
            Self::Delay(ui) => {
                let Plugin::Delay(plugin) = plugin else {
                    return Action::None;
                };
                ui.handle_event(plugin, event)
            }
        }
    }

    pub fn render<D: RaylibDraw>(&self, plugin: &Plugin, d: &mut D) {
        match self {
            Self::Lpf(ui) => {
                let Plugin::Lpf(plugin) = plugin else {
                    return;
                };
                ui.render(plugin, d)
            }
            Self::Delay(ui) => {
                let Plugin::Delay(plugin) = plugin else {
                    return;
                };
                ui.render(plugin, d)
            }
        }
    }
}

pub struct LpfPluginUi {
    drive: Knob,
    resonance: Knob,
    cutoff: Knob,
}

impl LpfPluginUi {
    fn new() -> Self {
        Self {
            drive: Knob::new(32.0, 32.0, "drive"),
            resonance: Knob::new(96.0, 32.0, "resonance"),
            cutoff: Knob::new(32.0, 96.0, "cutoff"),
        }
    }

    fn handle_event(&mut self, plugin: &mut LpfPlugin, event: Event) -> Action {
        match event.key {
            Key::Backspace => return Action::GoBack,
            Key::One => nudge(&mut plugin.dsp.drive, -0.1),
            Key::Two => nudge(&mut plugin.dsp.drive, 0.1),
            Key::Three => nudge(&mut plugin.dsp.resonance, -0.1),
            Key::Four => nudge(&mut plugin.dsp.resonance, 0.1),
            Key::Five => nudge(&mut plugin.dsp.cutoff, -0.1),
            Key::Six => nudge(&mut plugin.dsp.cutoff, 0.1),
            _ => {}
        }
        Action::None
    }

    fn render<D: RaylibDraw>(&self, plugin: &LpfPlugin, d: &mut D) {
        self.drive.render(d, &plugin.dsp.drive);
        self.resonance.render(d, &plugin.dsp.resonance);
        self.cutoff.render(d, &plugin.dsp.cutoff);
        draw_text_centered(d, "LPF", 64, 64, 10, Color::GREEN);
    }
}

pub struct DelayPluginUi {
    delay_time: Knob,
    feedback: Knob,
    mix: Knob,
}

impl DelayPluginUi {
    fn new() -> Self {
        Self {
            delay_time: Knob::new(32.0, 32.0, "delay_time"),
            feedback: Knob::new(96.0, 32.0, "feedback"),
            mix: Knob::new(32.0, 96.0, "mix"),
        }
    }

    fn handle_event(&mut self, plugin: &mut DelayPlugin, event: Event) -> Action {
        match event.key {
            Key::Backspace => return Action::GoBack,
            Key::One => nudge(&mut plugin.dsp.delay_time, -0.1),
            Key::Two => nudge(&mut plugin.dsp.delay_time, 0.1),
            Key::Three => nudge(&mut plugin.dsp.feedback, -0.1),
            Key::Four => nudge(&mut plugin.dsp.feedback, 0.1),
            Key::Five => nudge(&mut plugin.dsp.mix, -0.1),
            Key::Six => nudge(&mut plugin.dsp.mix, 0.1),
            _ => {}
        }
        Action::None
    }

    fn render<D: RaylibDraw>(&self, plugin: &DelayPlugin, d: &mut D) {
        self.delay_time.render(d, &plugin.dsp.delay_time);
        self.feedback.render(d, &plugin.dsp.feedback);
        self.mix.render(d, &plugin.dsp.mix);
        draw_text_centered(d, "DELAY", 64, 64, 10, Color::PURPLE);
    }
}

fn nudge(param: &mut Param, delta: f32) {
    param.set_norm(param.get_norm() + delta);
}
