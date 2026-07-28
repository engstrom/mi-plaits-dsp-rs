//! Beat synced reverse playback.
//!
//! Every grid line of the selected [`Division`], the slice of audio that
//! was just recorded is played back backwards. Holding the pad therefore turns the
//! incoming audio inside out slice by slice, while [`Reverse::set_hold`] freezes the first
//! captured slice instead.

use core::any::Any;

use alloc::vec;
use alloc::vec::Vec;

use super::buffer::RecordBuffer;
use super::{Division, EffectParameters, PerformanceEffect, Transport, DEFAULT_BUFFER_LENGTH};

/// Longest declick fade applied at the slice boundaries, in seconds.
const MAX_FADE_TIME: f32 = 0.01;

#[derive(Debug, Clone)]
pub struct Reverse {
    buffer: RecordBuffer,
    slice: Vec<f32>,
    buffer_length: f32,
    sample_rate_hz: f32,
    active: bool,
    hold: bool,
    division: Division,
    loop_length: usize,
    loop_phase: usize,
}

impl Default for Reverse {
    fn default() -> Self {
        Self {
            buffer: RecordBuffer::new(),
            slice: Vec::new(),
            buffer_length: DEFAULT_BUFFER_LENGTH,
            sample_rate_hz: 48000.0,
            active: false,
            hold: false,
            division: Division::default(),
            loop_length: 0,
            loop_phase: 0,
        }
    }
}

impl Reverse {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an instance keeping `length` seconds of audio history. The longest slice
    /// that can be reversed is half of it.
    pub fn with_length(length: f32) -> Self {
        Self {
            buffer_length: length.max(0.05),
            ..Self::default()
        }
    }

    /// When enabled, the slice captured on engage is looped backwards instead of
    /// reversing the incoming audio slice by slice.
    pub fn set_hold(&mut self, hold: bool) {
        self.hold = hold;
    }

    pub fn hold(&self) -> bool {
        self.hold
    }

    fn capture(&mut self, loop_length: usize, offset: usize) {
        self.loop_length = loop_length.clamp(1, self.slice.len());
        self.buffer.copy_out(
            offset + self.loop_length,
            &mut self.slice[..self.loop_length],
        );
    }

    fn slice_length(&self, transport: &Transport, division: Division) -> usize {
        transport.samples_for(division).clamp(1, self.slice.len())
    }
}

impl PerformanceEffect for Reverse {
    fn init(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = sample_rate_hz.max(1.0);
        let length = (self.buffer_length * self.sample_rate_hz) as usize;
        self.buffer.init(length.max(8));
        self.slice = vec![0.0; self.buffer.len() / 2];
        self.reset();
    }

    fn reset(&mut self) {
        self.buffer.reset();
        self.slice.fill(0.0);
        self.active = false;
        self.loop_length = 0;
        self.loop_phase = 0;
    }

    fn engage(&mut self, parameters: &EffectParameters, transport: &Transport) {
        if self.slice.is_empty() {
            return;
        }

        self.division = parameters.division;

        let loop_length = self.slice_length(transport, self.division);
        let phase = transport.samples_since_grid(self.division) % loop_length;

        self.capture(loop_length, phase);
        self.loop_phase = phase.min(self.loop_length - 1);
        self.active = true;
    }

    fn release(&mut self, _parameters: &EffectParameters, _transport: &Transport) {
        self.active = false;
    }

    fn process(
        &mut self,
        parameters: &EffectParameters,
        transport: &Transport,
        in_out: &mut [f32],
    ) {
        if !self.active || self.loop_length == 0 {
            for sample in in_out.iter() {
                self.buffer.write(*sample);
            }
            return;
        }

        let fade_length = (parameters.amount.clamp(0.0, 1.0) * MAX_FADE_TIME * self.sample_rate_hz)
            .min(self.loop_length as f32 * 0.25);

        for sample in in_out.iter_mut() {
            self.buffer.write(*sample);

            let position = self.loop_length - 1 - self.loop_phase;
            let mut value = self.slice[position];

            if fade_length >= 1.0 {
                let distance = self.loop_phase.min(self.loop_length - 1 - self.loop_phase) as f32;
                value *= (distance / fade_length).min(1.0);
            }

            *sample = value;

            self.loop_phase += 1;

            if self.loop_phase >= self.loop_length {
                self.loop_phase = 0;

                if !self.hold {
                    // The slice that just finished recording becomes the next one to be
                    // played backwards.
                    self.division = parameters.division;
                    let loop_length = self.slice_length(transport, self.division);
                    self.capture(loop_length, 0);
                }
            }
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
