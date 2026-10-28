//! Versioned JSON serialization for schematic documents.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::schematic::{Schematic, SchematicError};

/// Current schematic file schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// Versioned on-disk representation of a schematic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchematicFile {
    pub schema_version: u32,
    pub schematic: Schematic,
}

/// Error returned when reading or writing a schematic document.
#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("missing or invalid schema_version")]
    InvalidVersion,
    #[error("unsupported schematic schema version {0}")]
    UnsupportedVersion(u32),
    #[error("invalid schematic JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid schematic: {0}")]
    Schematic(#[from] SchematicError),
}

/// Serializes a validated schematic to pretty JSON.
pub fn to_json(schematic: &Schematic) -> Result<String, DocumentError> {
    schematic.validate()?;
    let file = SchematicFile {
        schema_version: SCHEMA_VERSION,
        schematic: schematic.clone(),
    };
    Ok(serde_json::to_string_pretty(&file)?)
}

/// Deserializes and validates a schematic from JSON.
pub fn from_json(json: &str) -> Result<Schematic, DocumentError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or(DocumentError::InvalidVersion)?;
    if version != SCHEMA_VERSION {
        return Err(DocumentError::UnsupportedVersion(version));
    }
    let file: SchematicFile = serde_json::from_value(value)?;
    file.schematic.validate()?;
    Ok(file.schematic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::grid::GridPos;
    use crate::symbol::{Symbol, SymbolKind, SymbolRegistry};
    use crate::undo::UndoStack;

    fn sample() -> Schematic {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let registry = SymbolRegistry::builtin();
        let def = registry.get(&SymbolKind::Resistor).unwrap();
        let id = crate::symbol::SymbolId(schematic.allocate_id().unwrap());
        let symbol = Symbol::from_def(
            id,
            def,
            GridPos::new(3, 4),
            "R1",
            crate::grid::Orientation::R90,
            true,
        );
        history
            .execute(&mut schematic, Command::AddSymbol(symbol))
            .unwrap();
        schematic
    }

    #[test]
    fn round_trips_a_schematic() {
        let schematic = sample();
        let json = to_json(&schematic).unwrap();
        let restored = from_json(&json).unwrap();
        assert_eq!(restored, schematic);
    }

    #[test]
    fn round_trips_a_shunt_reference_symbol_and_its_properties() {
        let mut schematic = Schematic::new();
        let mut history = crate::undo::UndoStack::new();
        let registry = crate::symbol::SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::ShuntReference).unwrap();
        let id = crate::symbol::SymbolId(schematic.allocate_id().unwrap());
        let symbol = crate::symbol::Symbol::from_def(
            id,
            definition,
            crate::grid::GridPos::new(2, 3),
            "U1",
            crate::grid::Orientation::R0,
            false,
        );
        history
            .execute(
                &mut schematic,
                crate::command::Command::AddSymbol(symbol.clone()),
            )
            .unwrap();

        let json = to_json(&schematic).unwrap();
        let restored = from_json(&json).unwrap();

        assert_eq!(restored, schematic);
        assert_eq!(restored.symbol(id), Some(&symbol));
        assert!(json.contains("ShuntReference"));
    }

    #[test]
    fn round_trips_a_comparator_symbol_and_its_properties() {
        let mut schematic = Schematic::new();
        let mut history = crate::undo::UndoStack::new();
        let registry = crate::symbol::SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::Comparator).unwrap();
        let id = crate::symbol::SymbolId(schematic.allocate_id().unwrap());
        let symbol = crate::symbol::Symbol::from_def(
            id,
            definition,
            crate::grid::GridPos::new(4, 2),
            "U1",
            crate::grid::Orientation::R0,
            false,
        );
        history
            .execute(
                &mut schematic,
                crate::command::Command::AddSymbol(symbol.clone()),
            )
            .unwrap();

        let json = to_json(&schematic).unwrap();
        let restored = from_json(&json).unwrap();

        assert_eq!(restored, schematic);
        assert_eq!(restored.symbol(id), Some(&symbol));
        assert!(json.contains("Comparator"));
    }

    #[test]
    fn round_trips_a_mosfet_symbol_and_its_properties() {
        let mut schematic = Schematic::new();
        let mut history = crate::undo::UndoStack::new();
        let registry = crate::symbol::SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::Mosfet).unwrap();
        let id = crate::symbol::SymbolId(schematic.allocate_id().unwrap());
        let mut symbol = crate::symbol::Symbol::from_def(
            id,
            definition,
            crate::grid::GridPos::new(5, 2),
            "M1",
            crate::grid::Orientation::R0,
            false,
        );
        symbol.params.insert(
            "channel".to_owned(),
            sim_core::netlist::ParamValue::Text("p".to_owned()),
        );
        symbol.params.insert(
            "swap_ds".to_owned(),
            sim_core::netlist::ParamValue::Bool(true),
        );
        history
            .execute(
                &mut schematic,
                crate::command::Command::AddSymbol(symbol.clone()),
            )
            .unwrap();

        let json = to_json(&schematic).unwrap();
        let restored = from_json(&json).unwrap();

        assert_eq!(restored, schematic);
        assert_eq!(restored.symbol(id), Some(&symbol));
        assert!(json.contains("Mosfet"));
        let restored_symbol = restored.symbol(id).unwrap();
        assert_eq!(
            restored_symbol
                .params
                .get("channel")
                .and_then(sim_core::netlist::ParamValue::as_text),
            Some("p")
        );
        assert_eq!(
            restored_symbol
                .params
                .get("swap_ds")
                .and_then(sim_core::netlist::ParamValue::as_bool),
            Some(true)
        );
    }

    #[test]
    fn loads_a_version_one_project_without_mosfets() {
        let json = r#"{
            "schema_version": 1,
            "schematic": {
                "symbols": {
                    "1": {
                        "id": 1,
                        "kind": "Resistor",
                        "at": { "x": 0, "y": 0 },
                        "orientation": "R0",
                        "mirrored": false,
                        "label": "R1",
                        "params": {
                            "resistance": { "kind": "number", "value": 1000.0 }
                        }
                    }
                },
                "wires": {
                    "2": {
                        "id": 2,
                        "a": { "x": 0, "y": 0 },
                        "b": { "x": 10, "y": 0 }
                    }
                },
                "junctions": {},
                "labels": {},
                "next_id": 3
            }
        }"#;
        let schematic = from_json(json).unwrap();
        let registry = SymbolRegistry::builtin();
        let mosfet = registry.get(&SymbolKind::Mosfet).unwrap();
        // A project saved before the MOSFET existed keeps its own symbols and
        // gains the documented defaults only when a MOSFET is placed.
        assert_eq!(schematic.symbols().count(), 1);
        assert_eq!(
            mosfet
                .defaults
                .get("channel")
                .and_then(sim_core::netlist::ParamValue::as_text),
            Some("n")
        );
        assert_eq!(
            mosfet
                .defaults
                .get("vth")
                .and_then(sim_core::netlist::ParamValue::as_number),
            Some(1.6)
        );
    }

    #[test]
    fn rejects_future_schema_versions_before_parsing_the_payload() {
        let json = r#"{"schema_version":99,"future":true,"schematic":{"layout":"unknown"}}"#;
        assert!(matches!(
            from_json(json),
            Err(DocumentError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn rejects_missing_schema_versions() {
        let json =
            r#"{"schematic":{"symbols":{},"wires":{},"junctions":{},"labels":{},"next_id":1}}"#;
        assert!(matches!(
            from_json(json),
            Err(DocumentError::InvalidVersion)
        ));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(from_json("{"), Err(DocumentError::Json(_))));
    }

    #[test]
    fn loads_a_legacy_version_one_fixture() {
        let json = r#"{
            "schema_version": 1,
            "schematic": {
                "symbols": {
                    "1": {
                        "id": 1,
                        "kind": "Resistor",
                        "at": { "x": 0, "y": 0 },
                        "orientation": "R0",
                        "mirrored": false,
                        "label": "R1",
                        "params": {
                            "resistance": { "kind": "number", "value": 1000.0 }
                        }
                    }
                },
                "wires": {
                    "2": {
                        "id": 2,
                        "a": { "x": 0, "y": 0 },
                        "b": { "x": 10, "y": 0 }
                    }
                },
                "junctions": {},
                "labels": {},
                "next_id": 3
            }
        }"#;
        let schematic = from_json(json).unwrap();
        assert_eq!(schematic.symbols().count(), 1);
        assert_eq!(schematic.wires().count(), 1);
        assert_eq!(schematic.next_id(), 3);
        assert_eq!(
            schematic
                .symbol(crate::symbol::SymbolId(1))
                .map(|symbol| symbol.label.as_str()),
            Some("R1")
        );
    }
}
