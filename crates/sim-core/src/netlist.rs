//! Validated netlist model shared by the editor, worker, and simulator.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum number of characters accepted in a node, component, or parameter name.
pub const MAX_NAME_LEN: usize = 128;

/// Identifier of an electrical node inside a netlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// Identifier of a component instance inside a netlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ComponentId(pub u32);

/// Validation error for a user-visible name.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum NameError {
    #[error("name is empty")]
    Empty,
    #[error("name exceeds {max} characters (got {actual})")]
    TooLong { max: usize, actual: usize },
    #[error("name contains a control character")]
    ControlCharacter,
}

/// Validates a node, component, parameter, or label name.
pub fn validate_name(name: &str) -> Result<(), NameError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(NameError::Empty);
    }
    let length = trimmed.chars().count();
    if length > MAX_NAME_LEN {
        return Err(NameError::TooLong {
            max: MAX_NAME_LEN,
            actual: length,
        });
    }
    if trimmed.chars().any(char::is_control) {
        return Err(NameError::ControlCharacter);
    }
    Ok(())
}

/// Deterministic name of an electrical node.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetName(String);

impl NetName {
    /// Creates a validated net name, trimming surrounding whitespace.
    pub fn new(name: impl Into<String>) -> Result<Self, NameError> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self(name.trim().to_owned()))
    }

    /// Name of the reference ground node.
    pub fn ground() -> Self {
        Self("0".to_owned())
    }

    /// Deterministic generated name for an unnamed net.
    pub fn generated(index: u32) -> Self {
        Self(format!("N${index:03}"))
    }

    /// Returns the name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Revalidates a name loaded from an untrusted source.
    pub fn validate(&self) -> Result<(), NameError> {
        validate_name(&self.0)
    }
}

impl fmt::Display for NetName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Role of a node in the circuit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Ground,
    Electrical,
    Internal { owner: ComponentId, index: u16 },
}

impl NodeKind {
    /// Returns true for the reference node.
    pub const fn is_ground(&self) -> bool {
        matches!(self, Self::Ground)
    }
}

/// Electrical node in a netlist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: NetName,
    pub kind: NodeKind,
}

/// Supported component families. `Custom` names are resolved by a registry.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ComponentKind {
    Resistor,
    Capacitor,
    Inductor,
    VoltageSource,
    CurrentSource,
    Ground,
    Switch,
    Diode,
    Led,
    Zener,
    OpAmp,
    Custom(String),
}

impl ComponentKind {
    /// Stable registry key.
    pub fn key(&self) -> &str {
        match self {
            Self::Resistor => "resistor",
            Self::Capacitor => "capacitor",
            Self::Inductor => "inductor",
            Self::VoltageSource => "vsource",
            Self::CurrentSource => "isource",
            Self::Ground => "ground",
            Self::Switch => "switch",
            Self::Diode => "diode",
            Self::Led => "led",
            Self::Zener => "zener",
            Self::OpAmp => "opamp",
            Self::Custom(name) => name,
        }
    }

    /// Number of pins for built-in components.
    pub const fn known_pin_count(&self) -> Option<usize> {
        match self {
            Self::Ground => Some(1),
            Self::OpAmp => Some(5),
            Self::Resistor
            | Self::Capacitor
            | Self::Inductor
            | Self::VoltageSource
            | Self::CurrentSource
            | Self::Switch
            | Self::Diode
            | Self::Led
            | Self::Zener => Some(2),
            Self::Custom(_) => None,
        }
    }

    /// Resolves a built-in registry key.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "resistor" => Some(Self::Resistor),
            "capacitor" => Some(Self::Capacitor),
            "inductor" => Some(Self::Inductor),
            "vsource" => Some(Self::VoltageSource),
            "isource" => Some(Self::CurrentSource),
            "ground" => Some(Self::Ground),
            "switch" => Some(Self::Switch),
            "diode" => Some(Self::Diode),
            "led" => Some(Self::Led),
            "zener" => Some(Self::Zener),
            "opamp" => Some(Self::OpAmp),
            _ => None,
        }
    }
}

impl fmt::Display for ComponentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

/// Typed parameter value. Numbers are always stored in SI units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ParamValue {
    Number(f64),
    Bool(bool),
    Text(String),
}

impl ParamValue {
    /// Returns the numeric value when this parameter is a number.
    pub const fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Bool(_) | Self::Text(_) => None,
        }
    }

    /// Returns the boolean value when this parameter is a boolean.
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Number(_) | Self::Text(_) => None,
        }
    }

    /// Returns the text value when this parameter is text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Number(_) | Self::Bool(_) => None,
        }
    }
}

/// Deterministic map of component parameters.
pub type ParamMap = BTreeMap<String, ParamValue>;

/// Serializable description of one component instance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentSpec {
    pub id: ComponentId,
    pub kind: ComponentKind,
    pub label: String,
    pub params: ParamMap,
    pub pins: Vec<NodeId>,
}

/// Validated circuit netlist. Node and component identifiers equal their indices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Netlist {
    pub nodes: Vec<Node>,
    pub components: Vec<ComponentSpec>,
}

/// Structural or semantic netlist error.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum NetlistError {
    #[error("too many nodes")]
    TooManyNodes,
    #[error("too many components")]
    TooManyComponents,
    #[error("node at index {index} has id {actual:?}")]
    NodeIdMismatch { index: usize, actual: NodeId },
    #[error("component at index {index} has id {actual:?}")]
    ComponentIdMismatch { index: usize, actual: ComponentId },
    #[error("node {node:?} has an invalid name: {source}")]
    InvalidNodeName { node: NodeId, source: NameError },
    #[error("duplicate node name '{name}'")]
    DuplicateNodeName { name: String },
    #[error("component {component:?} has an invalid label: {source}")]
    InvalidComponentLabel {
        component: ComponentId,
        source: NameError,
    },
    #[error("duplicate component label '{label}'")]
    DuplicateComponentLabel { label: String },
    #[error("component {component:?} has an invalid custom kind: {source}")]
    InvalidCustomKind {
        component: ComponentId,
        source: NameError,
    },
    #[error("component {component:?} has no pins")]
    NoPins { component: ComponentId },
    #[error("component {component:?} pin {pin} references invalid node {node:?}")]
    InvalidPin {
        component: ComponentId,
        pin: usize,
        node: NodeId,
    },
    #[error("component {component:?} ({kind}) expects {expected} pins, got {actual}")]
    PinCountMismatch {
        component: ComponentId,
        kind: String,
        expected: usize,
        actual: usize,
    },
    #[error("component {component:?} has an invalid parameter name '{key}': {source}")]
    InvalidParamName {
        component: ComponentId,
        key: String,
        source: NameError,
    },
    #[error("component {component:?} parameter '{key}' is not finite")]
    NonFiniteParam { component: ComponentId, key: String },
    #[error("ground component {component:?} is connected to non-ground node {node:?}")]
    GroundComponentNotGround {
        component: ComponentId,
        node: NodeId,
    },
    #[error("internal node {node:?} references invalid owner {owner:?}")]
    InternalOwner { node: NodeId, owner: ComponentId },
    #[error("component {owner:?} has duplicate internal node index {index}")]
    DuplicateInternalIndex { owner: ComponentId, index: u16 },
    #[error("netlist has no ground node")]
    NoGround,
    #[error("netlist has {count} ground nodes, expected exactly one")]
    MultipleGround { count: usize },
    #[error("ground node is named '{name}', expected '0'")]
    InvalidGroundName { name: String },
}

impl Netlist {
    /// Creates a netlist and validates its structural invariants.
    pub fn new(nodes: Vec<Node>, components: Vec<ComponentSpec>) -> Result<Self, NetlistError> {
        let netlist = Self { nodes, components };
        netlist.validate()?;
        Ok(netlist)
    }

    /// Creates an empty structurally valid netlist.
    pub const fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            components: Vec::new(),
        }
    }

    /// Validates structural invariants, allowing zero or multiple ground nodes.
    pub fn validate(&self) -> Result<(), NetlistError> {
        let mut node_names = BTreeSet::new();
        let mut internal_indices = BTreeSet::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let expected = u32::try_from(index).map_err(|_| NetlistError::TooManyNodes)?;
            if node.id != NodeId(expected) {
                return Err(NetlistError::NodeIdMismatch {
                    index,
                    actual: node.id,
                });
            }
            node.name
                .validate()
                .map_err(|source| NetlistError::InvalidNodeName {
                    node: node.id,
                    source,
                })?;
            if !node_names.insert(node.name.as_str().to_owned()) {
                return Err(NetlistError::DuplicateNodeName {
                    name: node.name.as_str().to_owned(),
                });
            }
            if let NodeKind::Internal { owner, index } = node.kind {
                let owner_index =
                    usize::try_from(owner.0).map_err(|_| NetlistError::InternalOwner {
                        node: node.id,
                        owner,
                    })?;
                if owner_index >= self.components.len() {
                    return Err(NetlistError::InternalOwner {
                        node: node.id,
                        owner,
                    });
                }
                if !internal_indices.insert((owner, index)) {
                    return Err(NetlistError::DuplicateInternalIndex { owner, index });
                }
            }
        }

        let mut labels = BTreeSet::new();
        for (index, component) in self.components.iter().enumerate() {
            let expected = u32::try_from(index).map_err(|_| NetlistError::TooManyComponents)?;
            if component.id != ComponentId(expected) {
                return Err(NetlistError::ComponentIdMismatch {
                    index,
                    actual: component.id,
                });
            }
            validate_name(&component.label).map_err(|source| {
                NetlistError::InvalidComponentLabel {
                    component: component.id,
                    source,
                }
            })?;
            if !labels.insert(component.label.trim().to_owned()) {
                return Err(NetlistError::DuplicateComponentLabel {
                    label: component.label.trim().to_owned(),
                });
            }
            if let ComponentKind::Custom(name) = &component.kind {
                validate_name(name).map_err(|source| NetlistError::InvalidCustomKind {
                    component: component.id,
                    source,
                })?;
            }
            if component.pins.is_empty() {
                return Err(NetlistError::NoPins {
                    component: component.id,
                });
            }
            if let Some(expected) = component.kind.known_pin_count() {
                if component.pins.len() != expected {
                    return Err(NetlistError::PinCountMismatch {
                        component: component.id,
                        kind: component.kind.key().to_owned(),
                        expected,
                        actual: component.pins.len(),
                    });
                }
            }
            for (pin, node) in component.pins.iter().enumerate() {
                let node_index = usize::try_from(node.0).map_err(|_| NetlistError::InvalidPin {
                    component: component.id,
                    pin,
                    node: *node,
                })?;
                if node_index >= self.nodes.len() {
                    return Err(NetlistError::InvalidPin {
                        component: component.id,
                        pin,
                        node: *node,
                    });
                }
            }
            for (key, value) in &component.params {
                validate_name(key).map_err(|source| NetlistError::InvalidParamName {
                    component: component.id,
                    key: key.clone(),
                    source,
                })?;
                if matches!(value, ParamValue::Number(number) if !number.is_finite()) {
                    return Err(NetlistError::NonFiniteParam {
                        component: component.id,
                        key: key.clone(),
                    });
                }
            }
            if component.kind == ComponentKind::Ground {
                let node = component.pins[0];
                let node_is_ground = self
                    .nodes
                    .get(usize::try_from(node.0).unwrap_or(usize::MAX))
                    .is_some_and(|target| target.kind.is_ground());
                if !node_is_ground {
                    return Err(NetlistError::GroundComponentNotGround {
                        component: component.id,
                        node,
                    });
                }
            }
        }
        Ok(())
    }

    /// Validates the netlist for simulation, requiring exactly one ground named `0`.
    pub fn validate_for_simulation(&self) -> Result<(), NetlistError> {
        self.validate()?;
        let grounds: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|node| node.kind.is_ground())
            .collect();
        match grounds.len() {
            0 => Err(NetlistError::NoGround),
            1 => {
                let ground = grounds[0];
                if ground.name.as_str() != "0" {
                    return Err(NetlistError::InvalidGroundName {
                        name: ground.name.as_str().to_owned(),
                    });
                }
                Ok(())
            }
            count => Err(NetlistError::MultipleGround { count }),
        }
    }

    /// Returns the unique ground node, if present.
    pub fn ground_node(&self) -> Option<&Node> {
        self.nodes.iter().find(|node| node.kind.is_ground())
    }

    /// Returns the unique ground node id, if present.
    pub fn ground_node_id(&self) -> Option<NodeId> {
        self.ground_node().map(|node| node.id)
    }

    /// Looks up a node by id.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(usize::try_from(id.0).ok()?)
    }

    /// Looks up a component by id.
    pub fn component(&self, id: ComponentId) -> Option<&ComponentSpec> {
        self.components.get(usize::try_from(id.0).ok()?)
    }

    /// Looks up a node by name.
    pub fn node_by_name(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.name.as_str() == name)
    }

    /// Looks up a component by label.
    pub fn component_by_label(&self, label: &str) -> Option<&ComponentSpec> {
        self.components
            .iter()
            .find(|component| component.label == label)
    }
}

/// Incremental netlist builder with deterministic identifier allocation.
#[derive(Debug, Default)]
pub struct NetlistBuilder {
    nodes: Vec<Node>,
    components: Vec<ComponentSpec>,
    node_names: BTreeSet<String>,
    labels: BTreeSet<String>,
}

impl NetlistBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node and returns its identifier.
    pub fn add_node(
        &mut self,
        name: impl Into<String>,
        kind: NodeKind,
    ) -> Result<NodeId, NetlistError> {
        let name = NetName::new(name).map_err(|source| NetlistError::InvalidNodeName {
            node: NodeId(u32::try_from(self.nodes.len()).unwrap_or(u32::MAX)),
            source,
        })?;
        if !self.node_names.insert(name.as_str().to_owned()) {
            return Err(NetlistError::DuplicateNodeName {
                name: name.as_str().to_owned(),
            });
        }
        let id = NodeId(u32::try_from(self.nodes.len()).map_err(|_| NetlistError::TooManyNodes)?);
        self.nodes.push(Node { id, name, kind });
        Ok(id)
    }

    /// Adds a component and returns its identifier.
    pub fn add_component(
        &mut self,
        kind: ComponentKind,
        label: impl Into<String>,
        params: ParamMap,
        pins: Vec<NodeId>,
    ) -> Result<ComponentId, NetlistError> {
        let label = label.into();
        validate_name(&label).map_err(|source| NetlistError::InvalidComponentLabel {
            component: ComponentId(u32::try_from(self.components.len()).unwrap_or(u32::MAX)),
            source,
        })?;
        if !self.labels.insert(label.trim().to_owned()) {
            return Err(NetlistError::DuplicateComponentLabel {
                label: label.trim().to_owned(),
            });
        }
        let id = ComponentId(
            u32::try_from(self.components.len()).map_err(|_| NetlistError::TooManyComponents)?,
        );
        self.components.push(ComponentSpec {
            id,
            kind,
            label,
            params,
            pins,
        });
        Ok(id)
    }

    /// Finalizes and validates the netlist.
    pub fn build(self) -> Result<Netlist, NetlistError> {
        Netlist::new(self.nodes, self.components)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resistor_params() -> ParamMap {
        let mut params = ParamMap::new();
        params.insert("resistance".to_owned(), ParamValue::Number(1_000.0));
        params
    }

    fn simple_netlist() -> Netlist {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let output = builder.add_node("out", NodeKind::Electrical).unwrap();
        builder
            .add_component(
                ComponentKind::VoltageSource,
                "V1",
                ParamMap::new(),
                vec![output, ground],
            )
            .unwrap();
        builder
            .add_component(
                ComponentKind::Resistor,
                "R1",
                resistor_params(),
                vec![output, ground],
            )
            .unwrap();
        builder.build().unwrap()
    }

    #[test]
    fn builds_and_validates_a_simple_netlist() {
        let netlist = simple_netlist();
        assert!(netlist.validate().is_ok());
        assert!(netlist.validate_for_simulation().is_ok());
        assert_eq!(netlist.ground_node_id(), Some(NodeId(0)));
        assert_eq!(
            netlist.node_by_name("out").map(|node| node.id),
            Some(NodeId(1))
        );
        assert_eq!(
            netlist
                .component_by_label("R1")
                .map(|component| component.id),
            Some(ComponentId(1))
        );
    }

    #[test]
    fn rejects_missing_ground_for_simulation() {
        let netlist = Netlist {
            nodes: vec![Node {
                id: NodeId(0),
                name: NetName::new("out").unwrap(),
                kind: NodeKind::Electrical,
            }],
            components: Vec::new(),
        };
        assert_eq!(netlist.validate(), Ok(()));
        assert_eq!(
            netlist.validate_for_simulation(),
            Err(NetlistError::NoGround)
        );
    }

    #[test]
    fn rejects_multiple_grounds_for_simulation() {
        let mut builder = NetlistBuilder::new();
        builder.add_node("0", NodeKind::Ground).unwrap();
        builder.add_node("gnd2", NodeKind::Ground).unwrap();
        let netlist = builder.build().unwrap();
        assert_eq!(
            netlist.validate_for_simulation(),
            Err(NetlistError::MultipleGround { count: 2 })
        );
    }

    #[test]
    fn rejects_duplicate_labels() {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        builder
            .add_component(ComponentKind::Ground, "GND1", ParamMap::new(), vec![ground])
            .unwrap();
        let source = builder.add_node("source", NodeKind::Electrical).unwrap();
        let duplicate = builder.add_component(
            ComponentKind::Resistor,
            "GND1",
            ParamMap::new(),
            vec![source, ground],
        );
        assert_eq!(
            duplicate,
            Err(NetlistError::DuplicateComponentLabel {
                label: "GND1".to_owned()
            })
        );
    }

    #[test]
    fn rejects_invalid_pin_reference() {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        builder
            .add_component(
                ComponentKind::Resistor,
                "R1",
                ParamMap::new(),
                vec![ground, NodeId(9)],
            )
            .unwrap();
        assert!(matches!(
            builder.build(),
            Err(NetlistError::InvalidPin { .. })
        ));
    }

    #[test]
    fn rejects_non_finite_parameters() {
        let mut params = ParamMap::new();
        params.insert("resistance".to_owned(), ParamValue::Number(f64::NAN));
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let output = builder.add_node("out", NodeKind::Electrical).unwrap();
        builder
            .add_component(ComponentKind::Resistor, "R1", params, vec![output, ground])
            .unwrap();
        assert!(matches!(
            builder.build(),
            Err(NetlistError::NonFiniteParam { .. })
        ));
    }

    #[test]
    fn validates_internal_node_ownership() {
        let mut builder = NetlistBuilder::new();
        builder
            .add_node(
                "internal",
                NodeKind::Internal {
                    owner: ComponentId(4),
                    index: 0,
                },
            )
            .unwrap();
        assert!(matches!(
            builder.build(),
            Err(NetlistError::InternalOwner { .. })
        ));
    }

    #[test]
    fn validates_names() {
        assert!(NetName::new("out").is_ok());
        assert_eq!(NetName::new("  ").unwrap_err(), NameError::Empty);
        assert!(matches!(
            NetName::new("x".repeat(MAX_NAME_LEN + 1)),
            Err(NameError::TooLong { .. })
        ));
        assert_eq!(
            NetName::new("a\nb").unwrap_err(),
            NameError::ControlCharacter
        );
    }

    #[test]
    fn rejects_invalid_custom_kind_names() {
        let mut builder = NetlistBuilder::new();
        let node = builder.add_node("x", NodeKind::Electrical).unwrap();
        builder
            .add_component(
                ComponentKind::Custom("bad\nkind".to_owned()),
                "X1",
                ParamMap::new(),
                vec![node],
            )
            .unwrap();
        assert!(matches!(
            builder.build(),
            Err(NetlistError::InvalidCustomKind { .. })
        ));
    }

    #[test]
    fn custom_components_accept_any_positive_pin_count() {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let node = builder.add_node("x", NodeKind::Electrical).unwrap();
        builder
            .add_component(
                ComponentKind::Custom("custom".to_owned()),
                "X1",
                ParamMap::new(),
                vec![node, ground, node],
            )
            .unwrap();
        assert!(builder.build().is_ok());
    }
}
