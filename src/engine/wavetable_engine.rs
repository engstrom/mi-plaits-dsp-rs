//! Wavetable oscillator.
//!
//! Four banks of 8x8 waveforms, accessed by row and column, with or without interpolation.
//!
//! Banks:
//! - Bank A: harmonically poor waveforms obtained by additive synthesis
//!   (sine harmonics, drawbar organ waveforms).
//! - Bank B: harmonically rich waveforms obtained by formant synthesis or waveshaping.
//! - Bank C: wavetables from the Shruthi-1 / Ambika, sampled from classic wavetable
//!   or ROM playback synths.
//! - Bank D: a joyous semi-random permutation of waveforms from the other 3 banks.
//!
//! Engine parameters:
//! - *HARMONICS:* bank selection. 4 interpolated banks followed by the same 4 banks,
//!   in reverse order, without interpolation.
//! - *TIMBRE:* row index. Within a row, the waves are sorted by spectral brightness
//!   (except for bank D which is a mess!).
//! - *MORPH:* column index.
//!
//! *AUX* signal: low-fi (5-bit) output.

// Based on MIT-licensed code (c) 2016 by Emilie Gillet (emilie.o.gillet@gmail.com)

use alloc::boxed::Box;
use alloc::sync::Arc;

use super::{note_to_frequency, Engine, EngineParameters};
use crate::oscillator::wavetable_oscillator::{interpolate_wave_hermite, Differentiator};
use crate::resources::waves::WAV_INTEGRATED_WAVES;
use crate::utils::one_pole;
use crate::utils::parameter_interpolator::SimpleParameterInterpolator;

const TABLE_SIZE: usize = 128;
const TABLE_SIZE_F: f32 = TABLE_SIZE as f32;

/// #306 user-bank pool size: 4 independent banks × 64 waves × (`TABLE_SIZE` + 4
/// Hermite guard) i16. The stock 192-wave pool only has 3 independent banks
/// (bank D is a permutation), so a distinct 4-bank BLEND needs its own pool.
pub const USER_POOL_LEN: usize = 4 * 64 * (TABLE_SIZE + 4);

#[derive(Debug, Clone)]
pub struct WavetableEngine<'a> {
    phase: f32,

    x_pre_lp: f32,
    y_pre_lp: f32,
    z_pre_lp: f32,

    x_lp: f32,
    y_lp: f32,
    z_lp: f32,

    previous_x: f32,
    previous_y: f32,
    previous_z: f32,
    previous_f0: f32,

    diff_out: Differentiator,

    wavetables: &'a [i16; 25344],

    // #306: optional shared four-bank user pool. `Some` → `read_wave` reads four
    // independent 64-wave banks from here (no permutation, no ROM `% 192`) so
    // BLEND (HARMONICS/z) crossfades four DISTINCT user tables. `None` → stock.
    // Held behind an `Arc` so a caller that prebuilds the pool off the audio
    // thread can install it with a refcount move instead of copying it.
    user_pool: Option<Arc<[i16; USER_POOL_LEN]>>,
}

impl Default for WavetableEngine<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> WavetableEngine<'a> {
    pub fn new() -> Self {
        Self {
            phase: 0.0,

            x_pre_lp: 0.0,
            y_pre_lp: 0.0,
            z_pre_lp: 0.0,

            x_lp: 0.0,
            y_lp: 0.0,
            z_lp: 0.0,

            previous_x: 0.0,
            previous_y: 0.0,
            previous_z: 0.0,
            previous_f0: 0.0,

            diff_out: Differentiator::new(),

            wavetables: &WAV_INTEGRATED_WAVES,

            user_pool: None,
        }
    }

    pub fn set_wavetables(&mut self, wavetables: &'a [i16; 25344]) {
        self.wavetables = wavetables;
    }

    /// #306: load four independent user banks. `pool` is `[bank * 64 + cell]`,
    /// each entry a `TABLE_SIZE + 4` i16 wave (the caller pre-resolves any ROM /
    /// custom wave-map into flat data). While set, HARMONICS crossfades the four
    /// banks; `clear_user_banks` reverts to the stock pool.
    ///
    /// Converting the `Box` into the shared representation allocates once. Callers
    /// on a real-time thread should prebuild an [`Arc`] elsewhere and install it
    /// with [`WavetableEngine::set_user_banks_shared`] instead.
    pub fn set_user_banks(&mut self, pool: Box<[i16; USER_POOL_LEN]>) {
        self.user_pool = Some(Arc::from(pool));
    }

    /// Same as [`WavetableEngine::set_user_banks`], but takes an already shared
    /// pool and stores it as-is: no allocation, no copy of the wave data, just a
    /// refcount move. This makes bank swaps safe to perform on an audio thread
    /// when the pool was built ahead of time, and lets several engines play from
    /// one pool (it is only ever read).
    pub fn set_user_banks_shared(&mut self, pool: Arc<[i16; USER_POOL_LEN]>) {
        self.user_pool = Some(pool);
    }

    pub fn clear_user_banks(&mut self) {
        self.user_pool = None;
    }

    #[inline]
    fn read_wave(
        &self,
        x: usize,
        y: usize,
        z: usize,
        randomize: usize,
        phase_integral: usize,
        phase_fractional: f32,
    ) -> f32 {
        let (table, wave): (&[i16], usize) = match self.user_pool {
            // Four independent banks: `z` (already folded to 0..=3 by the
            // caller) selects the bank directly — no `randomize` / `% 192`
            // permutation, so each bank is its own distinct table.
            Some(ref pool) => (&pool[..], z * 64 + x + y * 8),
            None => (
                &self.wavetables[..],
                ((x + y * 8 + z * 64) * randomize) % 192,
            ),
        };
        interpolate_wave_hermite(
            &table[wave * (TABLE_SIZE + 4)..],
            phase_integral,
            phase_fractional,
        )
    }
}

impl Engine for WavetableEngine<'_> {
    fn init(&mut self, _sample_rate_hz: f32) {
        self.phase = 0.0;

        self.x_lp = 0.0;
        self.y_lp = 0.0;
        self.z_lp = 0.0;

        self.x_pre_lp = 0.0;
        self.y_pre_lp = 0.0;
        self.z_pre_lp = 0.0;

        self.previous_x = 0.0;
        self.previous_y = 0.0;
        self.previous_z = 0.0;
        self.previous_f0 = 55.0 / _sample_rate_hz; // A0 normalized

        self.diff_out.init();
    }

    /// Both smoothing stages land on their fixed point for these knobs: the
    /// per-call pre-filters on the scaled knobs, and the per-block
    /// interpolators plus per-sample one-poles on the quantized targets
    /// `render` would derive from them. Pitch lands too, so a fresh engine
    /// doesn't chirp up from A0 over its first block. Phase and the
    /// differentiator are left alone: they're the oscillator, not smoothing.
    fn settle(&mut self, parameters: &EngineParameters) {
        self.x_pre_lp = parameters.timbre * 6.9999;
        self.y_pre_lp = parameters.morph * 6.9999;
        self.z_pre_lp = parameters.harmonics * 6.9999;

        let (_, [x, y, z]) = quantized_targets([self.x_pre_lp, self.y_pre_lp, self.z_pre_lp]);
        self.previous_x = x;
        self.previous_y = y;
        self.previous_z = z;
        self.x_lp = x;
        self.y_lp = y;
        self.z_lp = z;

        self.previous_f0 = note_to_frequency(parameters.note, parameters.a0_normalized);
    }

    #[inline]
    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        _already_enveloped: &mut bool,
    ) {
        let f0 = note_to_frequency(parameters.note, parameters.a0_normalized);

        one_pole(&mut self.x_pre_lp, parameters.timbre * 6.9999, 0.2);
        one_pole(&mut self.y_pre_lp, parameters.morph * 6.9999, 0.2);
        one_pole(&mut self.z_pre_lp, parameters.harmonics * 6.9999, 0.05);

        let (quantization, [x_target, y_target, z_target]) =
            quantized_targets([self.x_pre_lp, self.y_pre_lp, self.z_pre_lp]);
        let lp_coefficient = (2.0 * f0 * (4.0 - 3.0 * quantization)).clamp(0.01, 0.1);

        let x_modulation = SimpleParameterInterpolator::new(self.previous_x, x_target, out.len());
        let y_modulation = SimpleParameterInterpolator::new(self.previous_y, y_target, out.len());
        let z_modulation = SimpleParameterInterpolator::new(self.previous_z, z_target, out.len());

        let f0_modulation = SimpleParameterInterpolator::new(self.previous_f0, f0, out.len());

        for (out_sample, aux_sample) in out.iter_mut().zip(aux.iter_mut()) {
            let f0 = f0_modulation.update(&mut self.previous_f0);

            let gain = (1.0 / (f0 * 131072.0)) * (0.95 - f0);
            let cutoff = f32::min(TABLE_SIZE_F * f0, 1.0);

            one_pole(
                &mut self.x_lp,
                x_modulation.update(&mut self.previous_x),
                lp_coefficient,
            );
            one_pole(
                &mut self.y_lp,
                y_modulation.update(&mut self.previous_y),
                lp_coefficient,
            );
            one_pole(
                &mut self.z_lp,
                z_modulation.update(&mut self.previous_z),
                lp_coefficient,
            );

            let x = self.x_lp;
            let y = self.y_lp;
            let z = self.z_lp;

            let x_integral = x as usize;
            let x_fractional = x - (x_integral as f32);
            let y_integral = y as usize;
            let y_fractional = y - (y_integral as f32);
            let z_integral = z as usize;
            let z_fractional = z - (z_integral as f32);

            self.phase += f0;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }

            let p = self.phase * TABLE_SIZE_F;
            let p_integral = p as usize;
            let p_fractional = p - (p_integral as f32);

            {
                let x0 = x_integral;
                let x1 = x_integral + 1;
                let y0 = y_integral;
                let y1 = y_integral + 1;
                let mut z0 = z_integral;
                let mut z1 = z_integral + 1;

                if z0 >= 4 {
                    z0 = 7 - z0;
                }
                if z1 >= 4 {
                    z1 = 7 - z1;
                }

                let r0 = if z0 == 3 { 101 } else { 1 };
                let r1 = if z1 == 3 { 101 } else { 1 };

                let x0y0z0 = self.read_wave(x0, y0, z0, r0, p_integral, p_fractional);
                let x1y0z0 = self.read_wave(x1, y0, z0, r0, p_integral, p_fractional);
                let xy0z0 = x0y0z0 + (x1y0z0 - x0y0z0) * x_fractional;

                let x0y1z0 = self.read_wave(x0, y1, z0, r0, p_integral, p_fractional);
                let x1y1z0 = self.read_wave(x1, y1, z0, r0, p_integral, p_fractional);
                let xy1z0 = x0y1z0 + (x1y1z0 - x0y1z0) * x_fractional;

                let xyz0 = xy0z0 + (xy1z0 - xy0z0) * y_fractional;

                let x0y0z1 = self.read_wave(x0, y0, z1, r1, p_integral, p_fractional);
                let x1y0z1 = self.read_wave(x1, y0, z1, r1, p_integral, p_fractional);
                let xy0z1 = x0y0z1 + (x1y0z1 - x0y0z1) * x_fractional;

                let x0y1z1 = self.read_wave(x0, y1, z1, r1, p_integral, p_fractional);
                let x1y1z1 = self.read_wave(x1, y1, z1, r1, p_integral, p_fractional);
                let xy1z1 = x0y1z1 + (x1y1z1 - x0y1z1) * x_fractional;

                let xyz1 = xy0z1 + (xy1z1 - xy0z1) * y_fractional;

                let mut mix = xyz0 + (xyz1 - xyz0) * z_fractional;
                mix = self.diff_out.process(cutoff, mix) * gain;
                *out_sample = mix;
                *aux_sample = (((mix * 32.0) as i32) as f32) / 32.0;
            }
        }
    }
}

/// The per-sample stage's targets for the pre-filtered (x, y, z): each split
/// into cell + fraction, with the fraction pushed towards the cell edges as
/// HARMONICS enters the quantized (non-interpolated) half of its range.
/// Returns the quantization amount alongside, since the per-sample
/// coefficient depends on it.
#[inline]
fn quantized_targets(pre: [f32; 3]) -> (f32, [f32; 3]) {
    let quantization = (pre[2] - 3.0).clamp(0.0, 1.0);
    let targets = pre.map(|v| {
        let integral = v as usize;
        let mut fractional = v - (integral as f32);
        fractional += quantization * (clamp(fractional, 16.0) - fractional);
        integral as f32 + fractional
    });
    (quantization, targets)
}

#[inline]
fn clamp(mut x: f32, amount: f32) -> f32 {
    x -= 0.5;
    x *= amount;
    x = x.clamp(-0.5, 0.5);
    x += 0.5;

    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TriggerState;

    const BLOCK: usize = 24;

    fn params(note: f32, timbre: f32, morph: f32, harmonics: f32) -> EngineParameters {
        EngineParameters {
            trigger: TriggerState::High,
            note,
            timbre,
            morph,
            harmonics,
            accent: 1.0,
            a0_normalized: 55.0 / 48_000.0,
        }
    }

    fn smoothing(e: &WavetableEngine) -> [f32; 10] {
        [
            e.x_pre_lp,
            e.y_pre_lp,
            e.z_pre_lp,
            e.previous_x,
            e.previous_y,
            e.previous_z,
            e.x_lp,
            e.y_lp,
            e.z_lp,
            e.previous_f0,
        ]
    }

    fn render(e: &mut WavetableEngine, p: &EngineParameters) {
        let (mut out, mut aux) = ([0.0; BLOCK], [0.0; BLOCK]);
        e.render(p, &mut out, &mut aux, &mut false);
    }

    // The interpolated and the quantized half of HARMONICS, and a low note
    // where the per-sample coefficient sits on its 0.01 floor.
    const CASES: [(f32, f32, f32, f32); 3] = [
        (52.0, 0.44, 0.43, 0.31),
        (28.0, 0.9, 0.1, 0.2),
        (64.0, 0.25, 0.75, 0.8),
    ];

    #[test]
    fn settle_lands_on_a_fixed_point_of_render() {
        for (note, t, m, h) in CASES {
            let p = params(note, t, m, h);
            let mut e = WavetableEngine::new();
            e.init(48_000.0);
            e.settle(&p);
            let settled = smoothing(&e);
            render(&mut e, &p);
            assert_eq!(smoothing(&e), settled, "one render moved a settled engine");
        }
    }

    #[test]
    fn settle_matches_a_long_run_at_the_same_knobs() {
        for (note, t, m, h) in CASES {
            let p = params(note, t, m, h);
            let mut run = WavetableEngine::new();
            run.init(48_000.0);
            for _ in 0..4_000 {
                render(&mut run, &p);
            }
            let mut snap = WavetableEngine::new();
            snap.init(48_000.0);
            snap.settle(&p);
            for (a, b) in smoothing(&run).iter().zip(smoothing(&snap).iter()) {
                assert!((a - b).abs() < 1e-4, "long run {a} vs settle {b}");
            }
        }
    }
}
