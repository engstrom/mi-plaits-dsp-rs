//! Tests for the performance effects.

mod common;

use common::*;
use mi_plaits_dsp::performance::*;

const BLOCK_SIZE: usize = 24;
const TEMPO_BPM: f32 = 120.0;
const DURATION: f32 = 4.0;

/// Renders a simple sixteenth note pattern used as source material for the effects.
fn source(sample_rate: u32, duration: f32) -> Vec<f32> {
    const NOTES: [f32; 8] = [36.0, 48.0, 43.0, 48.0, 39.0, 48.0, 55.0, 48.0];

    let length = (sample_rate as f32 * duration) as usize;
    let step = (sample_rate as f32 * 60.0 / TEMPO_BPM / 4.0) as usize;
    let mut out = Vec::with_capacity(length);

    for n in 0..length {
        let note = NOTES[(n / step) % NOTES.len()];
        let position = (n % step) as f32;
        let frequency = 440.0 * ((note - 69.0) / 12.0).exp2();
        let phase = position * frequency / sample_rate as f32;
        let envelope = (-position / (step as f32 * 0.2)).exp();

        out.push((phase * core::f32::consts::TAU).sin() * envelope * 0.5);
    }

    out
}

/// Renders the pattern through a single effect that is held down between `hold_start` and
/// `hold_end` seconds, and writes the result as a WAV file.
fn render(name: &str, kind: EffectKind, amount: f32, division: Division, hold: (f32, f32)) {
    for sample_rate in SAMPLE_RATES {
        let mut fx = PerformanceFx::new();
        fx.init(sample_rate as f32);
        fx.set_tempo(TEMPO_BPM);

        let slot = fx.add_kind(kind);
        fx.set_amount(slot, amount);
        fx.set_division(slot, division);

        let source = source(sample_rate, DURATION);
        let mut wav_data = Vec::with_capacity(source.len());
        let mut block = [0.0; BLOCK_SIZE];
        let mut position = 0;

        while position + BLOCK_SIZE <= source.len() {
            block.copy_from_slice(&source[position..position + BLOCK_SIZE]);

            let time = position as f32 / sample_rate as f32;

            if time >= hold.0 && time < hold.1 {
                fx.engage(slot);
            } else {
                fx.release(slot);
            }

            fx.process(&mut block);
            wav_data.extend_from_slice(&block);
            position += BLOCK_SIZE;
        }

        let filename = format!("performance/{name}/{name}_{sample_rate}.wav");
        write_wav(filename, &wav_data, sample_rate).ok();
    }
}

#[test]
fn beat_repeat() {
    render(
        "beat_repeat",
        EffectKind::BeatRepeat,
        0.0,
        Division::Sixteenth,
        (1.0, 2.5),
    );
}

#[test]
fn beat_repeat_decay() {
    render(
        "beat_repeat_decay",
        EffectKind::BeatRepeat,
        0.8,
        Division::ThirtySecond,
        (1.0, 3.0),
    );
}

#[test]
fn reverse() {
    render(
        "reverse",
        EffectKind::Reverse,
        0.3,
        Division::Eighth,
        (1.0, 3.0),
    );
}

#[test]
fn tape_stop() {
    render(
        "tape_stop",
        EffectKind::TapeStop,
        0.5,
        Division::Half,
        (1.0, 3.0),
    );
}

#[test]
fn gate() {
    render(
        "gate",
        EffectKind::Gate,
        0.4,
        Division::ThirtySecond,
        (0.5, 3.5),
    );
}

#[test]
fn filter_sweep() {
    for sample_rate in SAMPLE_RATES {
        let mut fx = PerformanceFx::new();
        fx.init(sample_rate as f32);
        fx.set_tempo(TEMPO_BPM);

        let slot = fx.add_kind(EffectKind::FilterSweep);
        fx.effect_mut::<FilterSweep>(slot)
            .unwrap()
            .set_resonance(0.6);

        let source = source(sample_rate, DURATION);
        let mut wav_data = Vec::with_capacity(source.len());
        let mut block = [0.0; BLOCK_SIZE];
        let mut position = 0;

        fx.engage(slot);

        while position + BLOCK_SIZE <= source.len() {
            block.copy_from_slice(&source[position..position + BLOCK_SIZE]);

            // Sweep the filter from fully dark to fully thin over the whole test.
            fx.set_amount(slot, position as f32 / source.len() as f32);

            fx.process(&mut block);
            wav_data.extend_from_slice(&block);
            position += BLOCK_SIZE;
        }

        let filename = format!("performance/filter_sweep/filter_sweep_{sample_rate}.wav");
        write_wav(filename, &wav_data, sample_rate).ok();
    }
}

#[test]
fn crush() {
    render(
        "crush",
        EffectKind::Crush,
        0.7,
        Division::Eighth,
        (1.0, 3.0),
    );
}

#[test]
fn delay() {
    render(
        "delay",
        EffectKind::Delay,
        0.7,
        Division::DottedEighth,
        (1.0, 2.0),
    );
}

#[test]
fn chain() {
    for sample_rate in SAMPLE_RATES {
        let mut fx = PerformanceFx::new();
        fx.init(sample_rate as f32);
        fx.set_tempo(TEMPO_BPM);

        let repeat = fx.add_kind(EffectKind::BeatRepeat);
        fx.set_amount(repeat, 0.0);
        fx.set_division(repeat, Division::Sixteenth);

        let filter = fx.add_kind(EffectKind::FilterSweep);
        fx.set_amount(filter, 0.25);

        let delay = fx.add_kind(EffectKind::Delay);
        fx.set_amount(delay, 0.6);
        fx.set_mix(delay, 0.5);
        fx.set_division(delay, Division::DottedEighth);

        let source = source(sample_rate, DURATION);
        let mut wav_data = Vec::with_capacity(source.len());
        let mut block = [0.0; BLOCK_SIZE];
        let mut position = 0;

        while position + BLOCK_SIZE <= source.len() {
            block.copy_from_slice(&source[position..position + BLOCK_SIZE]);

            let time = position as f32 / sample_rate as f32;

            if (1.0..2.0).contains(&time) {
                fx.engage(repeat);
            } else {
                fx.release(repeat);
            }

            if (1.5..3.0).contains(&time) {
                fx.engage(filter);
            } else {
                fx.release(filter);
            }

            if (2.0..3.0).contains(&time) {
                fx.engage(delay);
            } else {
                fx.release(delay);
            }

            fx.process(&mut block);
            wav_data.extend_from_slice(&block);
            position += BLOCK_SIZE;
        }

        let filename = format!("performance/chain/chain_{sample_rate}.wav");
        write_wav(filename, &wav_data, sample_rate).ok();
    }
}

#[test]
fn beat_repeat_loops_the_captured_slice() {
    const SAMPLE_RATE: f32 = 48000.0;

    let mut transport = Transport::new();
    transport.init(SAMPLE_RATE);
    transport.set_tempo(TEMPO_BPM);

    let parameters = EffectParameters {
        amount: 0.0,
        mix: 1.0,
        division: Division::Sixteenth,
    };

    let loop_length = transport.samples_for(parameters.division);

    let mut effect = BeatRepeat::new();
    effect.init(SAMPLE_RATE);

    // Fill the buffer with a ramp so that every sample is unique, and stop right on a
    // grid line.
    let mut block = [0.0; BLOCK_SIZE];
    let mut sample_index = 0;

    while sample_index < loop_length * 4 {
        for sample in block.iter_mut() {
            *sample = (sample_index % loop_length) as f32 / loop_length as f32;
            sample_index += 1;
        }
        effect.process(&parameters, &transport, &mut block);
        transport.advance(BLOCK_SIZE);
    }

    assert_eq!(transport.samples_since_grid(parameters.division), 0);

    effect.engage(&parameters, &transport);
    assert_eq!(effect.slice_samples(), loop_length);

    // Feed silence and check that the last recorded slice comes back unchanged, twice.
    let mut output = Vec::new();

    while output.len() < loop_length * 2 {
        block.fill(0.0);
        effect.process(&parameters, &transport, &mut block);
        transport.advance(BLOCK_SIZE);
        output.extend_from_slice(&block);
    }

    for n in 0..loop_length {
        let expected = n as f32 / loop_length as f32;
        assert!(
            (output[n] - expected).abs() < 1.0e-6,
            "sample {n} of the first repetition: {} != {expected}",
            output[n]
        );
        assert!(
            (output[n + loop_length] - expected).abs() < 1.0e-6,
            "sample {n} of the second repetition: {} != {expected}",
            output[n + loop_length]
        );
    }
}

#[test]
fn tape_stop_comes_to_a_halt() {
    const SAMPLE_RATE: f32 = 48000.0;

    let mut transport = Transport::new();
    transport.init(SAMPLE_RATE);
    transport.set_tempo(TEMPO_BPM);

    let parameters = EffectParameters {
        amount: 0.5,
        mix: 1.0,
        division: Division::Quarter,
    };

    let mut effect = TapeStop::new();
    effect.init(SAMPLE_RATE);
    effect.engage(&parameters, &transport);

    let mut block = [0.0; BLOCK_SIZE];
    let stop_samples = transport.samples_for(parameters.division);
    let mut peak = 0.0_f32;
    let mut sample_index = 0;

    while sample_index < stop_samples * 2 {
        block.fill(1.0);
        effect.process(&parameters, &transport, &mut block);
        transport.advance(BLOCK_SIZE);
        sample_index += BLOCK_SIZE;

        if sample_index > stop_samples {
            peak = peak.max(block.iter().fold(0.0_f32, |a, b| a.max(b.abs())));
        }
    }

    assert!(effect.speed() < 1.0e-3, "the tape did not stop");
    assert!(peak < 1.0e-3, "the output did not fade out: {peak}");

    // Releasing spins the tape back up and catches up with the input again.
    effect.release(&parameters, &transport);
    assert!(effect.is_active());

    let mut blocks = 0;

    while effect.is_active() && blocks < 100000 / BLOCK_SIZE {
        block.fill(1.0);
        effect.process(&parameters, &transport, &mut block);
        transport.advance(BLOCK_SIZE);
        blocks += 1;
    }

    assert!(!effect.is_active(), "the tape never caught up");
}

#[test]
fn gate_follows_the_grid() {
    const SAMPLE_RATE: f32 = 48000.0;

    let mut transport = Transport::new();
    transport.init(SAMPLE_RATE);
    transport.set_tempo(TEMPO_BPM);

    let parameters = EffectParameters {
        amount: 0.5,
        mix: 1.0,
        division: Division::Sixteenth,
    };

    let length = transport.samples_for(parameters.division);

    let mut effect = Gate::new();
    effect.init(SAMPLE_RATE);
    effect.engage(&parameters, &transport);

    let mut output = Vec::new();
    let mut block = [0.0; BLOCK_SIZE];

    while output.len() < length {
        block.fill(1.0);
        effect.process(&parameters, &transport, &mut block);
        transport.advance(BLOCK_SIZE);
        output.extend_from_slice(&block);
    }

    // Open at the start of the division, closed towards its end.
    assert!(output[length / 4] > 0.9, "the gate did not open");
    assert!(output[length - 10] < 0.1, "the gate did not close");
}
