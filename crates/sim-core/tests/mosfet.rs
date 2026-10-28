//! Deterministic acceptance tests for the square-law level-1 MOSFET model.

use sim_core::netlist::{
    ComponentId, ComponentKind, Netlist, NetlistBuilder, NodeId, NodeKind, ParamMap, ParamValue,
};
use sim_core::{
    simulate_transient, solve_ac, solve_dc, AcExcitation, AcOptions, AcSweep, DcOptions,
    NewtonStrategy, TransientAction, TransientEvent, TransientOptions, TransientSession,
};

const VTH: f64 = 1.6;
const K: f64 = 0.05;
const LAMBDA: f64 = 0.02;
const THERMAL_VOLTAGE: f64 = 1.380_649e-23 * 300.15 / 1.602_176_634e-19;

fn params(entries: &[(&str, ParamValue)]) -> ParamMap {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn mosfet_params(channel: &str, swap_ds: bool) -> ParamMap {
    params(&[
        ("channel", ParamValue::Text(channel.to_owned())),
        ("vth", ParamValue::Number(VTH)),
        ("k", ParamValue::Number(K)),
        ("lambda", ParamValue::Number(LAMBDA)),
        ("rd", ParamValue::Number(0.0)),
        ("rs", ParamValue::Number(0.0)),
        ("is", ParamValue::Number(1e-14)),
        ("n", ParamValue::Number(1.0)),
        ("cgs", ParamValue::Number(0.0)),
        ("cgd", ParamValue::Number(0.0)),
        ("cds", ParamValue::Number(0.0)),
        ("imax", ParamValue::Number(0.3)),
        ("swap_ds", ParamValue::Bool(swap_ds)),
    ])
}

fn solver() -> DcOptions {
    DcOptions {
        max_iterations: 100,
        abs_tolerance: 1e-14,
        rel_tolerance: 1e-12,
        residual_tolerance: 1e-13,
        strategy: NewtonStrategy::Auto,
    }
}

fn voltage(value: f64) -> ParamMap {
    params(&[("dc", ParamValue::Number(value))])
}

/// Ideal-bias test bench: `VD` (drain to ground), `VG` (gate to ground), and
/// `VS` (source to ground) hold the three terminals at the requested voltages.
struct Bench {
    netlist: Netlist,
    drain_source: ComponentId,
}

fn bias_bench(channel: &str, vd: f64, vg: f64, vs: f64, swap_ds: bool) -> Bench {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let drain = builder.add_node("d", NodeKind::Electrical).unwrap();
    let gate = builder.add_node("g", NodeKind::Electrical).unwrap();
    let source = builder.add_node("s", NodeKind::Electrical).unwrap();
    let drain_source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "VD",
            voltage(vd),
            vec![drain, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "VG",
            voltage(vg),
            vec![gate, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "VS",
            voltage(vs),
            vec![source, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Mosfet,
            "M1",
            mosfet_params(channel, swap_ds),
            vec![drain, gate, source],
        )
        .unwrap();
    Bench {
        netlist: builder.build().unwrap(),
        drain_source,
    }
}

fn drain_current(bench: &Bench) -> f64 {
    let result = solve_dc(&bench.netlist, solver()).unwrap();
    -result.branch_current(bench.drain_source).unwrap()
}

/// Independent analytic reference for the documented model.
fn analytic_drain_current(
    channel: &str,
    vg: f64,
    vd: f64,
    vs: f64,
    vth: f64,
    k: f64,
    lambda: f64,
) -> f64 {
    let polarity = if channel == "n" { 1.0 } else { -1.0 };
    let vgs = polarity * (vg - vs);
    let vds = polarity * (vd - vs);
    let forward = |vgs: f64, vds: f64| {
        let overdrive = vgs - vth;
        if overdrive <= 0.0 {
            0.0
        } else {
            let modulation = 1.0 + lambda * vds;
            if vds < overdrive {
                k * (2.0 * overdrive * vds - vds * vds) * modulation
            } else {
                k * overdrive * overdrive * modulation
            }
        }
    };
    let channel_current = if vds >= 0.0 {
        forward(vgs, vds)
    } else {
        -forward(vgs - vds, -vds)
    };
    polarity * channel_current
}

fn relative_error(actual: f64, expected: f64) -> f64 {
    (actual - expected).abs() / expected.abs().max(1e-30)
}

#[test]
fn mosfet_cutoff_current_is_below_one_nanoamp() {
    for channel in ["n", "p"] {
        let (vd, vg, vs) = if channel == "n" {
            (5.0, 0.0, 0.0)
        } else {
            (-5.0, 0.0, 0.0)
        };
        let bench = bias_bench(channel, vd, vg, vs, false);
        let current = drain_current(&bench);
        assert!(
            current.abs() < 1e-9,
            "{channel} channel cutoff current {current:e} A"
        );
    }
}

#[test]
fn mosfet_triode_and_saturation_match_the_square_law() {
    for channel in ["n", "p"] {
        let polarity = if channel == "n" { 1.0 } else { -1.0 };
        let vg = polarity * 3.0;
        for vd in [polarity * 0.5, polarity * 3.0] {
            let bench = bias_bench(channel, vd, vg, 0.0, false);
            let current = drain_current(&bench);
            let expected = analytic_drain_current(channel, vg, vd, 0.0, VTH, K, LAMBDA);
            assert!(
                relative_error(current, expected) < 1e-9,
                "{channel} channel at Vds={vd}: solved {current:e} A, analytic {expected:e} A"
            );
        }
    }
}

#[test]
fn mosfet_drain_current_is_continuous_at_the_saturation_boundary() {
    let overdrive = 3.0 - VTH;
    let below = bias_bench("n", overdrive * (1.0 - 1e-9), 3.0, 0.0, false);
    let above = bias_bench("n", overdrive * (1.0 + 1e-9), 3.0, 0.0, false);
    let below_current = drain_current(&below);
    let above_current = drain_current(&above);
    let expected = analytic_drain_current("n", 3.0, overdrive, 0.0, VTH, K, LAMBDA);
    assert!(relative_error(below_current, expected) < 1e-9);
    assert!(relative_error(above_current, expected) < 1e-9);
    assert!(
        relative_error(above_current, below_current) < 1e-6,
        "current jumps from {below_current:e} to {above_current:e} A"
    );
}

#[test]
fn mosfet_n_and_p_channels_are_mirror_symmetric() {
    let n = bias_bench("n", 3.0, 3.0, 0.0, false);
    let p = bias_bench("p", -3.0, -3.0, 0.0, false);
    let n_current = drain_current(&n);
    let p_current = drain_current(&p);
    assert!(n_current > 0.0);
    assert!(p_current < 0.0);
    assert!(relative_error(n_current, -p_current) < 1e-9);
    assert!(relative_error(p_current, -n_current) < 1e-9);
}

#[test]
fn mosfet_swap_ds_reverses_conduction_with_the_same_magnitude() {
    // Forward conduction: D = 0.5 V, G = 3.0 V, S = 0 V.
    let forward = bias_bench("n", 0.5, 3.0, 0.0, false);
    // Swapped terminals with the gate still 3 V above the effective source:
    // the electrical source is now the pin named D.
    let reverse = bias_bench("n", 0.0, 3.0, 0.5, true);
    let forward_current = drain_current(&forward);
    let reverse_current = drain_current(&reverse);
    let expected = analytic_drain_current("n", 3.0, 0.5, 0.0, VTH, K, LAMBDA);
    assert!(relative_error(forward_current, expected) < 1e-9);
    assert!(
        relative_error(reverse_current, -expected) < 1e-9,
        "swapped current {reverse_current:e} A, expected {:e} A",
        -expected
    );
    assert!(relative_error(forward_current.abs(), reverse_current.abs()) < 1e-9);
}

#[test]
fn mosfet_swap_ds_flips_the_body_diode_direction() {
    // The same terminal voltages make the body diode conduct without swap and
    // block once the drain and source are permuted.
    let conducting = bias_bench("n", -0.8, 0.0, 0.0, false);
    let blocked = bias_bench("n", -0.8, 0.0, 0.0, true);
    let conducting_current = drain_current(&conducting);
    let blocked_current = drain_current(&blocked);
    assert!(
        conducting_current < -1e-3,
        "body diode should conduct, got {conducting_current:e} A"
    );
    assert!(
        blocked_current.abs() < 1e-9,
        "swapped body diode should block, got {blocked_current:e} A"
    );
}

#[test]
fn mosfet_body_diode_matches_the_existing_diode_model() {
    // Every PN junction carries the same permanent shunt as `mna.rs`.
    const JUNCTION_GMIN: f64 = 1e-12;
    for channel in ["n", "p"] {
        let polarity = if channel == "n" { 1.0 } else { -1.0 };
        let vd = -polarity * 0.6;
        let bench = bias_bench(channel, vd, 0.0, 0.0, false);
        let current = drain_current(&bench);
        let diode_voltage = polarity * (0.0 - vd);
        let shockley = -polarity * 1e-14 * ((diode_voltage / THERMAL_VOLTAGE).exp() - 1.0);
        let expected_current = shockley - polarity * JUNCTION_GMIN * diode_voltage;
        assert!(
            relative_error(current, expected_current) < 1e-9,
            "{channel} body diode: solved {current:e} A, analytic {expected_current:e} A"
        );
    }
}

/// AC bench: ideal gate and drain sources hold the operating point while one
/// source carries the small-signal excitation.
struct AcBench {
    netlist: Netlist,
    gate_source: ComponentId,
    drain_source: ComponentId,
}

fn ac_bench(channel: &str, vd: f64, vg: f64, vds_capacitance: bool) -> AcBench {
    let mut parameters = mosfet_params(channel, false);
    if vds_capacitance {
        parameters.insert("cgs".to_owned(), ParamValue::Number(1e-9));
        parameters.insert("cgd".to_owned(), ParamValue::Number(1e-9));
        parameters.insert("cds".to_owned(), ParamValue::Number(1e-9));
    }
    ac_bench_with(vd, vg, parameters)
}

fn ac_bench_with(vd: f64, vg: f64, parameters: ParamMap) -> AcBench {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let drain = builder.add_node("d", NodeKind::Electrical).unwrap();
    let gate = builder.add_node("g", NodeKind::Electrical).unwrap();
    let source = builder.add_node("s", NodeKind::Electrical).unwrap();
    let drain_source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "VD",
            voltage(vd),
            vec![drain, ground],
        )
        .unwrap();
    let gate_source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "VG",
            voltage(vg),
            vec![gate, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "VS",
            voltage(0.0),
            vec![source, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Mosfet,
            "M1",
            parameters,
            vec![drain, gate, source],
        )
        .unwrap();
    AcBench {
        netlist: builder.build().unwrap(),
        gate_source,
        drain_source,
    }
}

fn ac_branch_current(bench: &AcBench, excitation: ComponentId, measured: ComponentId) -> f64 {
    let options = AcOptions {
        sweep: AcSweep::Explicit(vec![1_000.0]),
        excitations: vec![AcExcitation {
            component: excitation,
            magnitude: 1.0,
            phase_degrees: 0.0,
        }],
        solver: solver(),
    };
    let result = solve_ac(&bench.netlist, options).unwrap();
    let sample = &result.samples[0];
    sample.branch_current(measured).unwrap().magnitude()
}

#[test]
fn mosfet_ac_gm_matches_the_dc_finite_difference() {
    for channel in ["n", "p"] {
        let polarity = if channel == "n" { 1.0 } else { -1.0 };
        let (vd, vg) = (polarity * 3.0, polarity * 3.0);
        let bench = ac_bench(channel, vd, vg, false);
        // A 1 V gate excitation produces the small-signal drain current.
        let measured = ac_branch_current(&bench, bench.gate_source, bench.drain_source);

        let step = 1e-3;
        let above = drain_current(&bias_bench(channel, vd, vg + polarity * step, 0.0, false));
        let below = drain_current(&bias_bench(channel, vd, vg - polarity * step, 0.0, false));
        let finite_difference = polarity * (above - below) / (2.0 * step);
        assert!(finite_difference > 0.0);
        assert!(
            relative_error(measured, finite_difference) < 0.01,
            "{channel} gm: AC {measured:e} S, finite difference {finite_difference:e} S"
        );
    }
}

#[test]
fn mosfet_ac_gds_matches_the_dc_finite_difference() {
    for channel in ["n", "p"] {
        let polarity = if channel == "n" { 1.0 } else { -1.0 };
        let (vd, vg) = (polarity * 3.0, polarity * 3.0);
        let bench = ac_bench(channel, vd, vg, false);
        // A 1 V drain excitation produces the small-signal drain current.
        let measured = ac_branch_current(&bench, bench.drain_source, bench.drain_source);

        let step = 1e-3;
        let above = drain_current(&bias_bench(channel, vd + polarity * step, vg, 0.0, false));
        let below = drain_current(&bias_bench(channel, vd - polarity * step, vg, 0.0, false));
        let finite_difference = polarity * (above - below) / (2.0 * step);
        assert!(finite_difference > 0.0);
        assert!(
            relative_error(measured, finite_difference) < 0.01,
            "{channel} gds: AC {measured:e} S, finite difference {finite_difference:e} S"
        );
    }
}

#[test]
fn mosfet_ac_capacitances_load_the_terminals() {
    let without = ac_bench("n", 3.0, 3.0, false);
    let with = ac_bench("n", 3.0, 3.0, true);
    let base = ac_branch_current(&without, without.gate_source, without.gate_source);
    let loaded = ac_branch_current(&with, with.gate_source, with.gate_source);
    assert!(base.abs() < 1e-12, "the bare gate draws no current");
    let omega = std::f64::consts::TAU * 1_000.0;
    let expected = omega * 2e-9;
    assert!(
        relative_error(loaded, expected) < 1e-9,
        "gate admittance {loaded:e} S, expected {expected:e} S"
    );
    // Gate-drain capacitance couples the drain excitation back into the gate.
    let coupled = ac_branch_current(&with, with.drain_source, with.gate_source);
    assert!(
        relative_error(coupled, omega * 1e-9) < 1e-9,
        "gate-drain admittance {coupled:e} S"
    );
    // With no channel conductance, the drain admittance is purely `cds`.
    let mut parameters = mosfet_params("n", false);
    parameters.insert("lambda".to_owned(), ParamValue::Number(0.0));
    parameters.insert("cds".to_owned(), ParamValue::Number(1e-9));
    let drain_only = ac_bench_with(3.0, 3.0, parameters);
    let drain = ac_branch_current(
        &drain_only,
        drain_only.drain_source,
        drain_only.drain_source,
    );
    assert!(
        relative_error(drain, omega * 1e-9) < 1e-9,
        "drain admittance {drain:e} S"
    );
}

/// Resistive-load bench used by the transient tests.
fn switching_bench() -> (Netlist, ComponentId, NodeId, NodeId) {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let supply = builder.add_node("vdd", NodeKind::Electrical).unwrap();
    let drain = builder.add_node("d", NodeKind::Electrical).unwrap();
    let gate = builder.add_node("g", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "VDD",
            voltage(10.0),
            vec![supply, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "RD",
            params(&[("resistance", ParamValue::Number(1_000.0))]),
            vec![supply, drain],
        )
        .unwrap();
    let gate_source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "VG",
            voltage(0.0),
            vec![gate, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Mosfet,
            "M1",
            mosfet_params("n", false),
            vec![drain, gate, ground],
        )
        .unwrap();
    (builder.build().unwrap(), gate_source, drain, gate)
}

fn gate_step_result(step: f64) -> sim_core::TransientResult {
    let (netlist, gate_source, _, _) = switching_bench();
    let options = TransientOptions {
        start_time: 0.0,
        stop_time: 1.5e-3,
        step,
        max_steps: 1_000_000,
        solver: solver(),
        events: vec![TransientEvent {
            time: 1e-3,
            action: TransientAction::SetSourceValue {
                component: gate_source,
                value: 5.0,
            },
        }],
    };
    simulate_transient(&netlist, options).unwrap()
}

#[test]
fn mosfet_gate_step_converges_at_ten_microseconds() {
    let result = gate_step_result(10e-6);
    assert!(result.steps > 100);
    let last = result.samples.last().unwrap();
    assert!((last.node_voltage(sim_node("g")).unwrap() - 5.0).abs() < 1e-9);
    let drain = last.node_voltage(sim_node("d")).unwrap();
    assert!(drain > 0.0 && drain < 10.0);
    // The drain voltage reaches the static operating point at Vgs = 5 V.
    let (netlist, gate_source, _, _) = switching_bench();
    let mut with_gate_on = netlist.clone();
    with_gate_on.components[usize::try_from(gate_source.0).unwrap()]
        .params
        .insert("dc".to_owned(), ParamValue::Number(5.0));
    let dc = solve_dc(&with_gate_on, solver()).unwrap();
    assert!(
        (drain - dc.node_voltage(sim_node("d")).unwrap()).abs() < 1e-6,
        "transient drain {drain} V, DC {} V",
        dc.node_voltage(sim_node("d")).unwrap()
    );
}

#[test]
fn mosfet_gate_step_converges_at_the_minimal_live_step() {
    let result = gate_step_result(1e-6);
    assert!(result.steps > 1_000);
    let last = result.samples.last().unwrap();
    assert!((last.node_voltage(sim_node("g")).unwrap() - 5.0).abs() < 1e-9);
}

/// Low-side driver with an LED load, mirroring the documented example.
struct LedSwitchBench {
    netlist: Netlist,
    switch: ComponentId,
    gate: NodeId,
    out: NodeId,
    drain: NodeId,
}

fn led_switch_bench() -> LedSwitchBench {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let supply = builder.add_node("vdd", NodeKind::Electrical).unwrap();
    let out = builder.add_node("out", NodeKind::Electrical).unwrap();
    let drain = builder.add_node("d", NodeKind::Electrical).unwrap();
    let gate = builder.add_node("g", NodeKind::Electrical).unwrap();
    let drive = builder.add_node("drive", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            voltage(10.0),
            vec![supply, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            params(&[("resistance", ParamValue::Number(1_000.0))]),
            vec![supply, out],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Led,
            "LED1",
            params(&[
                ("vf", ParamValue::Number(2.0)),
                ("if", ParamValue::Number(0.02)),
                ("imax", ParamValue::Number(0.03)),
            ]),
            vec![out, drain],
        )
        .unwrap();
    let mut mosfet = mosfet_params("n", false);
    mosfet.insert("rd".to_owned(), ParamValue::Number(1.0));
    mosfet.insert("rs".to_owned(), ParamValue::Number(2.0));
    builder
        .add_component(
            ComponentKind::Mosfet,
            "M1",
            mosfet,
            vec![drain, gate, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V2",
            voltage(5.0),
            vec![drive, ground],
        )
        .unwrap();
    let switch = builder
        .add_component(
            ComponentKind::Switch,
            "SW1",
            params(&[
                ("ron", ParamValue::Number(1e-3)),
                ("roff", ParamValue::Number(1e18)),
                ("closed", ParamValue::Bool(true)),
            ]),
            vec![drive, gate],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R2",
            params(&[("resistance", ParamValue::Number(100_000.0))]),
            vec![gate, ground],
        )
        .unwrap();
    LedSwitchBench {
        netlist: builder.build().unwrap(),
        switch,
        gate,
        out,
        drain,
    }
}

#[test]
fn opening_the_gate_switch_keeps_the_floating_drain_solvable() {
    let bench = led_switch_bench();
    let options = TransientOptions {
        step: 1e-6,
        stop_time: 1.0,
        solver: DcOptions::default(),
        ..TransientOptions::default()
    };
    let mut session = TransientSession::new(&bench.netlist, &options).unwrap();
    for _ in 0..1_000 {
        session.advance().expect("closed switch advances");
    }

    let mut open = bench.netlist.clone();
    open.components[usize::try_from(bench.switch.0).unwrap()]
        .params
        .insert("closed".to_owned(), ParamValue::Bool(false));
    session
        .update_netlist(&open)
        .expect("switch parameter updates");

    // The drain island now only touches reverse-biased junctions, so the hot
    // update must still converge instead of stalling on a near-singular Jacobian.
    for step in 0..5 {
        session
            .advance()
            .unwrap_or_else(|error| panic!("hot step {step} failed: {error}"));
    }

    let sample = session.snapshot();
    assert!(
        sample.node_voltage(bench.gate).unwrap().abs() < 1e-6,
        "gate must fall to 0 V, got {:?}",
        sample.node_voltage(bench.gate)
    );
    assert!(
        sample.node_voltage(bench.out).unwrap() > 9.9,
        "the load must see nearly the full rail, got {:?}",
        sample.node_voltage(bench.out)
    );
    let drain = sample.node_voltage(bench.drain).unwrap();
    assert!(
        drain > 0.0 && drain < 10.0,
        "the floating drain must stay bounded, got {drain} V"
    );
}

fn sim_node(name: &str) -> NodeId {
    // Nodes are created in a fixed order in `switching_bench`.
    match name {
        "0" => NodeId(0),
        "vdd" => NodeId(1),
        "d" => NodeId(2),
        "g" => NodeId(3),
        other => panic!("unknown node {other}"),
    }
}
