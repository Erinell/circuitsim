use std::f64::consts::{FRAC_1_SQRT_2, PI};

use sim_core::netlist::{
    ComponentId, ComponentKind, Netlist, NetlistBuilder, NodeId, NodeKind, ParamMap, ParamValue,
};
use sim_core::{solve_ac, AcError, AcExcitation, AcOptions, AcSweep, DcError, DcOptions, Phasor};

fn params(entries: &[(&str, ParamValue)]) -> ParamMap {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn number(key: &str, value: f64) -> ParamMap {
    params(&[(key, ParamValue::Number(value))])
}

fn diode_params() -> ParamMap {
    params(&[
        ("is", ParamValue::Number(1e-14)),
        ("n", ParamValue::Number(1.0)),
    ])
}

fn opamp_params() -> ParamMap {
    params(&[
        ("gain", ParamValue::Number(1e5)),
        ("v_sat_pos", ParamValue::Number(15.0)),
        ("v_sat_neg", ParamValue::Number(-15.0)),
    ])
}

fn shunt_reference_params() -> ParamMap {
    params(&[
        ("vref", ParamValue::Number(2.495)),
        ("iref", ParamValue::Number(2e-6)),
        ("ik_min", ParamValue::Number(1e-3)),
        ("imax", ParamValue::Number(0.1)),
        ("z_ka", ParamValue::Number(0.2)),
    ])
}

fn comparator_params() -> ParamMap {
    params(&[
        ("v_offset", ParamValue::Number(0.0)),
        ("r_sink", ParamValue::Number(20.0)),
        ("imax", ParamValue::Number(0.02)),
    ])
}

fn excitation(component: ComponentId, magnitude: f64, phase: f64) -> AcExcitation {
    AcExcitation {
        component,
        magnitude,
        phase_degrees: phase,
    }
}

fn explicit_options(frequency: f64, excitations: Vec<AcExcitation>) -> AcOptions {
    AcOptions {
        sweep: AcSweep::Explicit(vec![frequency]),
        excitations,
        solver: DcOptions::default(),
    }
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual}"
    );
}

fn assert_phasor_close(actual: Phasor, expected: Phasor, tolerance: f64) {
    assert_close(actual.re, expected.re, tolerance);
    assert_close(actual.im, expected.im, tolerance);
}

#[test]
fn shunt_reference_ac_uses_the_dc_control_slope() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let source_node = builder.add_node("source", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number("dc", 5.0),
            vec![source_node, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "Rseries",
            number("resistance", 100.0),
            vec![source_node, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "Rload",
            number("resistance", 1_000.0),
            vec![output, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::ShuntReference,
            "U1",
            shunt_reference_params(),
            vec![output, output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_ac(
        &netlist,
        explicit_options(1_000.0, vec![excitation(source, 1.0, 0.0)]),
    )
    .unwrap();

    let dc_voltage = result.operating_point.node_voltage(output).unwrap();
    let response = result.samples[0].node_voltage(output).unwrap().magnitude();
    assert!((2.49..2.53).contains(&dc_voltage));
    assert!(
        (0.0018..0.0022).contains(&response),
        "AC magnitude was {response}"
    );
}

#[test]
fn comparator_ac_linearizes_its_active_pull_down_resistance() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let vcc = builder.add_node("vcc", NodeKind::Electrical).unwrap();
    let input_plus = builder.add_node("in_plus", NodeKind::Electrical).unwrap();
    let input_minus = builder.add_node("in_minus", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let supply = builder
        .add_component(
            ComponentKind::VoltageSource,
            "VCC",
            number("dc", 5.0),
            vec![vcc, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "VINP",
            number("dc", 1.0),
            vec![input_plus, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "VINM",
            number("dc", 2.0),
            vec![input_minus, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "RPU",
            number("resistance", 1_000.0),
            vec![vcc, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Comparator,
            "U1",
            comparator_params(),
            vec![input_plus, input_minus, output, vcc, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_ac(
        &netlist,
        explicit_options(1_000.0, vec![excitation(supply, 1.0, 0.0)]),
    )
    .unwrap();

    let output_dc = result.operating_point.node_voltage(output).unwrap();
    let output_ac = result.samples[0].node_voltage(output).unwrap().magnitude();
    assert!((0.09..0.11).contains(&output_dc));
    assert!(
        (0.019..0.020).contains(&output_ac),
        "AC magnitude was {output_ac}"
    );
}

fn rc_low_pass_netlist() -> (Netlist, ComponentId, NodeId, NodeId) {
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
    (builder.build().unwrap(), source, output, ground)
}

#[test]
fn rc_low_pass_matches_the_analytic_response() {
    let (netlist, source, output, _) = rc_low_pass_netlist();
    let cutoff = 1.0 / (2.0 * PI * 1_000.0 * 1e-6);
    let result = solve_ac(
        &netlist,
        explicit_options(cutoff, vec![excitation(source, 1.0, 0.0)]),
    )
    .unwrap();
    let sample = &result.samples[0];
    let output_voltage = sample.node_voltage(output).unwrap();

    assert_close(output_voltage.magnitude(), FRAC_1_SQRT_2, 1e-12);
    assert_close(output_voltage.phase_degrees(), -45.0, 1e-10);
    assert_phasor_close(
        sample.branch_current(source).unwrap(),
        Phasor::new(-0.5e-3, -0.5e-3),
        1e-15,
    );
    assert_close(
        result.operating_point.node_voltage(output).unwrap(),
        0.0,
        1e-15,
    );
}

#[test]
fn rc_high_pass_matches_the_analytic_response() {
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
            ComponentKind::Capacitor,
            "C1",
            number("capacitance", 1e-6),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number("resistance", 1_000.0),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let cutoff = 1.0 / (2.0 * PI * 1_000.0 * 1e-6);
    let result = solve_ac(
        &netlist,
        explicit_options(cutoff, vec![excitation(source, 1.0, 0.0)]),
    )
    .unwrap();
    let output_voltage = result.samples[0].node_voltage(output).unwrap();

    assert_close(output_voltage.magnitude(), FRAC_1_SQRT_2, 1e-12);
    assert_close(output_voltage.phase_degrees(), 45.0, 1e-10);
}

#[test]
fn rl_low_pass_matches_the_analytic_response() {
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
            ComponentKind::Inductor,
            "L1",
            number("inductance", 1e-3),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number("resistance", 1_000.0),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let cutoff = 1_000.0 / (2.0 * PI * 1e-3);
    let result = solve_ac(
        &netlist,
        explicit_options(cutoff, vec![excitation(source, 1.0, 0.0)]),
    )
    .unwrap();
    let output_voltage = result.samples[0].node_voltage(output).unwrap();

    assert_close(output_voltage.magnitude(), FRAC_1_SQRT_2, 1e-12);
    assert_close(output_voltage.phase_degrees(), -45.0, 1e-10);
}

#[test]
fn source_phase_and_branch_sign_follow_the_conventions() {
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
            ComponentKind::Resistor,
            "R2",
            number("resistance", 1_000.0),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let result = solve_ac(
        &netlist,
        explicit_options(1_000.0, vec![excitation(source, 2.0, 90.0)]),
    )
    .unwrap();
    let sample = &result.samples[0];

    assert_phasor_close(
        sample.node_voltage(output).unwrap(),
        Phasor::new(0.0, 1.0),
        1e-12,
    );
    assert_phasor_close(
        sample.branch_current(source).unwrap(),
        Phasor::new(0.0, -1e-3),
        1e-15,
    );
}

#[test]
fn current_source_direction_matches_the_dc_convention() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::CurrentSource,
            "I1",
            number("dc", 0.0),
            vec![output, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number("resistance", 1_000.0),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let result = solve_ac(
        &netlist,
        explicit_options(1_000.0, vec![excitation(source, 1e-3, 0.0)]),
    )
    .unwrap();

    assert_phasor_close(
        result.samples[0].node_voltage(output).unwrap(),
        Phasor::new(-1.0, 0.0),
        1e-12,
    );
}

#[test]
fn parallel_rlc_reaches_its_resonance() {
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
            number("resistance", 10.0),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Inductor,
            "L1",
            number("inductance", 1e-3),
            vec![output, ground],
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
    let netlist = builder.build().unwrap();
    let resonance = 1.0 / (2.0 * PI * (1e-3_f64 * 1e-6).sqrt());
    let result = solve_ac(
        &netlist,
        explicit_options(resonance, vec![excitation(source, 1.0, 0.0)]),
    )
    .unwrap();
    let output_voltage = result.samples[0].node_voltage(output).unwrap();

    assert_close(output_voltage.magnitude(), 1.0, 1e-7);
    assert_close(output_voltage.phase_degrees(), 0.0, 1e-7);
}

#[test]
fn logarithmic_sweep_is_ordered_and_monotonic() {
    let (netlist, source, output, _) = rc_low_pass_netlist();
    let result = solve_ac(
        &netlist,
        AcOptions {
            sweep: AcSweep::Logarithmic {
                start: 10.0,
                stop: 100_000.0,
                points: 6,
            },
            excitations: vec![excitation(source, 1.0, 0.0)],
            solver: DcOptions::default(),
        },
    )
    .unwrap();

    assert_eq!(result.samples.len(), 6);
    assert_eq!(result.samples[0].frequency, 10.0);
    assert_eq!(result.samples[5].frequency, 100_000.0);
    for pair in result.samples.windows(2) {
        assert!(pair[1].frequency > pair[0].frequency);
        assert!(
            pair[1].node_voltage(output).unwrap().magnitude()
                < pair[0].node_voltage(output).unwrap().magnitude()
        );
    }
}

#[test]
fn diode_response_uses_the_small_signal_conductance_at_the_operating_point() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "VBIAS",
            number("dc", 0.7),
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
            ComponentKind::Diode,
            "D1",
            diode_params(),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let result = solve_ac(
        &netlist,
        explicit_options(1_000.0, vec![excitation(source, 1.0, 0.0)]),
    )
    .unwrap();

    let diode_voltage = result.operating_point.node_voltage(output).unwrap();
    let thermal = 1.380_649e-23 * 300.15 / 1.602_176_634e-19;
    let conductance = 1e-14 / thermal * (diode_voltage / thermal).exp();
    let expected = 1.0 / (1.0 + 1_000.0 * conductance);
    let actual = result.samples[0].node_voltage(output).unwrap();

    assert_close(actual.re, expected, 1e-9);
    assert_close(actual.im, 0.0, 1e-12);
}

#[test]
fn opamp_ac_gain_includes_finite_open_loop_gain() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let feedback = builder.add_node("fb", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "VIN",
            number("dc", 0.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::OpAmp,
            "U1",
            opamp_params(),
            vec![input, feedback, output, ground, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "RF",
            number("resistance", 10_000.0),
            vec![output, feedback],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "RG",
            number("resistance", 1_000.0),
            vec![feedback, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let result = solve_ac(
        &netlist,
        explicit_options(1_000.0, vec![excitation(source, 1.0, 0.0)]),
    )
    .unwrap();
    let output_voltage = result.samples[0].node_voltage(output).unwrap();
    let expected = 1e5 / (1.0 + 1e5 / 11.0);

    assert_close(output_voltage.re, expected, 1e-9);
    assert_close(output_voltage.im, 0.0, 1e-9);
}

#[test]
fn absent_excitations_produce_a_zero_response() {
    let (netlist, _, output, _) = rc_low_pass_netlist();
    let result = solve_ac(
        &netlist,
        AcOptions {
            sweep: AcSweep::Explicit(vec![1_000.0]),
            ..AcOptions::default()
        },
    )
    .unwrap();

    assert_eq!(
        result.samples[0].node_voltage(output).unwrap(),
        Phasor::ZERO
    );
}

#[test]
fn ground_only_netlist_has_a_zero_ac_sample() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let netlist = builder.build().unwrap();
    let result = solve_ac(
        &netlist,
        AcOptions {
            sweep: AcSweep::Explicit(vec![1_000.0]),
            ..AcOptions::default()
        },
    )
    .unwrap();

    assert_eq!(result.samples.len(), 1);
    assert_eq!(
        result.samples[0].node_voltage(ground).unwrap(),
        Phasor::ZERO
    );
}

#[test]
fn generated_sweeps_keep_their_requested_endpoints() {
    let linear = AcSweep::Linear {
        start: 10.0,
        stop: 50.0,
        points: 5,
    };
    assert_eq!(
        linear.frequencies().unwrap(),
        vec![10.0, 20.0, 30.0, 40.0, 50.0]
    );

    let logarithmic = AcSweep::Logarithmic {
        start: 10.0,
        stop: 1_000.0,
        points: 3,
    };
    let logarithmic = logarithmic.frequencies().unwrap();
    assert_eq!(logarithmic.len(), 3);
    assert_eq!(logarithmic[0], 10.0);
    assert_close(logarithmic[1], 100.0, 1e-10);
    assert_eq!(logarithmic[2], 1_000.0);
}

#[test]
fn invalid_sweeps_are_rejected() {
    let invalid = [
        AcSweep::Explicit(Vec::new()),
        AcSweep::Explicit(vec![1.0, 1.0]),
        AcSweep::Explicit(vec![2.0, 1.0]),
        AcSweep::Explicit(vec![f64::NAN]),
        AcSweep::Linear {
            start: 0.0,
            stop: 1.0,
            points: 2,
        },
        AcSweep::Linear {
            start: 1.0,
            stop: 0.5,
            points: 2,
        },
        AcSweep::Linear {
            start: 1.0,
            stop: 2.0,
            points: 0,
        },
        AcSweep::Linear {
            start: 1.0,
            stop: 2.0,
            points: 1_000_001,
        },
        AcSweep::Logarithmic {
            start: 1.0,
            stop: 1.0,
            points: 2,
        },
    ];
    for sweep in invalid {
        assert!(matches!(
            sweep.frequencies(),
            Err(AcError::InvalidSweep { .. })
        ));
    }
}

#[test]
fn invalid_excitations_are_rejected_before_the_dc_solve() {
    let (netlist, source, _, _) = rc_low_pass_netlist();
    let resistor = netlist
        .components
        .iter()
        .find(|component| component.kind == ComponentKind::Resistor)
        .unwrap()
        .id;
    let invalid = [
        excitation(ComponentId(9_999), 1.0, 0.0),
        excitation(resistor, 1.0, 0.0),
        excitation(source, -1.0, 0.0),
        excitation(source, 1.0, f64::NAN),
    ];
    for item in invalid {
        let error = solve_ac(&netlist, explicit_options(1_000.0, vec![item])).unwrap_err();
        assert!(matches!(error, AcError::InvalidExcitation { .. }));
    }

    let duplicate = solve_ac(
        &netlist,
        explicit_options(
            1_000.0,
            vec![excitation(source, 1.0, 0.0), excitation(source, 2.0, 0.0)],
        ),
    )
    .unwrap_err();
    assert!(matches!(duplicate, AcError::InvalidExcitation { .. }));
}

#[test]
fn ac_reports_a_missing_dc_operating_point() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let floating = builder.add_node("floating", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C1",
            number("capacitance", 1e-6),
            vec![floating, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let error = solve_ac(
        &netlist,
        AcOptions {
            sweep: AcSweep::Explicit(vec![1_000.0]),
            ..AcOptions::default()
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        AcError::OperatingPoint(DcError::SingularMatrix { .. })
    ));
}
