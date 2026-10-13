//! Linear interpolation of parameters in rendering loops.
//!
//! Grout change (engstrom/grout#166): `ParameterInterpolator::detach` and
//! `DetachedInterpolator`, the same interpolation for a caller that keeps the state itself.

// Based on MIT-licensed code (c) 2015 by Olivier Gillet (ol.gillet@gmail.com)

/// Original implementation keeping a mutable reference to the interpolated value.
#[derive(Debug)]
pub struct ParameterInterpolator<'a> {
    state: &'a mut f32,
    value: f32,
    increment: f32,
}

impl<'a> ParameterInterpolator<'a> {
    pub fn new(state: &'a mut f32, new_value: f32, size: usize) -> Self {
        let v = *state;
        Self {
            state,
            value: v,
            increment: (new_value - v) / (size as f32),
        }
    }

    pub fn new_with_step(state: &'a mut f32, new_value: f32, step: f32) -> Self {
        let v = *state;
        Self {
            state,
            value: v,
            increment: (new_value - v) * step,
        }
    }

    pub fn init(&mut self, state: &'a mut f32, new_value: f32, size: usize) {
        let v = *state;
        self.state = state;
        self.value = v;
        self.increment = (new_value - v) / (size as f32);
    }

    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        self.value += self.increment;
        self.value
    }

    #[inline]
    pub fn subsample(&self, t: f32) -> f32 {
        self.value + self.increment * t
    }
}

impl ParameterInterpolator<'_> {
    /// The interpolation without the borrow of its state: the same value and increment, for a
    /// caller that writes the value back itself ([`DetachedInterpolator::value`]) rather than on
    /// drop. Writes the state back now, unchanged, as a drop before any `next` would.
    #[inline]
    pub fn detach(self) -> DetachedInterpolator {
        DetachedInterpolator {
            value: self.value,
            increment: self.increment,
        }
    }
}

/// A [`ParameterInterpolator`] whose state the caller keeps (Grout #166): `next` is the same
/// arithmetic, and the caller stores [`Self::value`] where the interpolator's drop would have.
#[derive(Debug, Clone, Copy)]
pub struct DetachedInterpolator {
    value: f32,
    increment: f32,
}

impl DetachedInterpolator {
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        self.value += self.increment;
        self.value
    }

    /// The value the interpolation has reached: what [`ParameterInterpolator`]'s drop stores.
    #[inline]
    pub fn value(&self) -> f32 {
        self.value
    }
}

impl Drop for ParameterInterpolator<'_> {
    fn drop(&mut self) {
        *self.state = self.value;
    }
}

/// Simplified version of the interpolator not keeping any references to avoid borrowing issues.
#[derive(Debug, Default, Copy, Clone)]
pub struct SimpleParameterInterpolator {
    increment: f32,
}

impl SimpleParameterInterpolator {
    pub fn new(value: f32, new_value: f32, size: usize) -> Self {
        Self {
            increment: (new_value - value) / (size as f32),
        }
    }

    pub fn init(&mut self, value: f32, new_value: f32, size: usize) {
        self.increment = (new_value - value) / (size as f32)
    }

    #[inline]
    pub fn update(&self, value: &mut f32) -> f32 {
        *value += self.increment;
        *value
    }

    #[inline]
    pub fn subsample(&self, value: f32, t: f32) -> f32 {
        value + self.increment * t
    }
}
