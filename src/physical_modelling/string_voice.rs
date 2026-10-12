//! Extended Karplus-Strong, with all the niceties from Rings.
//!
//! Grout change (engstrom/grout#1039): `render` returns early, touching nothing, for a string
//! that is neither excited nor tuned and holds no sound — not triggered, not sustained, no noise
//! burst left, `f0` exactly 0, its exciter filter's state and its last two output samples all
//! zero. Such a string renders exactly zero: its exciter filter turns zero into zero, and with
//! `f0` 0 the string's `src_ratio` is 0, so its per-sample loop never runs, nothing is drawn from
//! the PRNG, and `out` takes `crossfade(0, 0, src_phase)`. `out` and `aux` are only ever
//! accumulated into from a `+0.0` fill, so they never hold `-0.0`, and adding a zero of either
//! sign to them changes no bit. What the skip leaves undone is the sign of a zero in the exciter
//! filter's state and the string's damping filter's coefficients, which the next render that
//! uses them sets first: neither can reach a non-zero sample. `StringEngine::init` puts its
//! rotation where a rising edge excites string 0 and leaves strings 1 and 2 at `f0` 0, so a host
//! that re-initialises the engine at every note, as Grout does, renders one string of three.

// Based on MIT-licensed code (c) 2016 by Emilie Gillet (emilie.o.gillet@gmail.com)

use super::string::String;
use crate::noise::dust::dust;
use crate::utils::filter::{FilterMode, FrequencyApproximation, Svf};
use crate::utils::random;
use crate::utils::units::semitones_to_ratio;

#[derive(Debug, Clone)]
pub struct StringVoice {
    excitation_filter: Svf,
    string: String,
    remaining_noise_samples: usize,
}

impl Default for StringVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl StringVoice {
    pub fn new() -> Self {
        Self {
            excitation_filter: Svf::default(),
            string: String::new(),
            remaining_noise_samples: 0,
        }
    }

    pub fn init(&mut self, sample_rate_hz: f32) {
        self.excitation_filter.init();
        self.string.init(sample_rate_hz);
        self.remaining_noise_samples = 0;
    }

    pub fn reset(&mut self) {
        self.string.reset();
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn render(
        &mut self,
        sustain: bool,
        trigger: bool,
        accent: f32,
        f0: f32,
        structure: f32,
        mut brightness: f32,
        mut damping: f32,
        temp: &mut [f32],
        temp_2: &mut [f32],
        out: &mut [f32],
        aux: &mut [f32],
    ) {
        if !trigger
            && !sustain
            && f0 == 0.0
            && self.remaining_noise_samples == 0
            && self.excitation_filter.is_at_rest()
            && self.string.is_at_rest()
        {
            return;
        }

        let density = brightness * brightness;

        brightness += 0.25 * accent * (1.0 - brightness);
        damping += 0.25 * accent * (1.0 - damping);

        // Synthesize excitation signal.
        if trigger || sustain {
            let range = 72.0;
            let f = 4.0 * f0;
            let cutoff = f32::min(
                f * semitones_to_ratio((brightness * (2.0 - brightness) - 0.5) * range),
                0.499,
            );
            let q = if sustain { 1.0 } else { 0.5 };
            self.remaining_noise_samples = (1.0 / f0) as usize;
            self.excitation_filter
                .set_f_q(cutoff, q, FrequencyApproximation::Dirty);
        }

        if sustain {
            let dust_f = 0.00005 + 0.99995 * density * density;

            for sample_temp in temp.iter_mut() {
                *sample_temp = dust(dust_f) * (8.0 - dust_f * 6.0) * accent;
            }
        } else if self.remaining_noise_samples > 0 {
            let mut noise_samples = usize::min(self.remaining_noise_samples, out.len());
            self.remaining_noise_samples -= noise_samples;
            let mut tail = out.len() - noise_samples;
            let mut start_index = 0;
            while noise_samples > 0 {
                temp[start_index] = 2.0 * random::get_float() - 1.0;
                start_index += 1;
                noise_samples -= 1;
            }
            while tail > 0 {
                temp[start_index] = 0.0;
                start_index += 1;
                tail -= 1;
            }
        } else {
            for sample_temp in temp.iter_mut() {
                *sample_temp = 0.0;
            }
        }

        self.excitation_filter
            .process_buffer(temp, temp_2, FilterMode::LowPass);

        for (aux_sample, temp_sample) in aux.iter_mut().zip(temp_2.iter()) {
            *aux_sample += *temp_sample;
        }

        let non_linearity = if structure < 0.24 {
            (structure - 0.24) * 4.166
        } else if structure > 0.26 {
            (structure - 0.26) * 1.35135
        } else {
            0.0
        };
        self.string
            .process(f0, non_linearity, brightness, damping, temp_2, out);
    }
}
