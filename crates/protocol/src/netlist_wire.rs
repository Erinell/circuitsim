//! Postcard-compatible netlist representation used by protocol frames.
//!
//! `ParamValue` uses Serde's internally tagged representation for the JSON
//! schema. Non-self-describing formats cannot deserialize that form, so the
//! protocol uses an explicit wire shape and rebuilds a validated netlist.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sim_core::netlist::{
    ComponentId, ComponentKind, ComponentSpec, NameError, NetName, Netlist, NetlistError, Node,
    NodeId, NodeKind, ParamMap, ParamValue,
};
use thiserror::Error;

pub(crate) fn serialize<S>(netlist: &Netlist, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    WireNetlist::from_netlist(netlist).serialize(serializer)
}

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Netlist, D::Error>
where
    D: Deserializer<'de>,
{
    let wire = WireNetlist::deserialize(deserializer)?;
    wire.into_netlist().map_err(D::Error::custom)
}

#[derive(Serialize, Deserialize)]
struct WireNetlist {
    nodes: Vec<WireNode>,
    components: Vec<WireComponent>,
}

impl WireNetlist {
    fn from_netlist(netlist: &Netlist) -> Self {
        Self {
            nodes: netlist.nodes.iter().map(WireNode::from_node).collect(),
            components: netlist
                .components
                .iter()
                .map(WireComponent::from_component)
                .collect(),
        }
    }

    fn into_netlist(self) -> Result<Netlist, WireError> {
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in self.nodes {
            nodes.push(node.into_node()?);
        }
        let mut components = Vec::with_capacity(self.components.len());
        for component in self.components {
            components.push(component.into_component()?);
        }
        Ok(Netlist::new(nodes, components)?)
    }
}

#[derive(Serialize, Deserialize)]
struct WireNode {
    id: u32,
    name: String,
    kind: WireNodeKind,
}

impl WireNode {
    fn from_node(node: &Node) -> Self {
        Self {
            id: node.id.0,
            name: node.name.as_str().to_owned(),
            kind: match node.kind {
                NodeKind::Ground => WireNodeKind::Ground,
                NodeKind::Electrical => WireNodeKind::Electrical,
                NodeKind::Internal { owner, index } => WireNodeKind::Internal {
                    owner: owner.0,
                    index,
                },
            },
        }
    }

    fn into_node(self) -> Result<Node, WireError> {
        let name = NetName::new(self.name).map_err(|source| WireError::InvalidNodeName {
            node: self.id,
            source,
        })?;
        Ok(Node {
            id: NodeId(self.id),
            name,
            kind: match self.kind {
                WireNodeKind::Ground => NodeKind::Ground,
                WireNodeKind::Electrical => NodeKind::Electrical,
                WireNodeKind::Internal { owner, index } => NodeKind::Internal {
                    owner: ComponentId(owner),
                    index,
                },
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
enum WireNodeKind {
    Ground,
    Electrical,
    Internal { owner: u32, index: u16 },
}

#[derive(Serialize, Deserialize)]
struct WireComponent {
    id: u32,
    kind: WireComponentKind,
    label: String,
    params: Vec<(String, WireParamValue)>,
    pins: Vec<u32>,
}

impl WireComponent {
    fn from_component(component: &ComponentSpec) -> Self {
        Self {
            id: component.id.0,
            kind: WireComponentKind::from_kind(&component.kind),
            label: component.label.clone(),
            params: component
                .params
                .iter()
                .map(|(key, value)| (key.clone(), WireParamValue::from_value(value)))
                .collect(),
            pins: component.pins.iter().map(|pin| pin.0).collect(),
        }
    }

    fn into_component(self) -> Result<ComponentSpec, WireError> {
        let mut params = ParamMap::new();
        for (key, value) in self.params {
            if params.insert(key.clone(), value.into_value()).is_some() {
                return Err(WireError::DuplicateParameter {
                    component: self.id,
                    key,
                });
            }
        }
        Ok(ComponentSpec {
            id: ComponentId(self.id),
            kind: self.kind.into_kind(),
            label: self.label,
            params,
            pins: self.pins.into_iter().map(NodeId).collect(),
        })
    }
}

#[derive(Serialize, Deserialize)]
enum WireComponentKind {
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
    ShuntReference,
}

impl WireComponentKind {
    fn from_kind(kind: &ComponentKind) -> Self {
        match kind {
            ComponentKind::Resistor => Self::Resistor,
            ComponentKind::Capacitor => Self::Capacitor,
            ComponentKind::Inductor => Self::Inductor,
            ComponentKind::VoltageSource => Self::VoltageSource,
            ComponentKind::CurrentSource => Self::CurrentSource,
            ComponentKind::Ground => Self::Ground,
            ComponentKind::Switch => Self::Switch,
            ComponentKind::Diode => Self::Diode,
            ComponentKind::Led => Self::Led,
            ComponentKind::Zener => Self::Zener,
            ComponentKind::OpAmp => Self::OpAmp,
            ComponentKind::Custom(name) => Self::Custom(name.clone()),
            ComponentKind::ShuntReference => Self::ShuntReference,
        }
    }

    fn into_kind(self) -> ComponentKind {
        match self {
            Self::Resistor => ComponentKind::Resistor,
            Self::Capacitor => ComponentKind::Capacitor,
            Self::Inductor => ComponentKind::Inductor,
            Self::VoltageSource => ComponentKind::VoltageSource,
            Self::CurrentSource => ComponentKind::CurrentSource,
            Self::Ground => ComponentKind::Ground,
            Self::Switch => ComponentKind::Switch,
            Self::Diode => ComponentKind::Diode,
            Self::Led => ComponentKind::Led,
            Self::Zener => ComponentKind::Zener,
            Self::OpAmp => ComponentKind::OpAmp,
            Self::Custom(name) => ComponentKind::Custom(name),
            Self::ShuntReference => ComponentKind::ShuntReference,
        }
    }
}

#[derive(Serialize, Deserialize)]
enum WireParamValue {
    Number(f64),
    Bool(bool),
    Text(String),
}

impl WireParamValue {
    fn from_value(value: &ParamValue) -> Self {
        match value {
            ParamValue::Number(number) => Self::Number(*number),
            ParamValue::Bool(boolean) => Self::Bool(*boolean),
            ParamValue::Text(text) => Self::Text(text.clone()),
        }
    }

    fn into_value(self) -> ParamValue {
        match self {
            Self::Number(number) => ParamValue::Number(number),
            Self::Bool(boolean) => ParamValue::Bool(boolean),
            Self::Text(text) => ParamValue::Text(text),
        }
    }
}

#[derive(Debug, Error)]
enum WireError {
    #[error("node {node} has an invalid name: {source}")]
    InvalidNodeName {
        node: u32,
        #[source]
        source: NameError,
    },
    #[error("component {component} has duplicate parameter '{key}'")]
    DuplicateParameter { component: u32, key: String },
    #[error("invalid netlist: {0}")]
    Netlist(#[from] NetlistError),
}
