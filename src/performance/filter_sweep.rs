//! Single knob DJ style filter sweep.
//!
//! `amount` is neutral at `0.5`: turning it down sweeps a lowpass from wide open to
//! `20 Hz`, turning it up sweeps a highpass from `20 Hz` up to the top of the audio band.
//! Both filters are wide open at the center, so the switch between them is inaudible.
//! The resonance is not part of the performance parameters and is set with
//! [`FilterSweep::set_resonance`].

use core::any::Any;

use super::{EffectParameters, PerformanceEffect, Transport};
use crate::utils::filter::{FilterMode, FrequencyApproximation, Svf};
use crate::utils::one_pole;
use crate::utils::units::semitones_to_ratio;

const MIN_FREQUENCY_HZ: f32 = 20.0;

/// Sweep range in semitones, `20 Hz` to about `18 kHz`.
const RANGE_SEMITONES: f32 = 117.7;

/// Time constant of the cutoff smoothing, in seconds.
const SMOOTHING_TIME: f32 = 0.01;

#[derive(Debug, Clone)]
pub struct FilterSweep {
    svf: Svf,
    sample_rate_hz: f32,
    pitch: f32,
    resonance: f32,
    high_pass: bool,
}

impl Default for FilterSweep {
    fn default() -> Self {
        Self {
            svf: Svf::new(),
            sample_rate_hz: 48000.0,
            pitch: RANGE_SEMITONES,
            resonance: 0.3,
            high_pass: false,
        }
    }
}

impl FilterSweep {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the resonance, `0.0` giving a clean sweep and `1.0` a whistling one.
    /// Range: `0.0` - `1.0`
    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 1.0);
    }

    pub fn resonance(&self) -> f32 {
        self.resonance
    }

    /// Returns the cutoff frequency in Hz.
    pub fn cutoff(&self) -> f32 {
        MIN_FREQUENCY_HZ * semitones_to_ratio(self.pitch)
    }

    /// Maps the performance control to a cutoff pitch above [`MIN_FREQUENCY_HZ`] and to
    /// the filter mode.
    fn target_pitch(amount: f32) -> (f32, bool) {
        let amount = amount.clamp(0.0, 1.0);

        if amount < 0.5 {
            (amount * 2.0 * RANGE_SEMITONES, false)
        } else {
            ((amount - 0.5) * 2.0 * RANGE_SEMITONES, true)
        }
    }
}

impl PerformanceEffect for FilterSweep {
    fn init(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = sample_rate_hz.max(1.0);
        self.svf.init();
        self.reset();
    }

    fn reset(&mut self) {
        self.svf.reset();
        self.pitch = RANGE_SEMITONES;
        self.high_pass = false;
    }

    fn engage(&mut self, parameters: &EffectParameters, _transport: &Transport) {
        // Jump straight to the requested cutoff instead of sweeping up to it from
        // wherever the previous press left off.
        let (pitch, high_pass) = Self::target_pitch(parameters.amount);
        self.pitch = pitch;
        self.high_pass = high_pass;
        self.svf.reset();
    }

    fn process(
        &mut self,
        parameters: &EffectParameters,
        _transport: &Transport,
        in_out: &mut [f32],
    ) {
        let size = in_out.len();
        let (target_pitch, high_pass) = Self::target_pitch(parameters.amount);

        if high_pass != self.high_pass {
            self.high_pass = high_pass;
            self.svf.reset();
        }

        let coefficient = (size as f32 / (SMOOTHING_TIME * self.sample_rate_hz)).clamp(0.0, 1.0);
        one_pole(&mut self.pitch, target_pitch, coefficient);

        let f = (MIN_FREQUENCY_HZ / self.sample_rate_hz * semitones_to_ratio(self.pitch))
            .clamp(1.0e-5, 0.45);
        let resonance = 0.7 + self.resonance * self.resonance * 11.3;

        self.svf.set_f_q(f, resonance, FrequencyApproximation::Fast);

        let mode = if self.high_pass {
            FilterMode::HighPass
        } else {
            FilterMode::LowPass
        };

        for sample in in_out.iter_mut() {
            *sample = self.svf.process(*sample, mode.clone());
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
