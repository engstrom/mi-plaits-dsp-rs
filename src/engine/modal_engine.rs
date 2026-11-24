//! Modal resonator.
//!
//! Engine parameters:
//! - *HARMONICS:* amount of inharmonicity, or material selection.
//! - *TIMBRE:* excitation brightness and dust density.
//! - *MORPH:* decay time (energy absorption).
//!
//! *AUX* signal: raw exciter signal.
//!
//! When the *TRIG* input is not patched, the resonator is excited by dust (particle) noise.
//! Otherwise, the resonator is excited by a short burst of filtered white noise,
//! or by a low-pass filtered click.

// Based on MIT-licensed code (c) 2016 by Emilie Gillet (emilie.o.gillet@gmail.com)

use alloc::boxed::Box;
use alloc::vec;

use super::{note_to_frequency, Engine, EngineParameters, TriggerState};
use crate::physical_modelling::modal_voice::ModalVoice;
use crate::utils::one_pole;

#[derive(Debug, Clone)]
pub struct ModalEngine {
    voice: ModalVoice,
    harmonics_lp: f32,

    temp_buffer_1: Box<[f32]>,
    temp_buffer_2: Box<[f32]>,
}

impl ModalEngine {
    pub fn new(block_size: usize) -> Self {
        Self {
            voice: ModalVoice::default(),
            harmonics_lp: 0.0,
            temp_buffer_1: vec![0.0; block_size].into_boxed_slice(),
            temp_buffer_2: vec![0.0; block_size].into_boxed_slice(),
        }
    }
}

impl Engine for ModalEngine {
    fn init(&mut self, _sample_rate_hz: f32) {
        self.harmonics_lp = 0.0;
        self.reset();
    }

    fn reset(&mut self) {
        self.voice.init();
    }

    /// HARMONICS' one-pole is slow (0.01 per block, ~460 blocks to 99%), so
    /// a note after `init()` would otherwise glide its material from 0.
    fn settle(&mut self, parameters: &EngineParameters) {
        self.harmonics_lp = parameters.harmonics;
    }

    #[inline]
    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        _already_enveloped: &mut bool,
    ) {
        out.fill(0.0);
        aux.fill(0.0);

        one_pole(&mut self.harmonics_lp, parameters.harmonics, 0.01);

        let sustain = matches!(parameters.trigger, TriggerState::Unpatched);
        let trigger = matches!(parameters.trigger, TriggerState::RisingEdge);

        self.voice.render(
            sustain,
            trigger,
            parameters.accent,
            note_to_frequency(parameters.note, parameters.a0_normalized),
            self.harmonics_lp,
            parameters.timbre,
            parameters.morph,
            &mut self.temp_buffer_1,
            &mut self.temp_buffer_2,
            out,
            aux,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settle_matches_a_long_run_and_holds() {
        let p = EngineParameters {
            trigger: TriggerState::High,
            note: 48.0,
            timbre: 0.5,
            morph: 0.5,
            harmonics: 0.73,
            accent: 1.0,
            a0_normalized: 55.0 / 48_000.0,
        };
        let (mut out, mut aux) = ([0.0; 24], [0.0; 24]);
        let mut run = ModalEngine::new(24);
        run.init(48_000.0);
        for _ in 0..4_000 {
            run.render(&p, &mut out, &mut aux, &mut false);
        }
        let mut snap = ModalEngine::new(24);
        snap.init(48_000.0);
        snap.settle(&p);
        assert!((run.harmonics_lp - snap.harmonics_lp).abs() < 1e-4);
        snap.render(&p, &mut out, &mut aux, &mut false);
        assert_eq!(snap.harmonics_lp, p.harmonics);
    }
}
