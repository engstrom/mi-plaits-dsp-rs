//! Analog bass drum model.
//!
//! Grout change (engstrom/grout#166): `render` runs the analog model, the overdrive and the
//! synthetic model one sample at a time side by side, rather than as three loops one after
//! another. Each model's sample is the same arithmetic in the same order on its own state
//! (`AnalogBassDrum::step`, `OverdriveBlock::step`, `SyntheticBassDrum::step`, the bodies of
//! their own `render` loops, which `render` still runs), only the analog model's output reaches
//! the overdrive, and only the synthetic model draws from the PRNG, in the order it always did.
//! So OUT, AUX and the PRNG's state are the ones the three loops wrote. The analog model is one
//! long dependency chain a sample (two divisions and a diode's in it); beside it, the core has
//! the synthetic model's work to do while it waits.
//!
//! Behavioral simulation of circuits from classic drum machines.
//!
//! Engine parameters:
//! - *HARMONICS:* attack sharpness and amount of overdrive.
//! - *TIMBRE:* brightness.
//! - *MORPH:* decay time.
//!
//! Outputs:
//! - *OUT* signal: bridged T-network excited by a nicely shaped pulse.
//! - *AUX* signal: frequency-modulated triangle VCO, turned into a sine with a pair of diodes,
//!   and shaped by a dirty VCA.
//!
//! Without any signal patched to the *TRIG* input, a continuous tone is produced.

// Based on MIT-licensed code (c) 2016 by Emilie Gillet (emilie.o.gillet@gmail.com)

use super::{note_to_frequency, Engine, EngineParameters, TriggerState};
use crate::drums::analog_bass_drum::AnalogBassDrum;
use crate::drums::synthetic_bass_drum::SyntheticBassDrum;
use crate::fx::overdrive::Overdrive;

#[derive(Debug, Default, Clone)]
pub struct BassDrumEngine {
    analog_bass_drum: AnalogBassDrum,
    synthetic_bass_drum: SyntheticBassDrum,

    overdrive: Overdrive,
}

impl BassDrumEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Engine for BassDrumEngine {
    fn init(&mut self, sample_rate_hz: f32) {
        self.analog_bass_drum.init(sample_rate_hz);
        self.synthetic_bass_drum.init(sample_rate_hz);
        self.overdrive.init();
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

        let attack_fm_amount = f32::min(parameters.harmonics * 4.0, 1.0);
        let self_fm_amount = (parameters.harmonics * 4.0 - 1.0).clamp(0.0, 1.0);
        let drive =
            f32::max(parameters.harmonics * 2.0 - 1.0, 0.0) * f32::max(1.0 - 16.0 * f0, 0.0);

        let sustain = matches!(parameters.trigger, TriggerState::Unpatched);
        let trigger = matches!(parameters.trigger, TriggerState::RisingEdge);

        // The two models and the overdrive one sample at a time, side by side (Grout #166): each
        // is the same arithmetic in the same order as its own `render`, on its own state, and
        // only the analog model's output reaches the overdrive, so the samples are the ones the
        // three loops one after another write. The analog model is one long dependency chain a
        // sample; beside it, the core has the synthetic model's work to do while it waits.
        let mut analog = self.analog_bass_drum.begin(
            sustain,
            trigger,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            attack_fm_amount,
            self_fm_amount,
            out.len(),
        );
        let mut overdrive = self.overdrive.begin(0.5 + 0.5 * drive, out.len());
        let mut synthetic = self.synthetic_bass_drum.begin(
            sustain,
            trigger,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            if sustain {
                parameters.harmonics
            } else {
                0.4 - 0.25 * parameters.morph * parameters.morph
            },
            f32::min(parameters.harmonics * 2.0, 1.0),
            f32::max(parameters.harmonics * 2.0 - 1.0, 0.0),
            aux.len(),
        );
        let n = out.len().min(aux.len());
        for (o, a) in out[..n].iter_mut().zip(aux[..n].iter_mut()) {
            *o = overdrive.step(self.analog_bass_drum.step(&mut analog));
            *a = self.synthetic_bass_drum.step(&mut synthetic);
        }
        for o in out[n..].iter_mut() {
            *o = overdrive.step(self.analog_bass_drum.step(&mut analog));
        }
        for a in aux[n..].iter_mut() {
            *a = self.synthetic_bass_drum.step(&mut synthetic);
        }
        self.analog_bass_drum.end(&analog);
        self.overdrive.end(&overdrive);
        self.synthetic_bass_drum.end(&synthetic);
    }
}
