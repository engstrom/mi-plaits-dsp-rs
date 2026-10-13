//! Distortion/overdrive.
//!
//! Grout change (engstrom/grout#166): `process` is `begin`, `OverdriveBlock::step` a sample and
//! `end`, so `BassDrumEngine` can run it a sample at a time; the same arithmetic in the same order.

// Based on MIT-licensed code (c) 2014 by Emilie Gillet (emilie.o.gillet@gmail.com)

use crate::utils::parameter_interpolator::{DetachedInterpolator, ParameterInterpolator};
use crate::utils::soft_clip;

#[derive(Debug, Default, Clone)]
pub struct Overdrive {
    pre_gain: f32,
    post_gain: f32,
}

impl Overdrive {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.pre_gain = 0.0;
        self.post_gain = 0.0;
    }

    #[inline]
    pub fn process(&mut self, drive: f32, in_out: &mut [f32]) {
        let mut block = self.begin(drive, in_out.len());
        for in_out_sample in in_out.iter_mut() {
            *in_out_sample = block.step(*in_out_sample);
        }
        self.end(&block);
    }

    /// [`Self::process`] before its loop; [`OverdriveBlock::step`] is one sample of it and
    /// [`Self::end`] what follows it (Grout #166).
    #[inline]
    pub(crate) fn begin(&mut self, drive: f32, size: usize) -> OverdriveBlock {
        let drive_2 = drive * drive;
        let pre_gain_a = drive * 0.5;
        let pre_gain_b = drive_2 * drive_2 * drive * 24.0;
        let pre_gain = pre_gain_a + (pre_gain_b - pre_gain_a) * drive_2;
        let drive_squashed = drive * (2.0 - drive);
        let post_gain = 1.0 / soft_clip(0.33 + drive_squashed * (pre_gain - 0.33));

        let pre_gain_modulation =
            ParameterInterpolator::new(&mut self.pre_gain, pre_gain, size).detach();

        let post_gain_modulation =
            ParameterInterpolator::new(&mut self.post_gain, post_gain, size).detach();

        OverdriveBlock {
            pre_gain_modulation,
            post_gain_modulation,
        }
    }

    /// [`Self::process`] after its loop: the two interpolations are kept.
    #[inline]
    pub(crate) fn end(&mut self, b: &OverdriveBlock) {
        self.pre_gain = b.pre_gain_modulation.value();
        self.post_gain = b.post_gain_modulation.value();
    }
}

/// The two gains' interpolations over one [`Overdrive::process`] block.
pub(crate) struct OverdriveBlock {
    pre_gain_modulation: DetachedInterpolator,
    post_gain_modulation: DetachedInterpolator,
}

impl OverdriveBlock {
    /// One sample of [`Overdrive::process`]'s loop.
    #[inline(always)]
    pub(crate) fn step(&mut self, x: f32) -> f32 {
        let pre = self.pre_gain_modulation.next() * x;
        soft_clip(pre) * self.post_gain_modulation.next()
    }
}
