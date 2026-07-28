//! Tempo synced feedback delay.
//!
//! The delay time follows the selected [`Division`](super::Division) and is smoothed, so
//! changing the division while the pad is held down bends the pitch of the echoes like a
//! tape delay. The feedback path is damped at both ends, which turns high feedback
//! settings into dub style repeats instead of a build-up of noise.

use core::any::Any;

use super::buffer::RecordBuffer;
use super::{EffectParameters, PerformanceEffect, Transport, DEFAULT_BUFFER_LENGTH};
use crate::utils::one_pole;
use crate::utils::parameter_interpolator::ParameterInterpolator;

#[allow(unused_imports)]
use num_traits::float::Float;

/// Time constant of the delay time smoothing, in seconds.
const SMOOTHING_TIME: f32 = 0.05;

/// Cutoff of the lowpass in the feedback path, in Hz.
const DAMPING_HZ: f32 = 6000.0;

/// Cutoff of the highpass in the feedback path, in Hz.
const HIGH_PASS_HZ: f32 = 120.0;

/// Level below which the tail is considered to have died out.
const SILENCE: f32 = 1.0e-4;

#[derive(Debug, Clone)]
pub struct Delay {
    buffer: RecordBuffer,
    buffer_length: f32,
    sample_rate_hz: f32,
    delay_samples: f32,
    lp_state: f32,
    hp_state: f32,
    lp_coefficient: f32,
    hp_coefficient: f32,
    energy: f32,
}

impl Default for Delay {
    fn default() -> Self {
        Self {
            buffer: RecordBuffer::new(),
            buffer_length: DEFAULT_BUFFER_LENGTH,
            sample_rate_hz: 48000.0,
            delay_samples: 0.0,
            lp_state: 0.0,
            hp_state: 0.0,
            lp_coefficient: 1.0,
            hp_coefficient: 0.0,
            energy: 0.0,
        }
    }
}

impl Delay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an instance with a maximum delay time of `length` seconds.
    pub fn with_length(length: f32) -> Self {
        Self {
            buffer_length: length.max(0.05),
            ..Self::default()
        }
    }

    /// Returns the current delay time in samples.
    pub fn delay_samples(&self) -> f32 {
        self.delay_samples
    }
}

impl PerformanceEffect for Delay {
    fn init(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = sample_rate_hz.max(1.0);
        let length = (self.buffer_length * self.sample_rate_hz) as usize;
        self.buffer.init(length.max(8));
        self.lp_coefficient =
            (2.0 * core::f32::consts::PI * DAMPING_HZ / self.sample_rate_hz).clamp(0.0, 1.0);
        self.hp_coefficient =
            (2.0 * core::f32::consts::PI * HIGH_PASS_HZ / self.sample_rate_hz).clamp(0.0, 1.0);
        self.reset();
    }

    fn reset(&mut self) {
        self.buffer.reset();
        self.delay_samples = 0.0;
        self.lp_state = 0.0;
        self.hp_state = 0.0;
        self.energy = 0.0;
    }

    fn is_active(&self) -> bool {
        self.energy > SILENCE
    }

    fn process(
        &mut self,
        parameters: &EffectParameters,
        transport: &Transport,
        in_out: &mut [f32],
    ) {
        let size = in_out.len();

        if self.buffer.is_empty() || size == 0 {
            return;
        }

        let max_delay = (self.buffer.len() - 4) as f32;
        let target = (transport.samples_for(parameters.division) as f32).clamp(4.0, max_delay);

        if self.delay_samples == 0.0 {
            self.delay_samples = target;
        }

        let coefficient = (size as f32 / (SMOOTHING_TIME * self.sample_rate_hz)).clamp(0.0, 1.0);
        let mut smoothed = self.delay_samples;
        one_pole(&mut smoothed, target, coefficient);

        let feedback = parameters.amount.clamp(0.0, 1.0) * 0.95;

        let mut delay_modulation =
            ParameterInterpolator::new(&mut self.delay_samples, smoothed, size);

        let mut energy = self.energy;

        for sample in in_out.iter_mut() {
            let echo = self.buffer.read_frac(delay_modulation.next());

            one_pole(&mut self.lp_state, echo, self.lp_coefficient);
            let damped = self.lp_state;
            one_pole(&mut self.hp_state, damped, self.hp_coefficient);

            self.buffer
                .write(*sample + (damped - self.hp_state) * feedback);

            let level = echo.abs();
            energy = if level > energy {
                level
            } else {
                energy * 0.9999
            };

            *sample += echo;
        }

        self.energy = energy;
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
