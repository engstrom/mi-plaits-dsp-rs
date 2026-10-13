//! 808 bass drum model, revisited.
//!
//! Grout change (engstrom/grout#166): `render`'s loop body is `step`, what comes before it
//! `begin` and what after it `end`, so `BassDrumEngine` can run a sample of this model beside a
//! sample of the synthetic one. `render` is those three, and the same arithmetic in the same
//! order as before.

// Based on MIT-licensed code (c) 2016 by Emilie Gillet (emilie.o.gillet@gmail.com)

#[allow(unused_imports)]
use num_traits::float::Float;

use crate::oscillator::sine_oscillator::SineOscillator;
use crate::utils::filter::{FilterMode, FrequencyApproximation, Svf};
use crate::utils::one_pole;
use crate::utils::parameter_interpolator::{DetachedInterpolator, ParameterInterpolator};
use crate::utils::units::semitones_to_ratio;

#[derive(Debug, Default, Clone)]
pub struct AnalogBassDrum {
    pulse_remaining_samples: i32,
    fm_pulse_remaining_samples: i32,
    pulse: f32,
    pulse_height: f32,
    pulse_lp: f32,
    fm_pulse_lp: f32,
    retrig_pulse: f32,
    lp_out: f32,
    tone_lp: f32,
    sustain_gain: f32,

    resonator: Svf,

    // Replace the resonator in "free running" (sustain) mode.
    oscillator: SineOscillator,

    // Sample rate dependent constants
    trigger_pulse_duration: i32,
    fm_pulse_duration: i32,
    pulse_decay_time: f32,
    pulse_filter_time: f32,
    retrig_pulse_duration: f32,
}

impl AnalogBassDrum {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, sample_rate_hz: f32) {
        self.pulse_remaining_samples = 0;
        self.fm_pulse_remaining_samples = 0;
        self.pulse = 0.0;
        self.pulse_height = 0.0;
        self.pulse_lp = 0.0;
        self.fm_pulse_lp = 0.0;
        self.retrig_pulse = 0.0;
        self.lp_out = 0.0;
        self.tone_lp = 0.0;
        self.sustain_gain = 0.0;

        self.resonator.init();
        self.oscillator.init();

        // Pre-compute sample rate dependent constants
        self.trigger_pulse_duration = (1.0e-3 * sample_rate_hz) as i32;
        self.fm_pulse_duration = (6.0e-3 * sample_rate_hz) as i32;
        self.pulse_decay_time = 0.2e-3 * sample_rate_hz;
        self.pulse_filter_time = 0.1e-3 * sample_rate_hz;
        self.retrig_pulse_duration = 0.05 * sample_rate_hz;
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn render(
        &mut self,
        sustain: bool,
        trigger: bool,
        accent: f32,
        f0: f32,
        tone: f32,
        decay: f32,
        attack_fm_amount: f32,
        self_fm_amount: f32,
        out: &mut [f32],
    ) {
        let mut block = self.begin(
            sustain,
            trigger,
            accent,
            f0,
            tone,
            decay,
            attack_fm_amount,
            self_fm_amount,
            out.len(),
        );
        for out_sample in out.iter_mut() {
            *out_sample = self.step(&mut block);
        }
        self.end(&block);
    }

    /// [`Self::render`] before its loop: the trigger, and what every sample of the block reads.
    /// [`Self::step`] is one sample of the loop and [`Self::end`] what follows it, so a caller can
    /// run another model's samples beside this one's (Grout #166) and get the samples `render`
    /// writes.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub(crate) fn begin(
        &mut self,
        sustain: bool,
        trigger: bool,
        accent: f32,
        f0: f32,
        tone: f32,
        decay: f32,
        attack_fm_amount: f32,
        self_fm_amount: f32,
        size: usize,
    ) -> AnalogBassDrumBlock {
        let scale = 0.001 / f0;
        let q = 1500.0 * semitones_to_ratio(decay * 80.0);
        let tone_f = f32::min(4.0 * f0 * semitones_to_ratio(tone * 108.0), 1.0);
        let exciter_leak = 0.08 * (tone + 0.25);

        if trigger {
            self.pulse_remaining_samples = self.trigger_pulse_duration;
            self.fm_pulse_remaining_samples = self.fm_pulse_duration;
            self.pulse_height = 3.0 + 7.0 * accent;
            self.lp_out = 0.0;
        }

        let sustain_gain = ParameterInterpolator::new(&mut self.sustain_gain, accent * decay, size);

        AnalogBassDrumBlock {
            sustain,
            f0,
            scale,
            q,
            tone_f,
            exciter_leak,
            attack_fm_amount,
            self_fm_amount,
            sustain_gain: sustain_gain.detach(),
        }
    }

    /// One sample of [`Self::render`]'s loop.
    #[inline(always)]
    pub(crate) fn step(&mut self, b: &mut AnalogBassDrumBlock) -> f32 {
        let sustain = b.sustain;
        // Q39 / Q40
        let mut pulse;
        if self.pulse_remaining_samples != 0 {
            self.pulse_remaining_samples -= 1;
            pulse = if self.pulse_remaining_samples != 0 {
                self.pulse_height
            } else {
                self.pulse_height - 1.0
            };
            self.pulse = pulse;
        } else {
            self.pulse *= 1.0 - 1.0 / self.pulse_decay_time;
            pulse = self.pulse;
        }
        if sustain {
            pulse = 0.0;
        }

        // C40 / R163 / R162 / D83
        one_pole(&mut self.pulse_lp, pulse, 1.0 / self.pulse_filter_time);
        pulse = diode((pulse - self.pulse_lp) + pulse * 0.044);

        // Q41 / Q42
        let mut fm_pulse = 0.0;
        if self.fm_pulse_remaining_samples != 0 {
            self.fm_pulse_remaining_samples -= 1;
            fm_pulse = 1.0;
            // C39 / C52
            self.retrig_pulse = if self.fm_pulse_remaining_samples != 0 {
                0.0
            } else {
                -0.8
            };
        } else {
            // C39 / R161
            self.retrig_pulse *= 1.0 - 1.0 / self.retrig_pulse_duration;
        }
        if sustain {
            fm_pulse = 0.0;
        }
        one_pole(
            &mut self.fm_pulse_lp,
            fm_pulse,
            1.0 / self.pulse_filter_time,
        );

        // Q43 and R170 leakage
        let punch = 0.7 + diode(10.0 * self.lp_out - 1.0);

        // Q43 / R165
        let attack_fm = self.fm_pulse_lp * 1.7 * b.attack_fm_amount;
        let self_fm = punch * 0.08 * b.self_fm_amount;
        let mut f = b.f0 * (1.0 + attack_fm + self_fm);
        f = f.clamp(0.0, 0.4);

        let mut resonator_out = 0.0;
        if sustain {
            self.oscillator.next_sin_cos(
                f,
                b.sustain_gain.next(),
                &mut resonator_out,
                &mut self.lp_out,
            );
        } else {
            self.resonator
                .set_f_q(f, 1.0 + b.q * f, FrequencyApproximation::Dirty);
            self.resonator.process_dual(
                (pulse - self.retrig_pulse * 0.2) * b.scale,
                &mut resonator_out,
                &mut self.lp_out,
                FilterMode::BandPass,
                FilterMode::LowPass,
            );
        }

        one_pole(
            &mut self.tone_lp,
            pulse * b.exciter_leak + resonator_out,
            b.tone_f,
        );

        self.tone_lp
    }

    /// [`Self::render`] after its loop: the sustain gain's interpolation is kept.
    #[inline]
    pub(crate) fn end(&mut self, b: &AnalogBassDrumBlock) {
        self.sustain_gain = b.sustain_gain.value();
    }
}

/// What every sample of one [`AnalogBassDrum::render`] block reads, and its sustain gain's
/// interpolation.
pub(crate) struct AnalogBassDrumBlock {
    sustain: bool,
    f0: f32,
    scale: f32,
    q: f32,
    tone_f: f32,
    exciter_leak: f32,
    attack_fm_amount: f32,
    self_fm_amount: f32,
    sustain_gain: DetachedInterpolator,
}

#[inline]
fn diode(mut x: f32) -> f32 {
    if x >= 0.0 {
        x
    } else {
        x *= 2.0;
        0.7 * x / (1.0 + x.abs())
    }
}
