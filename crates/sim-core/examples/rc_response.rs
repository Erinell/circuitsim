//! Prints the DC, transient, and AC response of a first-order RC filter.

use std::error::Error;
use std::f64::consts::PI;

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

fn rc_filter() -> Result<(Netlist, ComponentId, NodeId), Box<dyn Error>> {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground)?;
    let input = builder.add_node("in", NodeKind::Electrical)?;
    let output = builder.add_node("out", NodeKind::Electrical)?;
    let source = builder.add_component(
        ComponentKind::VoltageSource,
        "V1",
        number("dc", 0.0),
        vec![input, ground],
    )?;
    builder.add_component(
        ComponentKind::Resistor,
        "R1",
        number("resistance", 1_000.0),
        vec![input, output],
    )?;
    builder.add_component(
        ComponentKind::Capacitor,
        "C1",
        number("capacitance", 1e-6),
        vec![output, ground],
    )?;
    Ok((builder.build()?, source, output))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (netlist, source, output) = rc_filter()?;

    let dc = solve_dc(&netlist, DcOptions::default())?;
    println!(
        "DC : V(OUT) = {:.6} V",
        dc.node_voltage(output)
            .ok_or_else(|| std::io::Error::other("sortie absente du résultat DC"))?
    );

    let cutoff = 1.0 / (2.0 * PI * 1_000.0 * 1e-6);
    let ac = solve_ac(
        &netlist,
        AcOptions {
            sweep: AcSweep::Explicit(vec![1.0, cutoff, 1e6]),
            excitations: vec![AcExcitation {
                component: source,
                magnitude: 1.0,
                phase_degrees: 0.0,
            }],
            solver: DcOptions::default(),
        },
    )?;
    for sample in &ac.samples {
        let voltage = sample
            .node_voltage(output)
            .ok_or_else(|| std::io::Error::other("sortie absente du résultat AC"))?;
        println!(
            "AC : f = {:>12.3} Hz, |V(OUT)| = {:.6} V, phase = {:.3}°",
            sample.frequency,
            voltage.magnitude(),
            voltage.phase_degrees()
        );
    }

    let transient = simulate_transient(
        &netlist,
        TransientOptions {
            stop_time: 5e-3,
            step: 1e-5,
            max_steps: 600,
            events: vec![TransientEvent {
                time: 0.0,
                action: TransientAction::SetSourceValue {
                    component: source,
                    value: 5.0,
                },
            }],
            ..TransientOptions::default()
        },
    )?;
    let final_sample = transient
        .samples
        .last()
        .ok_or_else(|| std::io::Error::other("transitoire sans échantillon"))?;
    println!(
        "Transitoire : {} échantillons, V(OUT, 5 ms) = {:.6} V",
        transient.samples.len(),
        final_sample
            .node_voltage(output)
            .ok_or_else(|| std::io::Error::other("sortie absente du transitoire"))?
    );

    Ok(())
}
