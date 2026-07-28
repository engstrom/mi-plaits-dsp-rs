//! Beat repeat, the classic stutter/roll effect.
//!
//! When engaged, the last complete slice of audio ending on the previous grid line of the
//! selected [`Division`] is captured and looped in sync with the
//! transport. The slice keeps playing until the pad is released, optionally getting
//! quieter with every repetition.

use core::any::Any;

use alloc::vec;
use alloc::vec::Vec;

use super::buffer::RecordBuffer;
use super::{Division, EffectParameters, PerformanceEffect, Transport, DEFAULT_BUFFER_LENGTH};

#[derive(Debug, Clone)]
pub struct BeatRepeat {
    buffer: RecordBuffer,
    slice: Vec<f32>,
    buffer_length: f32,
    active: bool,
    division: Division,
    loop_length: usize,
    loop_phase: usize,
    gain: f32,
}

impl Default for BeatRepeat {
    fn default() -> Self {
        Self {
            buffer: RecordBuffer::new(),
            slice: Vec::new(),
            buffer_length: DEFAULT_BUFFER_LENGTH,
            active: false,
            division: Division::default(),
            loop_length: 0,
            loop_phase: 0,
            gain: 1.0,
        }
    }
}

impl BeatRepeat {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an instance keeping `length` seconds of audio history. The longest slice
    /// that can be captured is half of it.
    pub fn with_length(length: f32) -> Self {
        Self {
            buffer_length: length.max(0.05),
            ..Self::default()
        }
    }

    /// Returns the length of the captured slice in samples.
    pub fn slice_samples(&self) -> usize {
        self.loop_length
    }

    /// Captures the slice ending `offset` samples ago.
    fn capture(&mut self, loop_length: usize, offset: usize) {
        self.loop_length = loop_length.clamp(1, self.slice.len());
        self.buffer.copy_out(
            offset + self.loop_length,
            &mut self.slice[..self.loop_length],
        );
    }

    /// Returns the slice length for a division, limited by the size of the buffer.
    fn slice_length(&self, transport: &Transport, division: Division) -> usize {
        transport.samples_for(division).clamp(1, self.slice.len())
    }
}

impl PerformanceEffect for BeatRepeat {
    fn init(&mut self, sample_rate_hz: f32) {
        let length = (self.buffer_length * sample_rate_hz) as usize;
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
        self.gain = 1.0;
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
        self.gain = 1.0;
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

        // `0.0` repeats the slice unchanged, `1.0` halves its level on every repetition.
        let decay = 1.0 - parameters.amount.clamp(0.0, 1.0) * 0.5;

        for sample in in_out.iter_mut() {
            self.buffer.write(*sample);

            *sample = self.slice[self.loop_phase] * self.gain;

            self.loop_phase += 1;

            if self.loop_phase >= self.loop_length {
                self.loop_phase = 0;
                self.gain *= decay;

                // Follow the rate control by grabbing a fresh slice of live audio
                // whenever the division changed while the pad was held down.
                if parameters.division != self.division {
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
