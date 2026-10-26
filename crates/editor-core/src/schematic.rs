//! Schematic object model and validation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sim_core::netlist::{validate_name, NameError, ParamValue};
use thiserror::Error;

use crate::grid::{point_on_segment, GridPos};
use crate::symbol::{Symbol, SymbolId};

/// Identifier of a wire segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WireId(pub u32);

/// Identifier of an explicit junction dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct JunctionId(pub u32);

/// Identifier of a net label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NetLabelId(pub u32);

/// Typed reference to any editable schematic object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ObjectRef {
    Symbol(SymbolId),
    Wire(WireId),
    Junction(JunctionId),
    Label(NetLabelId),
}

/// Geometry error for an axis-aligned wire.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum WireGeometryError {
    #[error("wire has zero length")]
    ZeroLength,
    #[error("wire is not axis-aligned")]
    NotOrthogonal,
    #[error("wire lies outside the supported grid")]
    OutOfBounds,
}

/// Orthogonal wire segment between two grid positions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire {
    pub id: WireId,
    pub a: GridPos,
    pub b: GridPos,
}

impl Wire {
    /// Creates a normalized wire.
    pub fn new(id: WireId, a: GridPos, b: GridPos) -> Result<Self, WireGeometryError> {
        let mut wire = Self { id, a, b };
        if wire.a > wire.b {
            std::mem::swap(&mut wire.a, &mut wire.b);
        }
        wire.validate()?;
        Ok(wire)
    }

    /// Validates length, orientation, and bounds.
    pub fn validate(&self) -> Result<(), WireGeometryError> {
        if self.a == self.b {
            return Err(WireGeometryError::ZeroLength);
        }
        if self.a.x != self.b.x && self.a.y != self.b.y {
            return Err(WireGeometryError::NotOrthogonal);
        }
        if !self.a.is_within_bounds() || !self.b.is_within_bounds() {
            return Err(WireGeometryError::OutOfBounds);
        }
        Ok(())
    }

    /// Returns true when the point lies on the closed segment.
    pub fn contains(&self, point: GridPos) -> bool {
        point_on_segment(point, self.a, self.b)
    }

    /// Returns true for a horizontal segment.
    pub const fn is_horizontal(&self) -> bool {
        self.a.y == self.b.y
    }

    /// Returns true for a vertical segment.
    pub const fn is_vertical(&self) -> bool {
        self.a.x == self.b.x
    }

    /// Length in grid units.
    pub fn length(&self) -> u32 {
        self.a.x.abs_diff(self.b.x) + self.a.y.abs_diff(self.b.y)
    }
}

/// Explicit junction dot. It connects crossing wires at its position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Junction {
    pub id: JunctionId,
    pub at: GridPos,
}

impl Junction {
    /// Creates a junction.
    pub const fn new(id: JunctionId, at: GridPos) -> Self {
        Self { id, at }
    }
}

/// Named net label placed at a grid position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetLabel {
    pub id: NetLabelId,
    pub at: GridPos,
    pub text: String,
}

impl NetLabel {
    /// Creates a validated net label with trimmed text.
    pub fn new(id: NetLabelId, at: GridPos, text: impl Into<String>) -> Result<Self, NameError> {
        let text = text.into();
        validate_name(&text)?;
        Ok(Self {
            id,
            at,
            text: text.trim().to_owned(),
        })
    }
}

/// Validation error for the schematic graph.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum SchematicError {
    #[error("invalid id counter {next_id} (maximum object id is {max_id})")]
    InvalidNextId { next_id: u32, max_id: u32 },
    #[error("object identifier space is exhausted")]
    IdSpaceExhausted,
    #[error("object {object:?} has identifier 0")]
    ZeroObjectId { object: ObjectRef },
    #[error("object {object:?} has id value {actual}, which does not match its map key")]
    IdMismatch { object: ObjectRef, actual: u32 },
    #[error("object {object:?} lies outside the supported grid")]
    PositionOutOfBounds { object: ObjectRef },
    #[error("wire {wire:?} is invalid: {source}")]
    InvalidWire {
        wire: WireId,
        source: WireGeometryError,
    },
    #[error("symbol {symbol:?} has an invalid kind name: {source}")]
    InvalidKind { symbol: SymbolId, source: NameError },
    #[error("object {object:?} has an invalid name: {source}")]
    InvalidName {
        object: ObjectRef,
        source: NameError,
    },
    #[error("duplicate symbol label '{label}'")]
    DuplicateSymbolLabel { label: String },
    #[error("symbol {symbol:?} has an invalid parameter name '{key}': {source}")]
    InvalidParamName {
        symbol: SymbolId,
        key: String,
        source: NameError,
    },
    #[error("symbol {symbol:?} parameter '{key}' is not finite")]
    NonFiniteParam { symbol: SymbolId, key: String },
    #[error("duplicate junction at {at:?}")]
    DuplicateJunction { at: GridPos },
}

/// Complete schematic document in grid coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schematic {
    pub(crate) symbols: BTreeMap<SymbolId, Symbol>,
    pub(crate) wires: BTreeMap<WireId, Wire>,
    pub(crate) junctions: BTreeMap<JunctionId, Junction>,
    pub(crate) labels: BTreeMap<NetLabelId, NetLabel>,
    pub(crate) next_id: u32,
}

impl Schematic {
    /// Creates an empty schematic with id counter 1.
    pub fn new() -> Self {
        Self {
            symbols: BTreeMap::new(),
            wires: BTreeMap::new(),
            junctions: BTreeMap::new(),
            labels: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Iterates over symbols in identifier order.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.values()
    }

    /// Iterates over wires in identifier order.
    pub fn wires(&self) -> impl Iterator<Item = &Wire> {
        self.wires.values()
    }

    /// Iterates over explicit junctions in identifier order.
    pub fn junctions(&self) -> impl Iterator<Item = &Junction> {
        self.junctions.values()
    }

    /// Iterates over net labels in identifier order.
    pub fn labels(&self) -> impl Iterator<Item = &NetLabel> {
        self.labels.values()
    }

    /// Looks up a symbol.
    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(&id)
    }

    /// Looks up a wire.
    pub fn wire(&self, id: WireId) -> Option<&Wire> {
        self.wires.get(&id)
    }

    /// Looks up a junction.
    pub fn junction(&self, id: JunctionId) -> Option<&Junction> {
        self.junctions.get(&id)
    }

    /// Looks up a net label.
    pub fn label(&self, id: NetLabelId) -> Option<&NetLabel> {
        self.labels.get(&id)
    }

    /// Returns the next identifier that will be allocated.
    pub const fn next_id(&self) -> u32 {
        self.next_id
    }

    /// Allocates a fresh object identifier.
    pub fn allocate_id(&mut self) -> Result<u32, SchematicError> {
        if self.next_id == 0 || self.next_id == u32::MAX {
            return Err(SchematicError::IdSpaceExhausted);
        }
        let id = self.next_id;
        self.next_id += 1;
        Ok(id)
    }

    /// Returns true when no object is present.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
            && self.wires.is_empty()
            && self.junctions.is_empty()
            && self.labels.is_empty()
    }

    /// Compares editable content while ignoring the monotonic id counter.
    pub fn content_eq(&self, other: &Self) -> bool {
        self.symbols == other.symbols
            && self.wires == other.wires
            && self.junctions == other.junctions
            && self.labels == other.labels
    }

    /// Returns all object references in deterministic order.
    pub fn object_refs(&self) -> Vec<ObjectRef> {
        let mut refs = Vec::with_capacity(
            self.symbols.len() + self.wires.len() + self.junctions.len() + self.labels.len(),
        );
        refs.extend(self.symbols.keys().copied().map(ObjectRef::Symbol));
        refs.extend(self.wires.keys().copied().map(ObjectRef::Wire));
        refs.extend(self.junctions.keys().copied().map(ObjectRef::Junction));
        refs.extend(self.labels.keys().copied().map(ObjectRef::Label));
        refs
    }

    /// Validates identifiers, names, parameters, and geometry.
    pub fn validate(&self) -> Result<(), SchematicError> {
        let mut max_id = 0;
        let mut symbol_labels = BTreeSet::new();

        for (key, symbol) in &self.symbols {
            max_id = max_id.max(key.0).max(symbol.id.0);
            validate_object_id(ObjectRef::Symbol(*key), key.0)?;
            if key != &symbol.id {
                return Err(SchematicError::IdMismatch {
                    object: ObjectRef::Symbol(*key),
                    actual: symbol.id.0,
                });
            }
            validate_position(ObjectRef::Symbol(*key), symbol.at)?;
            validate_name(symbol.kind.key()).map_err(|source| SchematicError::InvalidKind {
                symbol: symbol.id,
                source,
            })?;
            validate_name(&symbol.label).map_err(|source| SchematicError::InvalidName {
                object: ObjectRef::Symbol(symbol.id),
                source,
            })?;
            if !symbol_labels.insert(symbol.label.trim().to_owned()) {
                return Err(SchematicError::DuplicateSymbolLabel {
                    label: symbol.label.trim().to_owned(),
                });
            }
            for (key, value) in &symbol.params {
                validate_name(key).map_err(|source| SchematicError::InvalidParamName {
                    symbol: symbol.id,
                    key: key.clone(),
                    source,
                })?;
                if matches!(value, ParamValue::Number(number) if !number.is_finite()) {
                    return Err(SchematicError::NonFiniteParam {
                        symbol: symbol.id,
                        key: key.clone(),
                    });
                }
            }
        }

        for (key, wire) in &self.wires {
            max_id = max_id.max(key.0).max(wire.id.0);
            validate_object_id(ObjectRef::Wire(*key), key.0)?;
            if key != &wire.id {
                return Err(SchematicError::IdMismatch {
                    object: ObjectRef::Wire(*key),
                    actual: wire.id.0,
                });
            }
            wire.validate()
                .map_err(|source| SchematicError::InvalidWire {
                    wire: wire.id,
                    source,
                })?;
        }

        let mut junction_positions = BTreeSet::new();
        for (key, junction) in &self.junctions {
            max_id = max_id.max(key.0).max(junction.id.0);
            validate_object_id(ObjectRef::Junction(*key), key.0)?;
            if key != &junction.id {
                return Err(SchematicError::IdMismatch {
                    object: ObjectRef::Junction(*key),
                    actual: junction.id.0,
                });
            }
            validate_position(ObjectRef::Junction(*key), junction.at)?;
            if !junction_positions.insert(junction.at) {
                return Err(SchematicError::DuplicateJunction { at: junction.at });
            }
        }

        for (key, label) in &self.labels {
            max_id = max_id.max(key.0).max(label.id.0);
            validate_object_id(ObjectRef::Label(*key), key.0)?;
            if key != &label.id {
                return Err(SchematicError::IdMismatch {
                    object: ObjectRef::Label(*key),
                    actual: label.id.0,
                });
            }
            validate_position(ObjectRef::Label(*key), label.at)?;
            validate_name(&label.text).map_err(|source| SchematicError::InvalidName {
                object: ObjectRef::Label(*key),
                source,
            })?;
        }

        if self.next_id == 0 || self.next_id == u32::MAX || self.next_id <= max_id {
            return Err(SchematicError::InvalidNextId {
                next_id: self.next_id,
                max_id,
            });
        }
        Ok(())
    }
}

impl Default for Schematic {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_object_id(object: ObjectRef, id: u32) -> Result<(), SchematicError> {
    if id == 0 {
        Err(SchematicError::ZeroObjectId { object })
    } else {
        Ok(())
    }
}

fn validate_position(object: ObjectRef, position: GridPos) -> Result<(), SchematicError> {
    if position.is_within_bounds() {
        Ok(())
    } else {
        Err(SchematicError::PositionOutOfBounds { object })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Orientation;
    use crate::symbol::SymbolKind;

    fn pos(x: i32, y: i32) -> GridPos {
        GridPos::new(x, y)
    }

    fn symbol(id: u32, label: &str) -> Symbol {
        Symbol {
            id: SymbolId(id),
            kind: SymbolKind::Resistor,
            at: pos(0, 0),
            orientation: Orientation::R0,
            mirrored: false,
            label: label.to_owned(),
            params: Default::default(),
        }
    }

    #[test]
    fn normalizes_and_validates_wires() {
        let wire = Wire::new(WireId(1), pos(10, 0), pos(0, 0)).unwrap();
        assert_eq!(wire.a, pos(0, 0));
        assert_eq!(wire.b, pos(10, 0));
        assert!(wire.contains(pos(5, 0)));
        assert!(!wire.contains(pos(5, 1)));
        assert_eq!(
            Wire::new(WireId(1), pos(0, 0), pos(0, 0)),
            Err(WireGeometryError::ZeroLength)
        );
        assert_eq!(
            Wire::new(WireId(1), pos(0, 0), pos(1, 1)),
            Err(WireGeometryError::NotOrthogonal)
        );
    }

    #[test]
    fn validates_schematic_invariants() {
        let mut schematic = Schematic::new();
        let id = schematic.allocate_id().unwrap();
        schematic.symbols.insert(SymbolId(id), symbol(id, "R1"));
        assert_eq!(schematic.validate(), Ok(()));

        schematic.symbols.insert(SymbolId(99), symbol(99, "R1"));
        assert!(matches!(
            schematic.validate(),
            Err(SchematicError::DuplicateSymbolLabel { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_junctions() {
        let mut schematic = Schematic::new();
        let first = schematic.allocate_id().unwrap();
        let second = schematic.allocate_id().unwrap();
        schematic.junctions.insert(
            JunctionId(first),
            Junction::new(JunctionId(first), pos(1, 1)),
        );
        schematic.junctions.insert(
            JunctionId(second),
            Junction::new(JunctionId(second), pos(1, 1)),
        );
        assert!(matches!(
            schematic.validate(),
            Err(SchematicError::DuplicateJunction { .. })
        ));
    }

    #[test]
    fn content_equality_ignores_next_id() {
        let mut left = Schematic::new();
        let mut right = Schematic::new();
        left.allocate_id().unwrap();
        right.allocate_id().unwrap();
        right.allocate_id().unwrap();
        assert!(left.content_eq(&right));
        assert_ne!(left, right);
    }

    #[test]
    fn net_labels_are_trimmed_and_named() {
        let label = NetLabel::new(NetLabelId(1), pos(0, 0), "  OUT  ").unwrap();
        assert_eq!(label.text, "OUT");
        assert!(NetLabel::new(NetLabelId(2), pos(0, 0), "   ").is_err());
    }
}
