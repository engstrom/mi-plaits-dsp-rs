//! Koala Sampler style live performance effects.
//!
//! This module is not part of the Plaits port. It adds a small set of tempo synced
//! effects meant to be driven like the FX pads of a live sampler: each effect sits in a
//! [`Slot`] of a [`PerformanceFx`] chain and is *engaged* while a pad is held down.
//!
//! All effects are inserts that are always fed with the incoming audio, even while
//! bypassed, so that buffer based effects like [`BeatRepeat`] or [`Reverse`] always have
//! recent audio to work with. The chain crossfades between the dry input and the effect
//! output whenever a pad is pressed or released, which keeps the transitions click free.
//!
//! Every slot exposes the same three performance parameters:
//!
//! - `amount`: the main character control of the effect
//! - `mix`: dry/wet, applied by the chain, so it means the same thing for every effect
//! - `division`: the tempo synced rate, see [`Division`]
//!
//! ```
//! use mi_plaits_dsp::performance::{Division, EffectKind, PerformanceFx};
//!
//! let mut fx = PerformanceFx::new();
//! fx.init(48000.0);
//! fx.set_tempo(120.0);
//!
//! let repeat = fx.add_kind(EffectKind::BeatRepeat);
//! fx.set_division(repeat, Division::Sixteenth);
//!
//! let mut block = [0.0; 24];
//! fx.engage(repeat);
//! fx.process(&mut block);
//! fx.release(repeat);
//! ```

pub mod beat_repeat;
pub mod buffer;
pub mod crush;
pub mod delay;
pub mod filter_sweep;
pub mod gate;
pub mod reverse;
pub mod tape_stop;

use core::any::Any;

use alloc::boxed::Box;
use alloc::vec::Vec;

use dyn_clone::DynClone;

use crate::utils::parameter_interpolator::ParameterInterpolator;

pub use beat_repeat::BeatRepeat;
pub use crush::Crush;
pub use delay::Delay;
pub use filter_sweep::FilterSweep;
pub use gate::Gate;
pub use reverse::Reverse;
pub use tape_stop::TapeStop;

/// Length of the audio history kept by the buffer based effects, in seconds.
pub const DEFAULT_BUFFER_LENGTH: f32 = 2.0;

/// Duration of the crossfade applied when a slot is engaged or released, in seconds.
const FADE_TIME: f32 = 0.005;

/// Number of beats per bar assumed for [`Division::Bar`].
const BEATS_PER_BAR: f32 = 4.0;

/// Tempo synced rate of an effect, expressed as a note value.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Division {
    Bar,
    Half,
    DottedQuarter,
    HalfTriplet,
    Quarter,
    DottedEighth,
    QuarterTriplet,
    #[default]
    Eighth,
    DottedSixteenth,
    EighthTriplet,
    Sixteenth,
    SixteenthTriplet,
    ThirtySecond,
    SixtyFourth,
}

/// All divisions, ordered from the longest to the shortest.
pub const DIVISIONS: [Division; 14] = [
    Division::Bar,
    Division::Half,
    Division::DottedQuarter,
    Division::HalfTriplet,
    Division::Quarter,
    Division::DottedEighth,
    Division::QuarterTriplet,
    Division::Eighth,
    Division::DottedSixteenth,
    Division::EighthTriplet,
    Division::Sixteenth,
    Division::SixteenthTriplet,
    Division::ThirtySecond,
    Division::SixtyFourth,
];

impl Division {
    /// Returns the length of the division in beats.
    pub fn beats(&self) -> f32 {
        match self {
            Division::Bar => BEATS_PER_BAR,
            Division::Half => 2.0,
            Division::DottedQuarter => 1.5,
            Division::HalfTriplet => 2.0 * 2.0 / 3.0,
            Division::Quarter => 1.0,
            Division::DottedEighth => 0.75,
            Division::QuarterTriplet => 2.0 / 3.0,
            Division::Eighth => 0.5,
            Division::DottedSixteenth => 0.375,
            Division::EighthTriplet => 1.0 / 3.0,
            Division::Sixteenth => 0.25,
            Division::SixteenthTriplet => 1.0 / 6.0,
            Division::ThirtySecond => 0.125,
            Division::SixtyFourth => 0.0625,
        }
    }

    /// Returns a short name for display purposes.
    pub fn name(&self) -> &'static str {
        match self {
            Division::Bar => "1 bar",
            Division::Half => "1/2",
            Division::DottedQuarter => "1/4.",
            Division::HalfTriplet => "1/2T",
            Division::Quarter => "1/4",
            Division::DottedEighth => "1/8.",
            Division::QuarterTriplet => "1/4T",
            Division::Eighth => "1/8",
            Division::DottedSixteenth => "1/16.",
            Division::EighthTriplet => "1/8T",
            Division::Sixteenth => "1/16",
            Division::SixteenthTriplet => "1/16T",
            Division::ThirtySecond => "1/32",
            Division::SixtyFourth => "1/64",
        }
    }

    /// Maps a normalized control value to a division, `0.0` selecting the longest and
    /// `1.0` the shortest one.
    pub fn from_amount(amount: f32) -> Self {
        let index = (amount.clamp(0.0, 1.0) * (DIVISIONS.len() - 1) as f32 + 0.5) as usize;

        DIVISIONS[index.min(DIVISIONS.len() - 1)]
    }
}

/// Musical time base shared by all effects of a chain.
///
/// The transport is a free running sample counter. It does not need to be locked to an
/// external clock, but [`Transport::reset`] or [`Transport::set_position`] can be used to
/// align it with one.
#[derive(Debug, Clone)]
pub struct Transport {
    sample_rate_hz: f32,
    tempo_bpm: f32,
    samples_per_beat: f32,
    position: u64,
}

impl Default for Transport {
    fn default() -> Self {
        let mut transport = Self {
            sample_rate_hz: 48000.0,
            tempo_bpm: 120.0,
            samples_per_beat: 0.0,
            position: 0,
        };
        transport.update();

        transport
    }
}

impl Transport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = sample_rate_hz.max(1.0);
        self.position = 0;
        self.update();
    }

    /// Sets the tempo in beats per minute, clamped to the `1.0` to `999.0` range.
    pub fn set_tempo(&mut self, tempo_bpm: f32) {
        self.tempo_bpm = tempo_bpm.clamp(1.0, 999.0);
        self.update();
    }

    pub fn tempo(&self) -> f32 {
        self.tempo_bpm
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate_hz
    }

    /// Restarts the transport at the beginning of a bar.
    pub fn reset(&mut self) {
        self.position = 0;
    }

    /// Sets the playhead position as a number of samples since the beginning of a bar.
    pub fn set_position(&mut self, position: u64) {
        self.position = position;
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    /// Moves the playhead forward. Called once per block by [`PerformanceFx::process`].
    pub fn advance(&mut self, num_samples: usize) {
        self.position = self.position.wrapping_add(num_samples as u64);
    }

    pub fn samples_per_beat(&self) -> f32 {
        self.samples_per_beat
    }

    /// Returns the length of a division in samples.
    pub fn samples_for(&self, division: Division) -> usize {
        ((self.samples_per_beat * division.beats()) as usize).max(1)
    }

    /// Returns the number of samples elapsed since the last grid line of a division.
    pub fn samples_since_grid(&self, division: Division) -> usize {
        let length = self.samples_for(division) as u64;

        (self.position % length) as usize
    }

    /// Returns the position within the current division as a `0.0` to `1.0` phase.
    pub fn phase(&self, division: Division) -> f32 {
        let length = self.samples_for(division);

        self.samples_since_grid(division) as f32 / length as f32
    }

    fn update(&mut self) {
        self.samples_per_beat = self.sample_rate_hz * 60.0 / self.tempo_bpm;
    }
}

/// Performance parameters of a single effect.
#[derive(Debug, Clone, Copy)]
pub struct EffectParameters {
    /// Main character control of the effect.
    /// Range: `0.0` - `1.0`
    pub amount: f32,

    /// Dry/wet balance applied by the chain, `0.0` being fully dry.
    /// Range: `0.0` - `1.0`
    pub mix: f32,

    /// Tempo synced rate.
    pub division: Division,
}

impl Default for EffectParameters {
    fn default() -> Self {
        Self {
            amount: 0.5,
            mix: 1.0,
            division: Division::default(),
        }
    }
}

/// A single performance effect.
///
/// Implementations render the fully wet signal. The dry/wet balance and the click free
/// transitions are handled by [`PerformanceFx`].
pub trait PerformanceEffect: Send + Sync + DynClone {
    /// Allocates the resources of the effect for the given sample rate.
    fn init(&mut self, sample_rate_hz: f32);

    /// Clears all internal state.
    fn reset(&mut self) {}

    /// Called when the pad driving the effect is pressed.
    fn engage(&mut self, _parameters: &EffectParameters, _transport: &Transport) {}

    /// Called when the pad driving the effect is released.
    fn release(&mut self, _parameters: &EffectParameters, _transport: &Transport) {}

    /// Returns `true` while the effect still produces a tail after being released. The
    /// chain keeps such an effect wet until it returns `false` again.
    fn is_active(&self) -> bool {
        false
    }

    /// Processes a block of audio in place. Called for every block, also while the effect
    /// is bypassed, so that its buffers stay up to date.
    fn process(&mut self, parameters: &EffectParameters, transport: &Transport, in_out: &mut [f32]);

    /// Gives access to the concrete effect type, see [`PerformanceFx::effect_mut`].
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

dyn_clone::clone_trait_object!(PerformanceEffect);

/// The effects available from the factory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    BeatRepeat,
    Reverse,
    TapeStop,
    Gate,
    FilterSweep,
    Crush,
    Delay,
}

/// All effect kinds.
pub const EFFECT_KINDS: [EffectKind; 7] = [
    EffectKind::BeatRepeat,
    EffectKind::Reverse,
    EffectKind::TapeStop,
    EffectKind::Gate,
    EffectKind::FilterSweep,
    EffectKind::Crush,
    EffectKind::Delay,
];

impl EffectKind {
    /// Creates an uninitialized instance of the effect.
    pub fn create(&self) -> Box<dyn PerformanceEffect> {
        match self {
            EffectKind::BeatRepeat => Box::new(BeatRepeat::new()),
            EffectKind::Reverse => Box::new(Reverse::new()),
            EffectKind::TapeStop => Box::new(TapeStop::new()),
            EffectKind::Gate => Box::new(Gate::new()),
            EffectKind::FilterSweep => Box::new(FilterSweep::new()),
            EffectKind::Crush => Box::new(Crush::new()),
            EffectKind::Delay => Box::new(Delay::new()),
        }
    }

    /// Returns a short name for display purposes.
    pub fn name(&self) -> &'static str {
        match self {
            EffectKind::BeatRepeat => "Repeat",
            EffectKind::Reverse => "Reverse",
            EffectKind::TapeStop => "Tape Stop",
            EffectKind::Gate => "Gate",
            EffectKind::FilterSweep => "Filter",
            EffectKind::Crush => "Crush",
            EffectKind::Delay => "Delay",
        }
    }
}

/// An effect together with its performance parameters and pad state.
#[derive(Clone)]
pub struct Slot {
    effect: Box<dyn PerformanceEffect>,
    parameters: EffectParameters,
    engaged: bool,
    fade: f32,
    mix_state: f32,
}

impl core::fmt::Debug for Slot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Slot")
            .field("parameters", &self.parameters)
            .field("engaged", &self.engaged)
            .field("fade", &self.fade)
            .finish_non_exhaustive()
    }
}

impl Slot {
    fn new(effect: Box<dyn PerformanceEffect>) -> Self {
        Self {
            effect,
            parameters: EffectParameters::default(),
            engaged: false,
            fade: 0.0,
            mix_state: 1.0,
        }
    }
}

/// Chain of performance effects driven like the FX pads of a live sampler.
#[derive(Debug, Default, Clone)]
pub struct PerformanceFx {
    transport: Transport,
    slots: Vec<Slot>,
    dry: Vec<f32>,
    fade_increment: f32,
    sample_rate_hz: f32,
}

impl PerformanceFx {
    pub fn new() -> Self {
        Self::default()
    }

    /// Initializes the chain and all effects already added to it.
    pub fn init(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = sample_rate_hz.max(1.0);
        self.fade_increment = 1.0 / (FADE_TIME * self.sample_rate_hz).max(1.0);
        self.transport.init(self.sample_rate_hz);

        for slot in self.slots.iter_mut() {
            slot.effect.init(self.sample_rate_hz);
            slot.engaged = false;
            slot.fade = 0.0;
        }
    }

    /// Clears the state of all effects without releasing their buffers.
    pub fn reset(&mut self) {
        self.transport.reset();

        for slot in self.slots.iter_mut() {
            slot.effect.reset();
            slot.engaged = false;
            slot.fade = 0.0;
        }
    }

    /// Appends an effect to the chain and returns the index of its slot.
    pub fn add(&mut self, mut effect: Box<dyn PerformanceEffect>) -> usize {
        if self.sample_rate_hz > 0.0 {
            effect.init(self.sample_rate_hz);
        }

        self.slots.push(Slot::new(effect));

        self.slots.len() - 1
    }

    /// Appends an effect of the given kind to the chain and returns the index of its slot.
    pub fn add_kind(&mut self, kind: EffectKind) -> usize {
        self.add(kind.create())
    }

    pub fn num_slots(&self) -> usize {
        self.slots.len()
    }

    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut Transport {
        &mut self.transport
    }

    pub fn set_tempo(&mut self, tempo_bpm: f32) {
        self.transport.set_tempo(tempo_bpm);
    }

    /// Presses the pad of a slot.
    pub fn engage(&mut self, slot: usize) {
        if let Some(slot) = self.slots.get_mut(slot) {
            if !slot.engaged {
                slot.engaged = true;
                slot.effect.engage(&slot.parameters, &self.transport);
            }
        }
    }

    /// Releases the pad of a slot.
    pub fn release(&mut self, slot: usize) {
        if let Some(slot) = self.slots.get_mut(slot) {
            if slot.engaged {
                slot.engaged = false;
                slot.effect.release(&slot.parameters, &self.transport);
            }
        }
    }

    /// Releases the pads of all slots.
    pub fn release_all(&mut self) {
        for index in 0..self.slots.len() {
            self.release(index);
        }
    }

    pub fn is_engaged(&self, slot: usize) -> bool {
        self.slots.get(slot).is_some_and(|slot| slot.engaged)
    }

    pub fn set_parameters(&mut self, slot: usize, parameters: EffectParameters) {
        if let Some(slot) = self.slots.get_mut(slot) {
            slot.parameters = parameters;
        }
    }

    pub fn parameters(&self, slot: usize) -> Option<&EffectParameters> {
        self.slots.get(slot).map(|slot| &slot.parameters)
    }

    pub fn set_amount(&mut self, slot: usize, amount: f32) {
        if let Some(slot) = self.slots.get_mut(slot) {
            slot.parameters.amount = amount.clamp(0.0, 1.0);
        }
    }

    pub fn set_mix(&mut self, slot: usize, mix: f32) {
        if let Some(slot) = self.slots.get_mut(slot) {
            slot.parameters.mix = mix.clamp(0.0, 1.0);
        }
    }

    pub fn set_division(&mut self, slot: usize, division: Division) {
        if let Some(slot) = self.slots.get_mut(slot) {
            slot.parameters.division = division;
        }
    }

    /// Gives mutable access to a concrete effect to reach settings that are not part of
    /// the common performance parameters.
    ///
    /// ```
    /// use mi_plaits_dsp::performance::{EffectKind, FilterSweep, PerformanceFx};
    ///
    /// let mut fx = PerformanceFx::new();
    /// fx.init(48000.0);
    /// let slot = fx.add_kind(EffectKind::FilterSweep);
    /// fx.effect_mut::<FilterSweep>(slot).unwrap().set_resonance(0.8);
    /// ```
    pub fn effect_mut<T: PerformanceEffect + 'static>(&mut self, slot: usize) -> Option<&mut T> {
        self.slots
            .get_mut(slot)
            .and_then(|slot| slot.effect.as_any_mut().downcast_mut::<T>())
    }

    /// Processes a block of audio in place and advances the transport.
    pub fn process(&mut self, in_out: &mut [f32]) {
        let size = in_out.len();

        if size == 0 {
            return;
        }

        if self.dry.len() < size {
            self.dry.resize(size, 0.0);
        }

        let fade_increment = self.fade_increment;

        for slot in self.slots.iter_mut() {
            let parameters = slot.parameters;
            let dry = &mut self.dry[..size];
            dry.copy_from_slice(in_out);

            slot.effect.process(&parameters, &self.transport, in_out);

            let target = if slot.engaged || slot.effect.is_active() {
                1.0
            } else {
                0.0
            };
            let mut fade = slot.fade;

            {
                let mut mix_modulation = ParameterInterpolator::new(
                    &mut slot.mix_state,
                    parameters.mix.clamp(0.0, 1.0),
                    size,
                );

                for (sample, dry_sample) in in_out.iter_mut().zip(dry.iter()) {
                    if fade < target {
                        fade = (fade + fade_increment).min(target);
                    } else if fade > target {
                        fade = (fade - fade_increment).max(target);
                    }

                    let amount = fade * mix_modulation.next();
                    *sample = *dry_sample + (*sample - *dry_sample) * amount;
                }
            }

            slot.fade = fade;
        }

        self.transport.advance(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisions_are_ordered_from_long_to_short() {
        for pair in DIVISIONS.windows(2) {
            assert!(
                pair[0].beats() > pair[1].beats(),
                "{} is not longer than {}",
                pair[0].name(),
                pair[1].name()
            );
        }
    }

    #[test]
    fn division_from_amount_covers_the_full_range() {
        assert_eq!(Division::from_amount(0.0), Division::Bar);
        assert_eq!(Division::from_amount(1.0), Division::SixtyFourth);
        assert_eq!(Division::from_amount(-1.0), Division::Bar);
        assert_eq!(Division::from_amount(2.0), Division::SixtyFourth);
    }

    #[test]
    fn transport_grid_matches_the_tempo() {
        let mut transport = Transport::new();
        transport.init(48000.0);
        transport.set_tempo(120.0);

        // 120 BPM: one beat is half a second.
        assert_eq!(transport.samples_for(Division::Quarter), 24000);
        assert_eq!(transport.samples_for(Division::Sixteenth), 6000);
        assert_eq!(transport.samples_for(Division::Bar), 96000);

        transport.advance(3000);
        assert_eq!(transport.samples_since_grid(Division::Sixteenth), 3000);
        assert_eq!(transport.phase(Division::Sixteenth), 0.5);

        transport.advance(3000);
        assert_eq!(transport.samples_since_grid(Division::Sixteenth), 0);
    }

    #[test]
    fn bypassed_chain_passes_the_signal_through() {
        let mut fx = PerformanceFx::new();
        fx.init(48000.0);

        for kind in EFFECT_KINDS {
            fx.add_kind(kind);
        }

        let mut block = [0.0; 32];

        for n in 0..64 {
            for (index, sample) in block.iter_mut().enumerate() {
                *sample = ((n * 32 + index) % 17) as f32 / 17.0 - 0.5;
            }

            let expected = block;
            fx.process(&mut block);

            for (sample, expected) in block.iter().zip(expected.iter()) {
                assert!(
                    (sample - expected).abs() < 1.0e-6,
                    "bypassed chain altered the signal: {sample} != {expected}"
                );
            }
        }
    }

    #[test]
    fn engaging_a_slot_fades_in_without_a_jump() {
        let mut fx = PerformanceFx::new();
        fx.init(48000.0);
        let slot = fx.add_kind(EffectKind::Gate);
        fx.set_amount(slot, 0.0);

        let mut block = [1.0; 64];
        fx.process(&mut block);
        fx.engage(slot);

        let mut previous = 1.0;

        for _ in 0..16 {
            block.fill(1.0);
            fx.process(&mut block);

            for sample in block.iter() {
                assert!(
                    (sample - previous).abs() < 0.05,
                    "discontinuity in the fade: {previous} -> {sample}"
                );
                previous = *sample;
            }
        }
    }
}
