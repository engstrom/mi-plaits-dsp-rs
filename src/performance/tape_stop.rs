//! Tape stop.
//!
//! While the pad is held down, playback slows to a halt over the length of the selected
//! [`Division`](super::Division). Releasing the pad spins the tape back up, running
//! slightly fast until the playback position has caught up with the input again.

use core::any::Any;

use super::buffer::RecordBuffer;
use super::{EffectParameters, PerformanceEffect, Transport, DEFAULT_BUFFER_LENGTH};

#[allow(unused_imports)]
use num_traits::float::Float;

/// Time constant of the catch-up after release, in seconds.
const CATCH_UP_TIME: f32 = 0.15;

/// Highest playback speed used while catching up.
const MAX_SPEED: f32 = 4.0;

/// Speed below which the output is faded out to avoid a frozen sample turning into DC.
const MUTE_SPEED: f32 = 0.05;

#[derive(Debug, Clone)]
pub struct TapeStop {
    buffer: RecordBuffer,
    buffer_length: f32,
    sample_rate_hz: f32,
    engaged: bool,
    /// Progress of the slow down, `0.0` at full speed and `1.0` when stopped.
    stop_phase: f32,
    /// Playback position expressed as a distance behind the write position.
    lag: f32,
}

impl Default for TapeStop {
    fn default() -> Self {
        Self {
            buffer: RecordBuffer::new(),
            buffer_length: DEFAULT_BUFFER_LENGTH,
            sample_rate_hz: 48000.0,
            engaged: false,
            stop_phase: 0.0,
            lag: 1.0,
        }
    }
}

impl TapeStop {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an instance keeping `length` seconds of audio history, which also limits
    /// how far playback can fall behind the input.
    pub fn with_length(length: f32) -> Self {
        Self {
            buffer_length: length.max(0.05),
            ..Self::default()
        }
    }

    /// Returns the playback speed as a ratio of the input speed.
    pub fn speed(&self) -> f32 {
        if self.engaged {
            Self::speed_from_phase(self.stop_phase, 1.0)
        } else {
            self.catch_up_speed()
        }
    }

    fn max_lag(&self) -> f32 {
        (self.buffer.len() as f32 - 4.0).max(1.0)
    }

    fn catch_up_speed(&self) -> f32 {
        (1.0 + self.lag / (CATCH_UP_TIME * self.sample_rate_hz)).min(MAX_SPEED)
    }

    /// Maps the progress of the slow down to a playback speed. `curve` bends the ramp
    /// from a linear slow down towards an abrupt one.
    fn speed_from_phase(phase: f32, curve: f32) -> f32 {
        (1.0 - phase).max(0.0).powf(curve)
    }
}

impl PerformanceEffect for TapeStop {
    fn init(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = sample_rate_hz.max(1.0);
        let length = (self.buffer_length * self.sample_rate_hz) as usize;
        self.buffer.init(length.max(8));
        self.reset();
    }

    fn reset(&mut self) {
        self.buffer.reset();
        self.engaged = false;
        self.stop_phase = 0.0;
        self.lag = 1.0;
    }

    fn engage(&mut self, _parameters: &EffectParameters, _transport: &Transport) {
        self.engaged = true;
        self.stop_phase = 0.0;
    }

    fn release(&mut self, _parameters: &EffectParameters, _transport: &Transport) {
        self.engaged = false;
        self.stop_phase = 0.0;
    }

    fn is_active(&self) -> bool {
        !self.engaged && self.lag > 1.5
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

        if !self.engaged && self.lag <= 1.5 {
            self.lag = 1.0;

            for sample in in_out.iter() {
                self.buffer.write(*sample);
            }

            return;
        }

        // The speed follows an analytic ramp while the pad is held down, so it can be
        // evaluated at the block boundaries and interpolated in between.
        let (speed, speed_increment) = if self.engaged {
            let stop_samples = transport.samples_for(parameters.division).max(1) as f32;
            let curve = 1.0 + parameters.amount.clamp(0.0, 1.0) * 3.0;
            let phase = self.stop_phase;
            let next_phase = (phase + size as f32 / stop_samples).min(1.0);
            self.stop_phase = next_phase;

            let speed = Self::speed_from_phase(phase, curve);
            let next_speed = Self::speed_from_phase(next_phase, curve);

            (speed, (next_speed - speed) / size as f32)
        } else {
            (self.catch_up_speed(), 0.0)
        };

        let max_lag = self.max_lag();
        let mut speed = speed;
        let mut lag = self.lag;

        for sample in in_out.iter_mut() {
            self.buffer.write(*sample);

            let gain = (speed * (1.0 / MUTE_SPEED)).min(1.0);
            *sample = self.buffer.read_frac(lag) * gain;

            lag = (lag + 1.0 - speed).clamp(1.0, max_lag);
            speed += speed_increment;
        }

        self.lag = lag;
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
