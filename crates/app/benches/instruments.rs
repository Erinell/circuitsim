//! Criterion benchmarks for instrument FFT and CSV serialization.

use std::hint::black_box;

use circuitsim_app::analysis::{
    bode_csv, compute_spectrum, transient_csv, BodePoint, BodeTrace, SignalId, Trace,
};
use criterion::{criterion_group, criterion_main, Criterion};

fn sine_trace() -> Trace {
    let points = (0..=16_384)
        .map(|index| {
            let time = index as f64 / 16_384.0;
            let value = 2.0 * (std::f64::consts::TAU * 440.0 * time).sin();
            [time, value]
        })
        .collect();
    Trace {
        signal: SignalId::Voltage("OUT".to_owned()),
        points,
    }
}

fn transient_traces() -> Vec<Trace> {
    let points = (0..10_000)
        .map(|index| {
            let time = index as f64 * 1e-5;
            [time, 5.0 * (1.0 - (-time / 1e-3).exp())]
        })
        .collect::<Vec<_>>();
    vec![
        Trace {
            signal: SignalId::Voltage("OUT".to_owned()),
            points: points.clone(),
        },
        Trace {
            signal: SignalId::Current("V1".to_owned()),
            points,
        },
    ]
}

fn bode_trace() -> BodeTrace {
    let points = (0..101)
        .map(|index| {
            let frequency = 10f64.powf(index as f64 / 20.0);
            BodePoint {
                frequency,
                magnitude: 1.0 / (1.0 + frequency / 159.154_943).sqrt(),
                gain_db: -10.0 * (1.0 + (frequency / 159.154_943).powi(2)).log10(),
                phase_degrees: -(frequency / 159.154_943).atan().to_degrees(),
            }
        })
        .collect();
    BodeTrace {
        signal: SignalId::Voltage("OUT".to_owned()),
        points,
    }
}

fn benchmark_instruments(criterion: &mut Criterion) {
    let trace = sine_trace();
    criterion.bench_function("fft/16384_hann_bins", |bencher| {
        bencher.iter(|| {
            let spectrum = compute_spectrum(black_box(&trace), 1.0 / 16_384.0).unwrap();
            black_box(spectrum);
        });
    });

    let traces = transient_traces();
    criterion.bench_function("csv/transient_2x10000", |bencher| {
        bencher.iter(|| {
            let csv = transient_csv(black_box(&traces));
            black_box(csv);
        });
    });

    let bode = bode_trace();
    criterion.bench_function("csv/bode_101_points", |bencher| {
        bencher.iter(|| {
            let csv = bode_csv(black_box(&bode));
            black_box(csv);
        });
    });
}

criterion_group!(benches, benchmark_instruments);
criterion_main!(benches);
