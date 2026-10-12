//! Resonator, taken from Rings' code but with fixed position.
//!
//! Grout change (engstrom/grout#1039): `Resonator::process` computes every mode's coefficients
//! first and then renders the batches of modes three to a per-sample loop
//! (`process_bandpass_group`), where the original rendered one batch at a time, each its own
//! pass over the block. Each mode's recurrence waits on its last sample, so one batch at a time
//! leaves the core waiting on a dependency chain; three in one loop let it overlap them. The
//! float operations and their order are the original's — each batch's sum from `0.0` in mode
//! order, then the batches' sums added to `out` in batch order — so not one sample changes.

// Based on MIT-licensed code (c) 2016 by Emilie Gillet (emilie.o.gillet@gmail.com)

use crate::resources::stiffness::LUT_STIFFNESS;
use crate::utils::cosine_oscillator::{CosineOscillator, CosineOscillatorMode};
use crate::utils::filter::{FilterMode, FrequencyApproximation, OnePole};
use crate::utils::interpolate;
use crate::utils::units::semitones_to_ratio;

pub const MAX_NUM_MODES: usize = 24;
pub const MODE_BATCH_SIZE: usize = 4;

const MODE_FILTERS_LENGTH: usize = MAX_NUM_MODES / MODE_BATCH_SIZE;

#[derive(Debug, Default, Clone)]
pub struct Resonator {
    resolution: usize,
    mode_amplitude: [f32; MAX_NUM_MODES],
    mode_filters: [ResonatorSvf<MODE_BATCH_SIZE>; MODE_FILTERS_LENGTH],
}

impl Resonator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, position: f32, resolution: usize) {
        self.resolution = usize::min(resolution, MAX_NUM_MODES);

        let mut amplitudes = CosineOscillator::new();
        amplitudes.init(position, CosineOscillatorMode::Approximate);

        for i in 0..resolution {
            self.mode_amplitude[i] = amplitudes.next() * 0.25;
        }

        for i in 0..(MAX_NUM_MODES / MODE_BATCH_SIZE) {
            self.mode_filters[i].init();
        }
    }

    #[inline]
    pub fn process(
        &mut self,
        mut f0: f32,
        structure: f32,
        mut brightness: f32,
        damping: f32,
        in_: &[f32],
        out: &mut [f32],
    ) {
        let mut stiffness = interpolate(&LUT_STIFFNESS, structure, 64.0);
        f0 *= nth_harmonic_compensation(3, stiffness);

        let mut harmonic = f0;
        let mut stretch_factor = 1.0;
        let q_sqrt = semitones_to_ratio(damping * 79.7);
        let mut q = 500.0 * q_sqrt * q_sqrt;
        brightness *= 1.0 - structure * 0.3;
        brightness *= 1.0 - damping * 0.3;
        let q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;

        let mut mode_q = [[0.0; MODE_BATCH_SIZE]; MODE_FILTERS_LENGTH];
        let mut mode_f = [[0.0; MODE_BATCH_SIZE]; MODE_FILTERS_LENGTH];
        let mut mode_a = [[0.0; MODE_BATCH_SIZE]; MODE_FILTERS_LENGTH];

        // Every mode's frequency, Q and gain first, in the order and arithmetic the per-batch
        // version used (none of it reads a filter's state). A mode past the last whole batch is
        // computed and dropped, as it was.
        for i in 0..self.resolution {
            let mut mode_frequency = harmonic * stretch_factor;
            if mode_frequency >= 0.499 {
                mode_frequency = 0.499;
            }
            let mode_attenuation = 1.0 - mode_frequency * 2.0;

            let (batch, slot) = (i / MODE_BATCH_SIZE, i % MODE_BATCH_SIZE);
            mode_f[batch][slot] = mode_frequency;
            mode_q[batch][slot] = 1.0 + mode_frequency * q;
            mode_a[batch][slot] = self.mode_amplitude[i] * mode_attenuation;

            stretch_factor += stiffness;

            if stiffness < 0.0 {
                // Make sure that the partials do not fold back into negative
                // frequencies.
                stiffness *= 0.93;
            } else {
                // This helps adding a few extra partials in the highest
                // frequencies.
                stiffness *= 0.98;
            }

            harmonic += f0;
            q *= q_loss;
        }

        // Then the batches, `MODES_GROUP` to a per-sample loop (Grout #1039).
        let batches = self.resolution / MODE_BATCH_SIZE;
        let mut b = 0;
        while b + MODES_GROUP <= batches {
            process_bandpass_group::<MODES_GROUP>(
                &mut self.mode_filters[b..b + MODES_GROUP],
                &mode_f[b..b + MODES_GROUP],
                &mode_q[b..b + MODES_GROUP],
                &mode_a[b..b + MODES_GROUP],
                in_,
                out,
            );
            b += MODES_GROUP;
        }
        while b < batches {
            process_bandpass_group::<1>(
                &mut self.mode_filters[b..b + 1],
                &mode_f[b..b + 1],
                &mode_q[b..b + 1],
                &mode_a[b..b + 1],
                in_,
                out,
            );
            b += 1;
        }
    }
}

/// How many batches of modes share one per-sample loop. Three was the fastest on the desktop
/// that measured it (#1039); six spilled there.
const MODES_GROUP: usize = 3;

/// `N` batches of modes as band-passes added to `out`: the same samples as `N` calls of
/// `ResonatorSvf::process(.., FilterMode::BandPass, true)` one after another. Each batch keeps
/// its own chain of operations and its own `s_out`, summed from `0.0` in mode order, and each
/// output sample takes the batches' sums in batch order — the order the separate calls added
/// them — so every float operation is the one it was. What changes is only the schedule: the
/// batches' recurrences are independent, so in one loop the core overlaps them instead of
/// waiting out each one's dependency chain in turn, and the state is stored once a call rather
/// than once a sample.
#[inline]
fn process_bandpass_group<const N: usize>(
    filters: &mut [ResonatorSvf<MODE_BATCH_SIZE>],
    f: &[[f32; MODE_BATCH_SIZE]],
    q: &[[f32; MODE_BATCH_SIZE]],
    gain: &[[f32; MODE_BATCH_SIZE]],
    in_: &[f32],
    out: &mut [f32],
) {
    let mut g = [[0.0f32; MODE_BATCH_SIZE]; N];
    let mut r_plus_g = [[0.0f32; MODE_BATCH_SIZE]; N];
    let mut h = [[0.0f32; MODE_BATCH_SIZE]; N];
    let mut state_1 = [[0.0f32; MODE_BATCH_SIZE]; N];
    let mut state_2 = [[0.0f32; MODE_BATCH_SIZE]; N];
    let mut gains = [[0.0f32; MODE_BATCH_SIZE]; N];

    for n in 0..N {
        for i in 0..MODE_BATCH_SIZE {
            g[n][i] = OnePole::tan(f[n][i], FrequencyApproximation::Fast);
            let r = 1.0 / q[n][i];
            h[n][i] = 1.0 / (1.0 + r * g[n][i] + g[n][i] * g[n][i]);
            r_plus_g[n][i] = r + g[n][i];
            state_1[n][i] = filters[n].state_1[i];
            state_2[n][i] = filters[n].state_2[i];
            gains[n][i] = gain[n][i];
        }
    }

    for (in_sample, out_sample) in in_.iter().zip(out.iter_mut()) {
        let s_in = *in_sample;
        let mut s_out = [0.0f32; N];

        for n in 0..N {
            for i in 0..MODE_BATCH_SIZE {
                let hp = (s_in - r_plus_g[n][i] * state_1[n][i] - state_2[n][i]) * h[n][i];
                let bp = g[n][i] * hp + state_1[n][i];
                state_1[n][i] = g[n][i] * hp + bp;
                let lp = g[n][i] * bp + state_2[n][i];
                state_2[n][i] = g[n][i] * bp + lp;
                s_out[n] += gains[n][i] * bp;
            }
        }

        for s in s_out {
            *out_sample += s;
        }
    }

    for n in 0..N {
        filters[n].state_1 = state_1[n];
        filters[n].state_2 = state_2[n];
    }
}

#[derive(Debug, Clone)]
pub struct ResonatorSvf<const BATCH_SIZE: usize> {
    state_1: [f32; BATCH_SIZE],
    state_2: [f32; BATCH_SIZE],
}

impl<const BATCH_SIZE: usize> Default for ResonatorSvf<BATCH_SIZE> {
    fn default() -> Self {
        Self {
            state_1: [0.0; BATCH_SIZE],
            state_2: [0.0; BATCH_SIZE],
        }
    }
}

impl<const BATCH_SIZE: usize> ResonatorSvf<BATCH_SIZE> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        for elem in self.state_1.iter_mut() {
            *elem = 0.0;
        }
        for elem in self.state_2.iter_mut() {
            *elem = 0.0;
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn process(
        &mut self,
        f: &[f32],
        q: &[f32],
        gain: &[f32],
        in_: &[f32],
        out: &mut [f32],
        mode: FilterMode,
        add: bool,
    ) {
        let mut g: [f32; BATCH_SIZE] = [0.0; BATCH_SIZE];
        let mut r: [f32; BATCH_SIZE] = [0.0; BATCH_SIZE];
        let mut r_plus_g: [f32; BATCH_SIZE] = [0.0; BATCH_SIZE];
        let mut h: [f32; BATCH_SIZE] = [0.0; BATCH_SIZE];
        let mut state_1: [f32; BATCH_SIZE] = [0.0; BATCH_SIZE];
        let mut state_2: [f32; BATCH_SIZE] = [0.0; BATCH_SIZE];
        let mut gains: [f32; BATCH_SIZE] = [0.0; BATCH_SIZE];

        for i in 0..BATCH_SIZE {
            g[i] = OnePole::tan(f[i], FrequencyApproximation::Fast);
            r[i] = 1.0 / q[i];
            h[i] = 1.0 / (1.0 + r[i] * g[i] + g[i] * g[i]);
            r_plus_g[i] = r[i] + g[i];
            state_1[i] = self.state_1[i];
            state_2[i] = self.state_2[i];
            gains[i] = gain[i];
        }

        for (in_sample, out_sample) in in_.iter().zip(out.iter_mut()) {
            let s_in = *in_sample;
            let mut s_out = 0.0;

            for i in 0..BATCH_SIZE {
                let hp = (s_in - r_plus_g[i] * state_1[i] - state_2[i]) * h[i];
                let bp = g[i] * hp + state_1[i];
                state_1[i] = g[i] * hp + bp;
                let lp = g[i] * bp + state_2[i];
                state_2[i] = g[i] * bp + lp;
                s_out += gains[i]
                    * (if matches!(mode, FilterMode::LowPass) {
                        lp
                    } else {
                        bp
                    });
            }

            if add {
                *out_sample += s_out;
            } else {
                *out_sample = s_out;
            }

            self.state_1[..BATCH_SIZE].copy_from_slice(&state_1[..BATCH_SIZE]);
            self.state_2[..BATCH_SIZE].copy_from_slice(&state_2[..BATCH_SIZE])
        }
    }
}

#[inline]
fn nth_harmonic_compensation(n: usize, mut stiffness: f32) -> f32 {
    let mut stretch_factor = 1.0;

    for _ in 0..(n - 1) {
        stretch_factor += stiffness;
        if stiffness < 0.0 {
            stiffness *= 0.93;
        } else {
            stiffness *= 0.98;
        }
    }

    1.0 / stretch_factor
}
