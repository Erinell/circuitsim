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
}
