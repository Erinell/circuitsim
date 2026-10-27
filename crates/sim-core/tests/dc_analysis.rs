use sim_core::netlist::{
    ComponentId, ComponentKind, Netlist, NetlistBuilder, NodeId, NodeKind, ParamMap, ParamValue,
};
use sim_core::{solve_dc, DcError, DcOptions, MnaError, NewtonStrategy};

fn number(value: f64) -> ParamMap {
    let mut params = ParamMap::new();
    params.insert("resistance".to_owned(), ParamValue::Number(value));
    params
}

fn number_param(key: &str, value: f64) -> ParamMap {
    let mut params = ParamMap::new();
    params.insert(key.to_owned(), ParamValue::Number(value));
    params
}

fn diode_params() -> ParamMap {
    let mut params = ParamMap::new();
    params.insert("is".to_owned(), ParamValue::Number(1e-14));
    params.insert("n".to_owned(), ParamValue::Number(1.0));
    params
}

fn zener_params() -> ParamMap {
    let mut params = diode_params();
    params.insert("bv".to_owned(), ParamValue::Number(5.1));
    params.insert("ibv".to_owned(), ParamValue::Number(1e-3));
    params.insert("nbv".to_owned(), ParamValue::Number(1.0));
    params
}

#[test]
fn solves_a_resistive_voltage_divider() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 10.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R2",
            number(2_000.0),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    assert!((result.node_voltage(input).unwrap() - 10.0).abs() < 1e-9);
    assert!((result.node_voltage(output).unwrap() - 20.0 / 3.0).abs() < 1e-9);
    assert!((result.node_voltage(ground).unwrap()).abs() < 1e-12);
    assert!((result.branch_current(source).unwrap() + 10.0 / 3_000.0).abs() < 1e-9);
    assert_eq!(result.iterations, 1);
    assert!(result.residual < 1e-9);
}

#[test]
fn maps_a_ground_node_that_is_not_first() {
    let mut builder = NetlistBuilder::new();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 10.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R2",
            number(2_000.0),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    assert!((result.node_voltage(output).unwrap() - 20.0 / 3.0).abs() < 1e-9);
    assert_eq!(result.node_voltage(ground), Some(0.0));
}

#[test]
fn applies_current_source_sign_convention() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::CurrentSource,
            "I1",
            number_param("dc", 1e-3),
            vec![output, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    assert!((result.node_voltage(output).unwrap() + 1.0).abs() < 1e-9);
}

#[test]
fn models_a_capacitor_as_an_open_circuit_at_dc() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 10.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C1",
            number_param("capacitance", 1e-6),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    assert!((result.node_voltage(output).unwrap() - 10.0).abs() < 1e-9);
    assert!((result.branch_current(source).unwrap()).abs() < 1e-12);
}

#[test]
fn models_an_inductor_as_a_short_circuit_at_dc() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 10.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
            vec![input, output],
        )
        .unwrap();
    let inductor = builder
        .add_component(
            ComponentKind::Inductor,
            "L1",
            number_param("inductance", 1e-3),
            vec![output, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    assert!(result.node_voltage(output).unwrap().abs() < 1e-9);
    assert!((result.branch_current(source).unwrap() + 0.01).abs() < 1e-9);
    assert!((result.branch_current(inductor).unwrap() - 0.01).abs() < 1e-9);
}

#[test]
fn inductor_series_resistance_limits_the_dc_current() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 5.0),
            vec![input, ground],
        )
        .unwrap();
    let mut params = number_param("inductance", 1e-3);
    params.insert("resistance".to_owned(), ParamValue::Number(10.0));
    let inductor = builder
        .add_component(ComponentKind::Inductor, "L1", params, vec![input, ground])
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();
    assert!((result.branch_current(inductor).unwrap() - 0.5).abs() < 1e-12);
}

#[test]
fn solves_an_open_and_a_closed_switch() {
    fn switch_circuit(closed: bool) -> (Netlist, NodeId) {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let input = builder.add_node("in", NodeKind::Electrical).unwrap();
        let output = builder.add_node("out", NodeKind::Electrical).unwrap();
        builder
            .add_component(
                ComponentKind::VoltageSource,
                "V1",
                number_param("dc", 10.0),
                vec![input, ground],
            )
            .unwrap();
        builder
            .add_component(
                ComponentKind::Resistor,
                "R1",
                number(1_000.0),
                vec![input, output],
            )
            .unwrap();
        let mut params = number_param("ron", 1e-3);
        params.insert("roff".to_owned(), ParamValue::Number(1e9));
        params.insert("closed".to_owned(), ParamValue::Bool(closed));
        builder
            .add_component(ComponentKind::Switch, "S1", params, vec![output, ground])
            .unwrap();
        (builder.build().unwrap(), output)
    }

    let (open_netlist, open_output) = switch_circuit(false);
    let open = solve_dc(&open_netlist, DcOptions::default()).unwrap();
    assert!(open.node_voltage(open_output).unwrap() > 9.9);

    let (closed_netlist, closed_output) = switch_circuit(true);
    let closed = solve_dc(&closed_netlist, DcOptions::default()).unwrap();
    assert!(closed.node_voltage(closed_output).unwrap() < 1e-3);
}

#[test]
fn rejects_a_switch_without_its_state() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let mut params = number_param("ron", 1e-3);
    params.insert("roff".to_owned(), ParamValue::Number(1e9));
    builder
        .add_component(ComponentKind::Switch, "S1", params, vec![output, ground])
        .unwrap();
    let netlist = builder.build().unwrap();

    assert!(matches!(
        solve_dc(&netlist, DcOptions::default()),
        Err(DcError::Circuit(MnaError::MissingParameter {
            key: "closed",
            ..
        }))
    ));
}

#[test]
fn solves_a_forward_biased_diode() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 5.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
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

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    let voltage = result.node_voltage(output).unwrap();
    assert!((0.5..0.9).contains(&voltage), "diode voltage was {voltage}");
    let current = (5.0 - voltage) / 1_000.0;
    assert!(
        (0.004..0.0045).contains(&current),
        "diode current was {current}"
    );
    assert!(result.iterations > 1);
}

#[test]
fn solves_a_forward_biased_led() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 5.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
            vec![input, output],
        )
        .unwrap();
    let mut params = ParamMap::new();
    params.insert("is".to_owned(), ParamValue::Number(1e-20));
    params.insert("n".to_owned(), ParamValue::Number(1.8));
    builder
        .add_component(ComponentKind::Led, "D1", params, vec![output, ground])
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    let voltage = result.node_voltage(output).unwrap();
    assert!((1.5..2.2).contains(&voltage), "LED voltage was {voltage}");
    assert!(result.iterations > 1);
}

#[test]
fn led_vf_and_if_define_the_forward_characteristic() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 5.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(150.0),
            vec![input, output],
        )
        .unwrap();
    let mut params = ParamMap::new();
    params.insert("vf".to_owned(), ParamValue::Number(2.0));
    params.insert("if".to_owned(), ParamValue::Number(0.02));
    params.insert("imax".to_owned(), ParamValue::Number(0.03));
    params.insert("wavelength".to_owned(), ParamValue::Number(620.0));
    builder
        .add_component(ComponentKind::Led, "D1", params, vec![output, ground])
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();
    let voltage = result.node_voltage(output).unwrap();
    let current = (5.0 - voltage) / 150.0;
    assert!((1.9..2.1).contains(&voltage), "LED voltage was {voltage}");
    assert!(
        (0.018..0.022).contains(&current),
        "LED current was {current}"
    );
}

#[test]
fn reports_non_convergence_for_a_limited_iteration_budget() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 5.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
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

    let options = DcOptions {
        max_iterations: 1,
        ..DcOptions::default()
    };
    assert!(matches!(
        solve_dc(&netlist, options),
        Err(DcError::NoConvergence { iterations: 1, .. })
    ));
}

#[test]
fn solves_a_reverse_biased_zener() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 10.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number(1_000.0),
            vec![input, output],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Zener,
            "D1",
            zener_params(),
            vec![ground, output],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    let voltage = result.node_voltage(output).unwrap();
    assert!((5.0..5.3).contains(&voltage), "zener voltage was {voltage}");
}

#[test]
fn rejects_a_netlist_without_ground() {
    let error = solve_dc(&Netlist::empty(), DcOptions::default()).unwrap_err();
    assert!(matches!(error, DcError::Circuit(MnaError::Netlist(_))));
}

#[test]
fn rejects_unsupported_components() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let node = builder.add_node("node", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::Custom("mosfet".to_owned()),
            "M1",
            ParamMap::new(),
            vec![node, ground],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    let error = solve_dc(&netlist, DcOptions::default()).unwrap_err();
    assert!(matches!(
        error,
        DcError::Circuit(MnaError::UnsupportedComponent {
            kind: ComponentKind::Custom(_),
            ..
        })
    ));
}

#[test]
fn rejects_invalid_and_missing_parameters() {
    fn with_resistor(params: ParamMap) -> Netlist {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let output = builder.add_node("out", NodeKind::Electrical).unwrap();
        builder
            .add_component(ComponentKind::Resistor, "R1", params, vec![output, ground])
            .unwrap();
        builder.build().unwrap()
    }

    let zero = with_resistor(number(0.0));
    assert!(matches!(
        solve_dc(&zero, DcOptions::default()),
        Err(DcError::Circuit(MnaError::InvalidParameterValue { .. }))
    ));

    let missing = with_resistor(ParamMap::new());
    assert!(matches!(
        solve_dc(&missing, DcOptions::default()),
        Err(DcError::Circuit(MnaError::MissingParameter { .. }))
    ));

    let mut wrong_type_params = ParamMap::new();
    wrong_type_params.insert("resistance".to_owned(), ParamValue::Text("1k".to_owned()));
    let wrong_type = with_resistor(wrong_type_params);
    assert!(matches!(
        solve_dc(&wrong_type, DcOptions::default()),
        Err(DcError::Circuit(MnaError::InvalidParameterType {
            key: "resistance",
            ..
        }))
    ));
}

#[test]
fn rejects_a_floating_dc_node() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let input = builder.add_node("in", NodeKind::Electrical).unwrap();
    let floating = builder.add_node("floating", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 1.0),
            vec![input, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C1",
            number_param("capacitance", 1e-6),
            vec![input, floating],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    for strategy in [
        NewtonStrategy::Auto,
        NewtonStrategy::Plain,
        NewtonStrategy::Gmin,
        NewtonStrategy::SourceStepping,
    ] {
        let options = DcOptions {
            strategy,
            ..DcOptions::default()
        };
        assert!(matches!(
            solve_dc(&netlist, options),
            Err(DcError::SingularMatrix { .. })
        ));
    }
}

#[test]
fn rejects_invalid_options() {
    let error = solve_dc(
        &Netlist::empty(),
        DcOptions {
            max_iterations: 0,
            ..DcOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(error, DcError::InvalidOptions { .. }));
}

#[test]
fn solves_a_ground_only_netlist() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND1", ParamMap::new(), vec![ground])
        .unwrap();
    let netlist = builder.build().unwrap();

    let result = solve_dc(&netlist, DcOptions::default()).unwrap();

    assert_eq!(result.node_voltage(ground), Some(0.0));
    assert!(result.branch_currents.is_empty());
    assert_eq!(result.iterations, 0);
}

#[test]
fn rejects_a_degenerate_voltage_source_branch() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let node = builder.add_node("node", NodeKind::Electrical).unwrap();
    let source = builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number_param("dc", 1.0),
            vec![node, node],
        )
        .unwrap();
    let netlist = builder.build().unwrap();

    assert_eq!(
        solve_dc(&netlist, DcOptions::default()),
        Err(DcError::Circuit(MnaError::DegenerateBranch {
            component: ComponentId(source.0),
        }))
    );
    assert_eq!(netlist.nodes.len(), 2);
    assert_eq!(ground, NodeId(0));
}
