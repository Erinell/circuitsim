//! Demonstrates a custom NMOS symbol from the registry through extraction.
//!
//! CircuitSim stores custom components in the schematic and netlist, but the
//! solvers still reject them until a model is implemented in `sim-core`.

use std::error::Error;

use editor_core::{
    extract_netlist, from_json, to_json, Command, GridPos, Orientation, PinDef, Schematic, Symbol,
    SymbolDef, SymbolId, SymbolKind, SymbolRegistry, UndoStack, Wire, WireId,
};
use sim_core::netlist::{ComponentKind, ParamMap, ParamValue};
use sim_core::{solve_dc, DcError, DcOptions, MnaError};

const CUSTOM_KIND: &str = "nmos";

fn symbol_id(label: &str) -> SymbolKind {
    SymbolKind::Custom(label.to_owned())
}

fn add_symbol(
    schematic: &mut Schematic,
    history: &mut UndoStack,
    registry: &SymbolRegistry,
    kind: SymbolKind,
    at: GridPos,
    label: &str,
) -> Result<SymbolId, Box<dyn Error>> {
    let definition = registry
        .get(&kind)
        .ok_or_else(|| std::io::Error::other(format!("symbole inconnu : {kind}")))?
        .clone();
    let id = SymbolId(schematic.allocate_id()?);
    let symbol = Symbol::from_def(id, &definition, at, label, Orientation::R0, false);
    history.execute(schematic, Command::AddSymbol(symbol))?;
    Ok(id)
}

fn add_wire(
    schematic: &mut Schematic,
    history: &mut UndoStack,
    a: GridPos,
    b: GridPos,
) -> Result<(), Box<dyn Error>> {
    let id = WireId(schematic.allocate_id()?);
    let wire = Wire::new(id, a, b)?;
    history.execute(schematic, Command::AddWire(wire))?;
    Ok(())
}

fn custom_nmos() -> Result<SymbolDef, Box<dyn Error>> {
    let mut defaults = ParamMap::new();
    defaults.insert("vth".to_owned(), ParamValue::Number(2.0));
    defaults.insert("k".to_owned(), ParamValue::Number(1e-3));
    defaults.insert("lambda".to_owned(), ParamValue::Number(0.02));
    Ok(SymbolDef::new(
        symbol_id(CUSTOM_KIND),
        vec![
            PinDef::new("d", GridPos::new(-2, 0)),
            PinDef::new("g", GridPos::new(0, -2)),
            PinDef::new("s", GridPos::new(2, 0)),
        ],
        defaults,
    )?)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut registry = SymbolRegistry::builtin();
    registry.register(custom_nmos()?)?;

    let mut schematic = Schematic::new();
    let mut history = UndoStack::new();
    add_symbol(
        &mut schematic,
        &mut history,
        &registry,
        symbol_id(CUSTOM_KIND),
        GridPos::new(0, 0),
        "M1",
    )?;
    add_symbol(
        &mut schematic,
        &mut history,
        &registry,
        SymbolKind::Ground,
        GridPos::new(2, 4),
        "GND1",
    )?;

    for (a, b) in [
        (GridPos::new(-2, 0), GridPos::new(-2, 4)),
        (GridPos::new(-2, 4), GridPos::new(2, 4)),
        (GridPos::new(2, 0), GridPos::new(2, 4)),
        (GridPos::new(0, -2), GridPos::new(-4, -2)),
        (GridPos::new(-4, -2), GridPos::new(-4, 4)),
        (GridPos::new(-4, 4), GridPos::new(-2, 4)),
    ] {
        add_wire(&mut schematic, &mut history, a, b)?;
    }

    let extraction = extract_netlist(&schematic, &registry)?;
    let custom = extraction
        .netlist
        .components
        .iter()
        .find(|component| matches!(component.kind, ComponentKind::Custom(_)))
        .ok_or_else(|| std::io::Error::other("composant personnalisé absent de la netlist"))?;
    println!(
        "Registre : {} symboles, dont {} personnalisé(s).",
        registry.len(),
        1
    );
    println!(
        "Netlist : {} composant(s), M1 = {:?}.",
        extraction.netlist.components.len(),
        custom.kind
    );

    let json = to_json(&schematic)?;
    let restored = from_json(&json)?;
    println!(
        "JSON : {} octets, kind personnalisé restauré = {}.",
        json.len(),
        restored.content_eq(&schematic)
    );

    match solve_dc(&extraction.netlist, DcOptions::default()) {
        Err(DcError::Circuit(MnaError::UnsupportedComponent {
            kind: ComponentKind::Custom(kind),
            ..
        })) if kind == CUSTOM_KIND => {
            println!(
                "Simulation : refus explicite MnaError::UnsupportedComponent pour « {kind} »."
            );
        }
        other => {
            return Err(std::io::Error::other(format!(
                "le solveur aurait dû refuser le composant personnalisé : {other:?}"
            ))
            .into());
        }
    }

    println!(
        "Pour simuler ce transistor, ajouter un modèle dans sim-core puis accepter ce kind dans MNA."
    );
    Ok(())
}
