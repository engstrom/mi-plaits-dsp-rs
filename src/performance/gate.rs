//! Tempo synced gate, also known as a trance gate or chopper.
//!
//! The gate opens on every grid line of the selected [`Division`](super::Division) and
//! stays open for the fraction of the division set by `amount`. The edges are smoothed to
//! avoid clicks, and the depth of the gate follows the dry/wet balance of the slot.

use core::any::Any;

use super::{EffectParameters, PerformanceEffect, Transport};
use crate::utils::one_pole;

/// Time constant of the edge smoothing, in seconds.
const EDGE_TIME: f32 = 0.0015;

#[derive(Debug, Clone)]
pub struct Gate {
    gain: f32,
    coefficient: f32,
    engaged: bool,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            gain: 1.0,
            coefficient: 1.0,
            engaged: false,
        }
    }
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PerformanceEffect for Gate {
    fn init(&mut self, sample_rate_hz: f32) {
        self.coefficient = (1.0 / (EDGE_TIME * sample_rate_hz.max(1.0))).clamp(0.0, 1.0);
        self.reset();
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.engaged = false;
    }

    fn engage(&mut self, _parameters: &EffectParameters, _transport: &Transport) {
        self.engaged = true;
    }

    fn release(&mut self, _parameters: &EffectParameters, _transport: &Transport) {
        self.engaged = false;
    }

    fn process(
        &mut self,
        parameters: &EffectParameters,
        transport: &Transport,
        in_out: &mut [f32],
    ) {
        if !self.engaged {
            // Open up again so that the next press starts from a known state.
            self.gain = 1.0;
            return;
        }

        let length = transport.samples_for(parameters.division);
        let duty = 0.02 + parameters.amount.clamp(0.0, 1.0) * 0.96;
        let open_samples = (length as f32 * duty) as usize;
        let mut position = transport.samples_since_grid(parameters.division);

        for sample in in_out.iter_mut() {
            let target = if position < open_samples { 1.0 } else { 0.0 };
            one_pole(&mut self.gain, target, self.coefficient);
            *sample *= self.gain;

            position += 1;

            if position >= length {
                position = 0;
            }
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
