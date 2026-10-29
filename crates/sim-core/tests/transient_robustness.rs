use sim_core::netlist::{ComponentKind, Netlist, NetlistBuilder, NodeKind, ParamMap, ParamValue};
use sim_core::{simulate_transient, TransientOptions};

fn params(entries: &[(&str, ParamValue)]) -> ParamMap {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn slow_comparator_netlist() -> Netlist {
    let mut builder = NetlistBuilder::new();
    let n0 = builder.add_node("0", NodeKind::Ground).unwrap();
    let n1 = builder.add_node("N$001", NodeKind::Electrical).unwrap();
    let n2 = builder.add_node("N$002", NodeKind::Electrical).unwrap();
    let n3 = builder.add_node("N$003", NodeKind::Electrical).unwrap();
    let n4 = builder.add_node("N$004", NodeKind::Electrical).unwrap();
    let n5 = builder.add_node("N$005", NodeKind::Electrical).unwrap();
    let n6 = builder.add_node("N$006", NodeKind::Electrical).unwrap();
    let n7 = builder.add_node("N$007", NodeKind::Electrical).unwrap();
    let n8 = builder.add_node("N$008", NodeKind::Electrical).unwrap();
    let n9 = builder.add_node("N$009", NodeKind::Electrical).unwrap();
    let n10 = builder.add_node("N$010", NodeKind::Electrical).unwrap();
    let n11 = builder.add_node("N$011", NodeKind::Electrical).unwrap();
    let n12 = builder.add_node("N$012", NodeKind::Electrical).unwrap();
    let n13 = builder.add_node("N$013", NodeKind::Electrical).unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND1", params(&[]), vec![n0])
        .unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            params(&[
                ("dc", ParamValue::Number(30.0)),
                ("imax", ParamValue::Number(10.0)),
            ]),
            vec![n1, n0],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Switch,
            "SW1",
            params(&[
                ("closed", ParamValue::Bool(true)),
                ("imax", ParamValue::Number(1.0)),
                ("roff", ParamValue::Number(1e18)),
                ("ron", ParamValue::Number(0.001)),
            ]),
            vec![n1, n2],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Diode,
            "D1",
            params(&[
                ("if", ParamValue::Number(0.01)),
                ("imax", ParamValue::Number(5.0)),
                ("n", ParamValue::Number(1.05)),
                ("type", ParamValue::Text("schottky".to_owned())),
                ("vf", ParamValue::Number(0.85)),
            ]),
            vec![n2, n3],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(3_300.0)),
            ]),
            vec![n3, n4],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Zener,
            "DZ1",
            params(&[
                ("bv", ParamValue::Number(12.0)),
                ("ibv", ParamValue::Number(1e-3)),
                ("imax", ParamValue::Number(0.2)),
                ("is", ParamValue::Number(1e-14)),
                ("n", ParamValue::Number(1.0)),
                ("nbv", ParamValue::Number(1.0)),
            ]),
            vec![n0, n4],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C1",
            params(&[
                ("capacitance", ParamValue::Number(1e-5)),
                ("imax", ParamValue::Number(1.0)),
            ]),
            vec![n4, n0],
        )
        .unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND2", params(&[]), vec![n0])
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R2",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(2_200.0)),
            ]),
            vec![n4, n5],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::ShuntReference,
            "U1",
            params(&[
                ("ik_min", ParamValue::Number(6e-4)),
                ("imax", ParamValue::Number(0.1)),
                ("iref", ParamValue::Number(2e-6)),
                ("vref", ParamValue::Number(2.5)),
                ("z_ka", ParamValue::Number(0.2)),
            ]),
            vec![n5, n5, n0],
        )
        .unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND3", params(&[]), vec![n0])
        .unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C2",
            params(&[
                ("capacitance", ParamValue::Number(1e-7)),
                ("imax", ParamValue::Number(1.0)),
            ]),
            vec![n5, n0],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R3",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(1_000.0)),
            ]),
            vec![n1, n6],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R4",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(100_000.0)),
            ]),
            vec![n6, n7],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R5",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(9_700.0)),
            ]),
            vec![n7, n0],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Capacitor,
            "C3",
            params(&[
                ("capacitance", ParamValue::Number(1e-7)),
                ("imax", ParamValue::Number(1.0)),
            ]),
            vec![n7, n0],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Comparator,
            "U2",
            params(&[
                ("imax", ParamValue::Number(0.016)),
                ("r_sink", ParamValue::Number(20.0)),
                ("v_offset", ParamValue::Number(0.005)),
            ]),
            vec![n8, n7, n9, n4, n0],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R6",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(10_000.0)),
            ]),
            vec![n5, n8],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R7",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(4_700_000.0)),
            ]),
            vec![n8, n9],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R8",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(4_700.0)),
            ]),
            vec![n4, n9],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R9",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(1_000.0)),
            ]),
            vec![n9, n10],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R10",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(100_000.0)),
            ]),
            vec![n10, n0],
        )
        .unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND5", params(&[]), vec![n0])
        .unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND6", params(&[]), vec![n0])
        .unwrap();
    builder
        .add_component(
            ComponentKind::Mosfet,
            "M1",
            params(&[
                ("cds", ParamValue::Number(0.0)),
                ("cgd", ParamValue::Number(0.0)),
                ("cgs", ParamValue::Number(0.0)),
                ("channel", ParamValue::Text("n".to_owned())),
                ("imax", ParamValue::Number(0.3)),
                ("is", ParamValue::Number(1e-14)),
                ("k", ParamValue::Number(0.17)),
                ("lambda", ParamValue::Number(0.02)),
                ("n", ParamValue::Number(1.0)),
                ("rd", ParamValue::Number(1.5)),
                ("rs", ParamValue::Number(1.5)),
                ("swap_ds", ParamValue::Bool(false)),
                ("vth", ParamValue::Number(2.0)),
            ]),
            vec![n11, n10, n0],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R11",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(4_700.0)),
            ]),
            vec![n11, n12],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Mosfet,
            "M2",
            params(&[
                ("cds", ParamValue::Number(0.0)),
                ("cgd", ParamValue::Number(0.0)),
                ("cgs", ParamValue::Number(0.0)),
                ("channel", ParamValue::Text("p".to_owned())),
                ("imax", ParamValue::Number(15.0)),
                ("is", ParamValue::Number(1e-14)),
                ("k", ParamValue::Number(1.28)),
                ("lambda", ParamValue::Number(0.0)),
                ("n", ParamValue::Number(1.0)),
                ("rd", ParamValue::Number(0.06)),
                ("rs", ParamValue::Number(0.05)),
                ("swap_ds", ParamValue::Bool(false)),
                ("vth", ParamValue::Number(3.75)),
            ]),
            vec![n13, n12, n1],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Zener,
            "DZ2",
            params(&[
                ("bv", ParamValue::Number(5.1)),
                ("ibv", ParamValue::Number(1e-3)),
                ("imax", ParamValue::Number(0.2)),
                ("is", ParamValue::Number(1e-14)),
                ("n", ParamValue::Number(1.0)),
                ("nbv", ParamValue::Number(1.0)),
            ]),
            vec![n12, n1],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R12",
            params(&[
                ("imax", ParamValue::Number(0.25)),
                ("resistance", ParamValue::Number(10_000.0)),
            ]),
            vec![n12, n1],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R13",
            params(&[
                ("imax", ParamValue::Number(5.0)),
                ("resistance", ParamValue::Number(19.0)),
            ]),
            vec![n0, n13],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Diode,
            "D2",
            params(&[
                ("if", ParamValue::Number(0.01)),
                ("imax", ParamValue::Number(0.2)),
                ("n", ParamValue::Number(1.0)),
                ("type", ParamValue::Text("standard".to_owned())),
                ("vf", ParamValue::Number(0.7)),
            ]),
            vec![n0, n13],
        )
        .unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND7", params(&[]), vec![n0])
        .unwrap();
    builder
        .add_component(ComponentKind::Ground, "GND4", params(&[]), vec![n0])
        .unwrap();
    builder.build().unwrap()
}

#[test]
fn slow_comparator_supply_converges_with_internal_substeps() {
    let netlist = slow_comparator_netlist();
    let result = simulate_transient(
        &netlist,
        TransientOptions {
            start_time: 0.0,
            stop_time: 5e-3,
            step: 1e-3,
            ..TransientOptions::default()
        },
    )
    .expect("the 1 ms transient must recover through internal subdivisions");

    assert_eq!(result.steps, 5);
    assert_eq!(result.samples.len(), 6);
    let last = result.samples.last().unwrap();
    assert!((last.time - 5e-3).abs() < 1e-12);
    assert!(result.iterations > 5);
    assert!(result.energy.residual.is_finite());
}
