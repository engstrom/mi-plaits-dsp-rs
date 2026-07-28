//! Bit crusher and decimator.
//!
//! `amount` reduces the word length from 16 bits down to about 2 bits while at the same
//! time decimating the signal down to a few hundred Hz. Unlike
//! [`sample_rate_reducer`](crate::fx::sample_rate_reducer), no anti-aliasing is applied:
//! the aliasing is the point of this effect.

use core::any::Any;

use super::{EffectParameters, PerformanceEffect, Transport};
use crate::utils::units::semitones_to_ratio_safe;

#[allow(unused_imports)]
use num_traits::float::Float;

/// Lowest rate the decimator reaches, in Hz.
const MIN_RATE_HZ: f32 = 480.0;

/// Word length at `amount = 0.0`, in bits.
const MAX_BITS: f32 = 16.0;

/// Word length at `amount = 1.0`, in bits.
const MIN_BITS: f32 = 2.0;

#[derive(Debug, Clone)]
pub struct Crush {
    sample_rate_hz: f32,
    range_octaves: f32,
    phase: f32,
    hold: f32,
}

impl Default for Crush {
    fn default() -> Self {
        Self {
            sample_rate_hz: 48000.0,
            range_octaves: 0.0,
            phase: 1.0,
            hold: 0.0,
        }
    }
}

impl Crush {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PerformanceEffect for Crush {
    fn init(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = sample_rate_hz.max(MIN_RATE_HZ);
        self.range_octaves = (self.sample_rate_hz / MIN_RATE_HZ).log2();
        self.reset();
    }

    fn reset(&mut self) {
        self.phase = 1.0;
        self.hold = 0.0;
    }

    fn process(
        &mut self,
        parameters: &EffectParameters,
        _transport: &Transport,
        in_out: &mut [f32],
    ) {
        let amount = parameters.amount.clamp(0.0, 1.0);

        // Decimation rate as a fraction of the sample rate, mapped exponentially.
        let rate = semitones_to_ratio_safe(-amount * self.range_octaves * 12.0);

        // Quantization step of the word length.
        let bits = MAX_BITS - amount * (MAX_BITS - MIN_BITS);
        let levels = semitones_to_ratio_safe(bits * 12.0) * 0.5;
        let step = 1.0 / levels;

        for sample in in_out.iter_mut() {
            self.phase += rate;

            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.hold = (*sample * levels).round() * step;
            }

            *sample = self.hold;
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
