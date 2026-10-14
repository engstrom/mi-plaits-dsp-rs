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
    /// False from `init` until the first `render`, which starts `harmonics_lp` at the knob
    /// rather than smoothing up to it from 0.
    primed: bool,

    temp_buffer_1: Box<[f32]>,
    temp_buffer_2: Box<[f32]>,
}

impl ModalEngine {
    pub fn new(block_size: usize) -> Self {
        Self {
            voice: ModalVoice::default(),
            harmonics_lp: 0.0,
            primed: false,
            temp_buffer_1: vec![0.0; block_size].into_boxed_slice(),
            temp_buffer_2: vec![0.0; block_size].into_boxed_slice(),
        }
    }
}

impl Engine for ModalEngine {
    fn init(&mut self, _sample_rate_hz: f32) {
        self.harmonics_lp = 0.0;
        self.primed = false;
        self.reset();
    }

    fn reset(&mut self) {
        self.voice.init();
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

        // Grout: the first render after `init` starts the structure at the knob. On the
        // module `init` runs once, so the smoothing tracks the knob across notes and a held
        // knob is already where it points; a host that re-initialises the engine at every
        // note-on (Grout's `PlaitsVoice::restart`) would otherwise glide each note's
        // partials in from structure 0 over ~150 ms (grout #1038). Later renders smooth as
        // before, so a knob moved during a note still glides.
        if self.primed {
            one_pole(&mut self.harmonics_lp, parameters.harmonics, 0.01);
        } else {
            self.harmonics_lp = parameters.harmonics;
            self.primed = true;
        }

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
