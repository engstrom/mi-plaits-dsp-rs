//! Tests for wavetable engine

use std::sync::Arc;

use mi_plaits_dsp::engine::wavetable_engine::USER_POOL_LEN;
use mi_plaits_dsp::engine::*;

use crate::common::*;

const SAMPLE_RATE: f32 = 48000.0;
const A0_NORMALIZED: f32 = 55.0 / SAMPLE_RATE;
const BLOCK_SIZE: usize = 24;

#[test]
fn wavetable_engine_harmonics() {
    let mut engine = wavetable_engine::WavetableEngine::new();
    let mut out = [0.0; BLOCK_SIZE];
    let mut aux = [0.0; BLOCK_SIZE];
    let mut wav_data = Vec::new();
    let mut wav_data_aux = Vec::new();

    engine.init(SAMPLE_RATE);

    let duration = 2.0;
    let blocks = (duration * SAMPLE_RATE / (BLOCK_SIZE as f32)) as usize;
    let mut already_enveloped = false;

    for n in 0..blocks {
        let parameters = EngineParameters {
            trigger: if n == 0 {
                TriggerState::RisingEdge
            } else {
                TriggerState::Low
            },
            note: 48.0,
            timbre: 0.5,
            morph: 0.5,
            harmonics: mod_ramp_up(n, blocks),
            accent: 1.0,
            a0_normalized: A0_NORMALIZED,
        };

        engine.render(&parameters, &mut out, &mut aux, &mut already_enveloped);
        wav_data.extend_from_slice(&out);
        wav_data_aux.extend_from_slice(&aux);
    }

    write_wav(
        "engines/wavetable/wavetable_harmonics.wav",
        &wav_data,
        SAMPLE_RATE as u32,
    )
    .ok();
    write_wav(
        "engines/wavetable/wavetable_harmonics_aux.wav",
        &wav_data_aux,
        SAMPLE_RATE as u32,
    )
    .ok();
}

#[test]
fn wavetable_engine_timbre() {
    let mut engine = wavetable_engine::WavetableEngine::new();
    let mut out = [0.0; BLOCK_SIZE];
    let mut aux = [0.0; BLOCK_SIZE];
    let mut wav_data = Vec::new();
    let mut wav_data_aux = Vec::new();

    engine.init(SAMPLE_RATE);

    let duration = 2.0;
    let blocks = (duration * SAMPLE_RATE / (BLOCK_SIZE as f32)) as usize;
    let mut already_enveloped = false;

    for n in 0..blocks {
        let parameters = EngineParameters {
            trigger: if n == 0 {
                TriggerState::RisingEdge
            } else {
                TriggerState::Low
            },
            note: 48.0,
            timbre: mod_ramp_up(n, blocks),
            morph: 0.5,
            harmonics: 0.5,
            accent: 1.0,
            a0_normalized: A0_NORMALIZED,
        };

        engine.render(&parameters, &mut out, &mut aux, &mut already_enveloped);
        wav_data.extend_from_slice(&out);
        wav_data_aux.extend_from_slice(&aux);
    }

    write_wav(
        "engines/wavetable/wavetable_timbre.wav",
        &wav_data,
        SAMPLE_RATE as u32,
    )
    .ok();
    write_wav(
        "engines/wavetable/wavetable_timbre_aux.wav",
        &wav_data_aux,
        SAMPLE_RATE as u32,
    )
    .ok();
}

#[test]
fn wavetable_engine_morph() {
    let mut engine = wavetable_engine::WavetableEngine::new();
    let mut out = [0.0; BLOCK_SIZE];
    let mut aux = [0.0; BLOCK_SIZE];
    let mut wav_data = Vec::new();
    let mut wav_data_aux = Vec::new();

    engine.init(SAMPLE_RATE);

    let duration = 2.0;
    let blocks = (duration * SAMPLE_RATE / (BLOCK_SIZE as f32)) as usize;
    let mut already_enveloped = false;

    for n in 0..blocks {
        let parameters = EngineParameters {
            trigger: if n == 0 {
                TriggerState::RisingEdge
            } else {
                TriggerState::Low
            },
            note: 48.0,
            timbre: 0.5,
            morph: mod_ramp_up(n, blocks),
            harmonics: 0.5,
            accent: 1.0,
            a0_normalized: A0_NORMALIZED,
        };

        engine.render(&parameters, &mut out, &mut aux, &mut already_enveloped);
        wav_data.extend_from_slice(&out);
        wav_data_aux.extend_from_slice(&aux);
    }

    write_wav(
        "engines/wavetable/wavetable_morph.wav",
        &wav_data,
        SAMPLE_RATE as u32,
    )
    .ok();
    write_wav(
        "engines/wavetable/wavetable_morph_aux.wav",
        &wav_data_aux,
        SAMPLE_RATE as u32,
    )
    .ok();
}

/// Builds a deterministic four-bank user pool.
///
/// Each of the 4 x 64 cells holds a `TABLE_SIZE + 4` wave; the waveform depends on
/// the cell index so that every bank, row and column is audibly distinct, and on
/// `variant` so that two different pools can be told apart.
fn build_user_pool(variant: usize) -> Box<[i16; USER_POOL_LEN]> {
    const WAVE_LEN: usize = 128 + 4;

    let mut pool = vec![0_i16; USER_POOL_LEN];

    for cell in 0..(4 * 64) {
        let harmonic = 1 + ((cell + variant) % 8);
        let amplitude = 8192.0 - 64.0 * ((cell % 32) as f32);

        for (index, sample) in pool[cell * WAVE_LEN..(cell + 1) * WAVE_LEN]
            .iter_mut()
            .enumerate()
        {
            let phase = (index as f32) / 128.0 * core::f32::consts::TAU;
            *sample = (amplitude * (phase * (harmonic as f32)).sin()) as i16;
        }
    }

    pool.into_boxed_slice().try_into().unwrap()
}

/// Renders a fixed harmonics/timbre sweep and returns the concatenated out and
/// aux samples, so that two engines can be compared sample by sample.
fn render_sweep(engine: &mut wavetable_engine::WavetableEngine, blocks: usize) -> Vec<f32> {
    let mut out = [0.0; BLOCK_SIZE];
    let mut aux = [0.0; BLOCK_SIZE];
    let mut samples = Vec::with_capacity(blocks * BLOCK_SIZE * 2);
    let mut already_enveloped = false;

    for n in 0..blocks {
        let parameters = EngineParameters {
            trigger: if n == 0 {
                TriggerState::RisingEdge
            } else {
                TriggerState::Low
            },
            note: 48.0,
            timbre: mod_ramp_up(n, blocks),
            morph: 0.5,
            harmonics: mod_triangle(n, blocks, 2.0).abs(),
            accent: 1.0,
            a0_normalized: A0_NORMALIZED,
        };

        engine.render(&parameters, &mut out, &mut aux, &mut already_enveloped);
        samples.extend_from_slice(&out);
        samples.extend_from_slice(&aux);
    }

    samples
}

/// Same pool installed through the owned and the shared setter must render
/// bit-identical output.
#[test]
fn wavetable_engine_user_banks_shared_matches_owned() {
    let blocks = 200;

    let mut stock = wavetable_engine::WavetableEngine::new();
    stock.init(SAMPLE_RATE);
    let stock_samples = render_sweep(&mut stock, blocks);

    let mut owned = wavetable_engine::WavetableEngine::new();
    owned.init(SAMPLE_RATE);
    owned.set_user_banks(build_user_pool(0));
    let owned_samples = render_sweep(&mut owned, blocks);

    let mut shared = wavetable_engine::WavetableEngine::new();
    shared.init(SAMPLE_RATE);
    shared.set_user_banks_shared(Arc::new(*build_user_pool(0)));
    let shared_samples = render_sweep(&mut shared, blocks);

    assert_eq!(
        bits(&owned_samples),
        bits(&shared_samples),
        "shared pool must render bit-identically to the same owned pool"
    );

    // Guards against a vacuous comparison: the pool has to actually be in use.
    assert_ne!(
        bits(&stock_samples),
        bits(&owned_samples),
        "user pool must replace the stock wavetables"
    );
}

/// Clearing the banks must return the engine to the stock wavetables, whichever
/// setter installed them.
#[test]
fn wavetable_engine_clear_user_banks_restores_stock() {
    let blocks = 100;

    let mut engine = wavetable_engine::WavetableEngine::new();
    engine.init(SAMPLE_RATE);
    let stock_samples = render_sweep(&mut engine, blocks);

    engine.init(SAMPLE_RATE);
    engine.set_user_banks_shared(Arc::new(*build_user_pool(3)));
    let user_samples = render_sweep(&mut engine, blocks);
    assert_ne!(bits(&stock_samples), bits(&user_samples));

    engine.clear_user_banks();
    engine.init(SAMPLE_RATE);
    let cleared_samples = render_sweep(&mut engine, blocks);
    assert_eq!(bits(&stock_samples), bits(&cleared_samples));
}

/// Two engines playing from one shared pool must not interfere: each renders
/// exactly what it would render alone, and dropping or clearing one leaves the
/// other's pool intact.
#[test]
fn wavetable_engine_shared_user_banks_do_not_interfere() {
    let blocks = 100;
    let pool = Arc::new(*build_user_pool(1));

    let mut reference = wavetable_engine::WavetableEngine::new();
    reference.init(SAMPLE_RATE);
    reference.set_user_banks_shared(Arc::clone(&pool));
    let reference_samples = render_sweep(&mut reference, blocks);

    let mut first = wavetable_engine::WavetableEngine::new();
    let mut second = wavetable_engine::WavetableEngine::new();
    first.init(SAMPLE_RATE);
    second.init(SAMPLE_RATE);
    first.set_user_banks_shared(Arc::clone(&pool));
    second.set_user_banks_shared(Arc::clone(&pool));

    // Installing the pool twice shares it instead of copying it.
    assert_eq!(Arc::strong_count(&pool), 4);

    let first_samples = render_sweep(&mut first, blocks);

    // The second engine renders the same sweep after the first one is done with
    // it, and while the first one still holds the pool.
    let second_samples = render_sweep(&mut second, blocks);
    assert_eq!(bits(&reference_samples), bits(&first_samples));
    assert_eq!(bits(&reference_samples), bits(&second_samples));

    // Clearing one engine and dropping another must not disturb the survivor.
    second.clear_user_banks();
    drop(reference);
    assert_eq!(Arc::strong_count(&pool), 2);

    first.init(SAMPLE_RATE);
    let first_again = render_sweep(&mut first, blocks);
    assert_eq!(bits(&reference_samples), bits(&first_again));
}

/// A cloned engine keeps rendering from the pool of the engine it was cloned
/// from, sharing it instead of duplicating the wave data.
#[test]
fn wavetable_engine_clone_shares_user_banks() {
    let blocks = 100;
    let pool = Arc::new(*build_user_pool(2));

    let mut engine = wavetable_engine::WavetableEngine::new();
    engine.init(SAMPLE_RATE);
    engine.set_user_banks_shared(Arc::clone(&pool));
    assert_eq!(Arc::strong_count(&pool), 2);

    let mut clone = engine.clone();
    assert_eq!(Arc::strong_count(&pool), 3);

    let samples = render_sweep(&mut engine, blocks);
    let clone_samples = render_sweep(&mut clone, blocks);
    assert_eq!(bits(&samples), bits(&clone_samples));
}

/// Bit patterns of the samples, so that comparisons are exact and `NaN`-safe.
fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|sample| sample.to_bits()).collect()
}
