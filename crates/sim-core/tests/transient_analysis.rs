use sim_core::netlist::{
    ComponentId, ComponentKind, Netlist, NetlistBuilder, NodeId, NodeKind, ParamMap, ParamValue,
};
use sim_core::{
    simulate_transient, DcError, DcOptions, MnaError, NewtonStrategy, TransientAction,
    TransientError, TransientEvent, TransientOptions,
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

fn switch_state(closed: bool) -> ParamMap {
    params(&[
        ("ron", ParamValue::Number(1.0)),
        ("roff", ParamValue::Number(1e9)),
        ("closed", ParamValue::Bool(closed)),
    ])
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

fn source_event(time: f64, component: ComponentId, value: f64) -> TransientEvent {
    TransientEvent {
        time,
        action: TransientAction::SetSourceValue { component, value },
    }
}

fn rc_netlist() -> (Netlist, ComponentId, NodeId, ComponentId, NodeId) {
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
    let capacitor = builder
        .add_component(
            ComponentKind::Capacitor,
            "C1",
            number("capacitance", 1e-6),
            vec![output, ground],
        )
        .unwrap();
    (builder.build().unwrap(), source, output, capacitor, ground)
}

fn rl_netlist() -> (Netlist, ComponentId, NodeId, ComponentId, NodeId) {
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
            number("resistance", 100.0),
            vec![input, output],
        )
        .unwrap();
    let inductor = builder
        .add_component(
            ComponentKind::Inductor,
            "L1",
            number("inductance", 1e-3),
            vec![output, ground],
        )
        .unwrap();
    (builder.build().unwrap(), source, output, inductor, ground)
}

#[test]
fn charges_a_capacitor_through_a_resistor() {
    let (netlist, source, output, capacitor, _) = rc_netlist();
    let options = TransientOptions {
        stop_time: 2e-3,
        step: 1e-4,
        events: vec![source_event(0.0, source, 5.0)],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    assert_eq!(result.samples.len(), 21);
    assert_eq!(result.steps, 20);
    assert!(result.samples[0].node_voltage(output).unwrap().abs() < 1e-12);

    let at_one_ms = result
        .samples
        .iter()
        .find(|sample| (sample.time - 1e-3).abs() < 1e-12)
        .unwrap();
    let ratio: f64 = 1.0 / (1.0 + 1e-4 / 1e-3);
    let expected = 5.0 * (1.0 - ratio.powi(10));
    let actual = at_one_ms.node_voltage(output).unwrap();
    assert!(
        (actual - expected).abs() < 1e-9,
        "actual {actual}, expected {expected}"
    );

    let final_voltage = result.samples.last().unwrap().node_voltage(output).unwrap();
    assert!(final_voltage > 4.0 && final_voltage < 5.0);
    assert!(result.energy.supplied > 0.0);
    assert!(result.energy.dissipated > 0.0);
    assert!(result.energy.stored_delta > 0.0);
    let capacitor_energy = result.energy.components.get(&capacitor).unwrap();
    assert!((capacitor_energy.stored_delta - 0.5e-6 * final_voltage.powi(2)).abs() < 1e-12);
    let reconstructed =
        result.energy.supplied - result.energy.dissipated - result.energy.stored_delta;
    assert!((reconstructed - result.energy.residual).abs() < 1e-12);
    assert!(
        (result.energy.residual / result.energy.supplied).abs() < 0.05,
        "energy audit residual was {}",
        result.energy.residual
    );
}

#[test]
fn raises_an_inductor_current_through_a_resistor() {
    let (netlist, source, _, inductor, _) = rl_netlist();
    let options = TransientOptions {
        stop_time: 5e-4,
        step: 1e-5,
        events: vec![source_event(0.0, source, 5.0)],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    let at_one_tau = result
        .samples
        .iter()
        .find(|sample| (sample.time - 1e-4).abs() < 1e-15)
        .unwrap();
    let ratio: f64 = 1.0 / (1.0 + 1e-5 / 1e-5);
    let expected = 0.05 * (1.0 - ratio.powi(10));
    let actual = at_one_tau.branch_current(inductor).unwrap();
    assert!(
        (actual - expected).abs() < 1e-9,
        "actual {actual}, expected {expected}"
    );
    assert!(result.energy.stored_delta > 0.0);
    assert!(result.energy.supplied > 0.0);
}

#[test]
fn applies_a_source_event_after_the_sample_at_its_time() {
    let (netlist, source, output, _, _) = rc_netlist();
    let options = TransientOptions {
        stop_time: 2e-3,
        step: 2.5e-4,
        events: vec![
            source_event(0.0, source, 5.0),
            source_event(1e-3, source, 0.0),
        ],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    let at_event = result
        .samples
        .iter()
        .find(|sample| (sample.time - 1e-3).abs() < 1e-12)
        .unwrap();
    let after_event = result
        .samples
        .iter()
        .find(|sample| (sample.time - 1.25e-3).abs() < 1e-12)
        .unwrap();
    assert!(at_event.node_voltage(output).unwrap() > 2.5);
    assert!(after_event.node_voltage(output).unwrap() < at_event.node_voltage(output).unwrap());
    assert!(after_event.node_voltage(output).unwrap() > 0.0);
}

#[test]
fn inserts_an_event_time_that_is_not_on_a_nominal_tick() {
    let (netlist, source, output, _, _) = rc_netlist();
    let options = TransientOptions {
        stop_time: 2e-3,
        step: 5e-4,
        events: vec![
            source_event(0.0, source, 5.0),
            source_event(9e-4, source, 0.0),
        ],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    assert!(result
        .samples
        .windows(2)
        .all(|window| window[1].time > window[0].time));
    let at_event = result
        .samples
        .iter()
        .find(|sample| (sample.time - 9e-4).abs() < 1e-15)
        .unwrap();
    let after_event = result
        .samples
        .iter()
        .find(|sample| (sample.time - 1e-3).abs() < 1e-15)
        .unwrap();
    assert!(at_event.node_voltage(output).unwrap() > 1.0);
    assert!(after_event.node_voltage(output).unwrap() < at_event.node_voltage(output).unwrap());
}

#[test]
fn preserves_events_close_to_the_time_bounds() {
    let (netlist, source, output, _, _) = rc_netlist();
    let near_start = 1e-15;
    let near_stop = 1e-3 - 1e-15;
    let options = TransientOptions {
        stop_time: 1e-3,
        step: 1e-3,
        events: vec![
            source_event(near_start, source, 5.0),
            source_event(near_stop, source, 0.0),
        ],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    assert!(result
        .samples
        .windows(2)
        .all(|window| window[1].time > window[0].time));
    assert_eq!(result.samples.first().unwrap().time, 0.0);
    assert_eq!(result.samples.last().unwrap().time, 1e-3);
    let before_second_event = result
        .samples
        .iter()
        .find(|sample| sample.time == near_stop)
        .unwrap();
    assert!(before_second_event.node_voltage(output).unwrap() > 0.5);
    let after_second_event = result.samples.last().unwrap();
    assert!(after_second_event.node_voltage(output).unwrap() > 0.5);
    assert!(result
        .samples
        .iter()
        .any(|sample| sample.time == near_start));
}

#[test]
fn reports_an_initial_dc_failure_before_transient_steps() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let floating = builder.add_node("floating", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number("dc", 1.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C1",
            number("capacitance", 1e-6),
            vec![input, floating],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    assert!(matches!(
        simulate_transient(&netlist, TransientOptions::default()),
        Err(TransientError::InitialDc {
            source: sim_core::DcError::SingularMatrix { .. }
        })
    ));
}

#[test]
fn rejects_a_non_finite_source_event_value() {
    let (netlist, source, _, _, _) = rc_netlist();
    let options = TransientOptions {
        events: vec![source_event(0.0, source, f64::NAN)],
        ..TransientOptions::default()
    };
    assert!(matches!(
        simulate_transient(&netlist, options),
        Err(TransientError::InvalidEvent { .. })
    ));
}

#[test]
fn applies_a_switch_event_without_changing_the_capacitor_voltage() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number("dc", 5.0),
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
    let switch = builder
        .add_component(
            ComponentKind::Switch,
            "S1",
            switch_state(false),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let options = TransientOptions {
        stop_time: 2e-3,
        step: 1e-4,
        events: vec![TransientEvent {
            time: 1e-3,
            action: TransientAction::SetSwitchClosed {
                component: switch,
                closed: true,
            },
        }],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    let at_event = result
        .samples
        .iter()
        .find(|sample| (sample.time - 1e-3).abs() < 1e-12)
        .unwrap();
    let after_event = result
        .samples
        .iter()
        .find(|sample| (sample.time - 1.1e-3).abs() < 1e-12)
        .unwrap();
    assert!((at_event.node_voltage(output).unwrap() - 5.0).abs() < 1e-5);
    assert!(after_event.node_voltage(output).unwrap() < 0.1);
}

#[test]
fn simulates_a_diode_after_a_source_step() {
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
            ComponentKind::Diode,
            "D1",
            diode_params(),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let options = TransientOptions {
        stop_time: 2e-3,
        step: 1e-5,
        events: vec![source_event(0.0, source, 5.0)],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    let final_voltage = result.samples.last().unwrap().node_voltage(output).unwrap();
    assert!(
        (0.5..0.9).contains(&final_voltage),
        "diode voltage was {final_voltage}"
    );
    assert!(result.energy.dissipated > 0.0);
}

#[test]
fn solves_an_operational_amplifier_in_dc() {
    fn amplifier(input_voltage: f64) -> (Netlist, NodeId, ComponentId, ComponentId) {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let input = builder.add_node("in", NodeKind::Electrical).unwrap();
        let feedback = builder.add_node("fb", NodeKind::Electrical).unwrap();
        let output = builder.add_node("out", NodeKind::Electrical).unwrap();
        let source = builder
            .add_component(
                ComponentKind::VoltageSource,
                "VIN",
                number("dc", input_voltage),
                vec![input, ground],
            )
            .unwrap();
        let opamp = builder
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
        (builder.build().unwrap(), output, opamp, source)
    }

    let (netlist, output, opamp, source) = amplifier(1.0);
    let result = sim_core::solve_dc(&netlist, DcOptions::default()).unwrap();
    let voltage = result.node_voltage(output).unwrap();
    assert!((10.9..11.0).contains(&voltage), "output was {voltage}");
    assert!(result.branch_current(opamp).is_some());
    assert!(result.branch_current(source).is_some());

    let (saturated_netlist, saturated_output, _, _) = amplifier(2.0);
    let saturated = sim_core::solve_dc(&saturated_netlist, DcOptions::default()).unwrap();
    assert!((saturated.node_voltage(saturated_output).unwrap() - 15.0).abs() < 1e-6);
}

#[test]
fn propagates_an_opamp_step_in_transient() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
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
            vec![input, output, output, ground, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();
    let options = TransientOptions {
        stop_time: 1e-4,
        step: 1e-5,
        events: vec![source_event(0.0, source, 1.0)],
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    let final_voltage = result.samples.last().unwrap().node_voltage(output).unwrap();
    assert!(
        (0.999..=1.0).contains(&final_voltage),
        "output was {final_voltage}"
    );
}

#[test]
fn rejects_an_opamp_with_incoherent_saturation() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let mut parameters = opamp_params();
    parameters.insert("v_sat_pos".to_owned(), ParamValue::Number(-1.0));
    parameters.insert("v_sat_neg".to_owned(), ParamValue::Number(1.0));
    builder
        .add_component(
            ComponentKind::OpAmp,
            "U1",
            parameters,
            vec![input, output, output, ground, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    assert!(matches!(
        sim_core::solve_dc(&netlist, DcOptions::default()),
        Err(DcError::Circuit(MnaError::InvalidSaturation { .. }))
    ));
}

#[test]
fn robust_newton_strategies_solve_a_diode_circuit() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number("dc", 5.0),
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

    for strategy in [NewtonStrategy::Gmin, NewtonStrategy::SourceStepping] {
        let options = DcOptions {
            strategy,
            ..DcOptions::default()
        };
        let result = sim_core::solve_dc(&netlist, options).unwrap();
        let voltage = result.node_voltage(output).unwrap();
        assert!((0.5..0.9).contains(&voltage), "{strategy:?}: {voltage}");
    }
}

#[test]
fn rejects_invalid_transient_options_and_events() {
    let (netlist, source, _output, _, _) = rc_netlist();

    let invalid_time = TransientOptions {
        start_time: f64::NAN,
        ..TransientOptions::default()
    };
    assert!(matches!(
        simulate_transient(&netlist, invalid_time),
        Err(TransientError::InvalidOptions { .. })
    ));

    let invalid_step = TransientOptions {
        step: 0.0,
        ..TransientOptions::default()
    };
    assert!(matches!(
        simulate_transient(&netlist, invalid_step),
        Err(TransientError::InvalidOptions { .. })
    ));

    let outside = TransientOptions {
        events: vec![source_event(2.0, source, 1.0)],
        ..TransientOptions::default()
    };
    assert!(matches!(
        simulate_transient(&netlist, outside),
        Err(TransientError::InvalidEvent { .. })
    ));

    let duplicate = TransientOptions {
        events: vec![
            source_event(0.0, source, 1.0),
            source_event(0.0, source, 2.0),
        ],
        ..TransientOptions::default()
    };
    assert!(matches!(
        simulate_transient(&netlist, duplicate),
        Err(TransientError::InvalidEvent { .. })
    ));

    let wrong_target = TransientOptions {
        events: vec![TransientEvent {
            time: 0.0,
            action: TransientAction::SetSwitchClosed {
                component: source,
                closed: true,
            },
        }],
        ..TransientOptions::default()
    };
    assert!(matches!(
        simulate_transient(&netlist, wrong_target),
        Err(TransientError::InvalidEvent { .. })
    ));
}

#[test]
fn rejects_too_many_transient_steps() {
    let (netlist, _, _, _, _) = rc_netlist();
    let options = TransientOptions {
        stop_time: 1e-3,
        step: 1e-9,
        max_steps: 10,
        ..TransientOptions::default()
    };
    assert!(matches!(
        simulate_transient(&netlist, options),
        Err(TransientError::TooManySteps { limit: 10 })
    ));
}

#[test]
fn keeps_a_ground_only_netlist_at_zero() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND1", ParamMap::new(), vec![ground])
        .unwrap();
    let netlist = builder.build().unwrap();
    let options = TransientOptions {
        stop_time: 1e-3,
        step: 1e-4,
        ..TransientOptions::default()
    };

    let result = simulate_transient(&netlist, options).unwrap();
    assert_eq!(result.steps, 10);
    assert_eq!(result.iterations, 0);
    assert_eq!(result.samples.len(), 11);
    assert!(result
        .samples
        .iter()
        .all(|sample| sample.node_voltage(ground) == Some(0.0)));
}
