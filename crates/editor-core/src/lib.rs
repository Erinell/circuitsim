//! Schematic editing model and netlist extraction for CircuitSim.

pub mod command;
pub mod erc;
pub mod extract;
pub mod grid;
pub mod naming;
pub mod schematic;
pub mod serialize;
pub mod symbol;
pub mod undo;

pub use command::{Command, EditError};
pub use erc::{ErcCode, ErcIssue, ErcReport, ErcSeverity};
pub use extract::{extract_netlist, ExtractError, ExtractedNet, Extraction, PinKey};
pub use grid::{GridPos, Orientation, SegmentRelation};
pub use schematic::{
    Junction, JunctionId, NetLabel, NetLabelId, ObjectRef, Schematic, SchematicError, Wire,
    WireGeometryError, WireId,
};
pub use serialize::{from_json, to_json, DocumentError, SchematicFile, SCHEMA_VERSION};
pub use symbol::{PinDef, Symbol, SymbolDef, SymbolError, SymbolId, SymbolKind, SymbolRegistry};
pub use undo::UndoStack;
