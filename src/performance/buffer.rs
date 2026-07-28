//! Circular audio history buffer shared by the buffer based performance effects.

use alloc::vec;
use alloc::vec::Vec;

/// Ring buffer keeping the most recently written samples.
///
/// Delays are expressed in samples counted backwards from the write position, a delay of
/// `1` being the sample that was written last.
#[derive(Debug, Default, Clone)]
pub struct RecordBuffer {
    line: Vec<f32>,
    write_pos: usize,
}

impl RecordBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates the buffer with the given length in samples and clears it.
    pub fn init(&mut self, length: usize) {
        self.line = vec![0.0; length.max(4)];
        self.write_pos = 0;
    }

    pub fn reset(&mut self) {
        self.line.fill(0.0);
        self.write_pos = 0;
    }

    pub fn len(&self) -> usize {
        self.line.len()
    }

    pub fn is_empty(&self) -> bool {
        self.line.is_empty()
    }

    #[inline]
    pub fn write(&mut self, sample: f32) {
        if self.line.is_empty() {
            return;
        }

        self.line[self.write_pos] = sample;
        self.write_pos += 1;

        if self.write_pos >= self.line.len() {
            self.write_pos = 0;
        }
    }

    /// Returns the sample written `delay` samples ago.
    #[inline]
    pub fn read(&self, delay: usize) -> f32 {
        if self.line.is_empty() {
            return 0.0;
        }

        let len = self.line.len();
        let delay = delay.clamp(1, len);

        self.line[(self.write_pos + len - delay) % len]
    }

    /// Returns the sample written `delay` samples ago using linear interpolation
    /// between the two adjacent samples.
    #[inline]
    pub fn read_frac(&self, delay: f32) -> f32 {
        if self.line.is_empty() {
            return 0.0;
        }

        let delay = delay.clamp(1.0, (self.line.len() - 1) as f32);
        let delay_integral = delay as usize;
        let delay_fractional = delay - (delay_integral as f32);
        let a = self.read(delay_integral);
        let b = self.read(delay_integral + 1);

        a + (b - a) * delay_fractional
    }

    /// Copies `dest.len()` consecutive samples into `dest`, `dest[0]` being the sample
    /// written `delay` samples ago.
    pub fn copy_out(&self, delay: usize, dest: &mut [f32]) {
        for (n, sample) in dest.iter_mut().enumerate() {
            *sample = self.read(delay.saturating_sub(n).max(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_samples_in_reverse_order() {
        let mut buffer = RecordBuffer::new();
        buffer.init(8);

        for n in 0..5 {
            buffer.write(n as f32);
        }

        assert_eq!(buffer.read(1), 4.0);
        assert_eq!(buffer.read(2), 3.0);
        assert_eq!(buffer.read(5), 0.0);
    }

    #[test]
    fn read_wraps_around() {
        let mut buffer = RecordBuffer::new();
        buffer.init(4);

        for n in 0..10 {
            buffer.write(n as f32);
        }

        assert_eq!(buffer.read(1), 9.0);
        assert_eq!(buffer.read(4), 6.0);
    }

    #[test]
    fn copy_out_returns_a_forward_running_slice() {
        let mut buffer = RecordBuffer::new();
        buffer.init(16);

        for n in 0..10 {
            buffer.write(n as f32);
        }

        let mut slice = [0.0; 4];
        buffer.copy_out(6, &mut slice);

        assert_eq!(slice, [4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn read_frac_interpolates() {
        let mut buffer = RecordBuffer::new();
        buffer.init(8);

        buffer.write(0.0);
        buffer.write(2.0);

        assert_eq!(buffer.read_frac(1.5), 1.0);
    }
}
