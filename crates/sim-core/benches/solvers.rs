//! Criterion benchmarks for the dense DC, transient, and AC solvers.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use sim_core::netlist::{
    ComponentId, ComponentKind, Netlist, NetlistBuilder, NodeId, NodeKind, ParamMap, ParamValue,
};
use sim_core::{
    simulate_transient, solve_ac, solve_dc, AcExcitation, AcOptions, AcSweep, DcOptions,
    TransientAction, TransientEvent, TransientOptions,
};

fn params(entries: &[(&str, ParamValue)]) -> ParamMap {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn number(key: &str, value: f64) -> ParamMap {
    params(&[(key, ParamValue::Number(value))])
}

fn divider() -> Netlist {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number("dc", 10.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number("resistance", 1_000.0),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R2",
            number("resistance", 2_000.0),
            vec![output, ground],
        )
        .unwrap();
    builder.build().unwrap()
}

fn rc_low_pass() -> (Netlist, ComponentId, NodeId) {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number("dc", 0.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number("resistance", 1_000.0),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C1",
            number("capacitance", 1e-6),
            vec![output, ground],
        )
        .unwrap();
    (builder.build().unwrap(), source, output)
}

fn benchmark_solvers(criterion: &mut Criterion) {
    let divider = divider();
    criterion.bench_function("dc/resistive_divider", |bencher| {
        bencher.iter(|| {
            let result = solve_dc(black_box(&divider), DcOptions::default()).unwrap();
            black_box(result);
        });
    });

    let (transient_netlist, transient_source, _) = rc_low_pass();
    let transient_options = TransientOptions {
        start_time: 0.0,
        stop_time: 5e-3,
        step: 1e-5,
        max_steps: 600,
        solver: DcOptions::default(),
        events: vec![TransientEvent {
            time: 0.0,
            action: TransientAction::SetSourceValue {
                component: transient_source,
                value: 5.0,
            },
        }],
    };
    criterion.bench_function("transient/rc_501_samples", |bencher| {
        bencher.iter(|| {
            let result = simulate_transient(
                black_box(&transient_netlist),
                black_box(transient_options.clone()),
            )
            .unwrap();
            black_box(result);
        });
    });

    let (ac_netlist, ac_source, _) = rc_low_pass();
    let ac_options = AcOptions {
        sweep: AcSweep::Logarithmic {
            start: 1.0,
            stop: 1e6,
            points: 101,
        },
        excitations: vec![AcExcitation {
            component: ac_source,
            magnitude: 1.0,
            phase_degrees: 0.0,
        }],
        solver: DcOptions::default(),
    };
    criterion.bench_function("ac/rc_101_points", |bencher| {
        bencher.iter(|| {
            let result = solve_ac(black_box(&ac_netlist), black_box(ac_options.clone())).unwrap();
            black_box(result);
        });
    });
}

criterion_group!(benches, benchmark_solvers);
criterion_main!(benches);
