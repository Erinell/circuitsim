//! Symbol definitions, pin geometry, and the extensible symbol registry.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use sim_core::netlist::{validate_name, NameError, ParamMap, ParamValue};
use thiserror::Error;

use crate::grid::{GridPos, Orientation};

/// Kind of schematic symbol, including extension kinds registered by name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
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
    Voltmeter,
    Ammeter,
    Ohmmeter,
    Custom(String),
    ShuntReference,
    Comparator,
}

impl SymbolKind {
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
            Self::ShuntReference => "shunt_reference",
            Self::Comparator => "comparator",
            Self::OpAmp => "opamp",
            Self::Voltmeter => "voltmeter",
            Self::Ammeter => "ammeter",
            Self::Ohmmeter => "ohmmeter",
            Self::Custom(key) => key,
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
            "shunt_reference" => Some(Self::ShuntReference),
            "comparator" => Some(Self::Comparator),
            "opamp" => Some(Self::OpAmp),
            "voltmeter" => Some(Self::Voltmeter),
            "ammeter" => Some(Self::Ammeter),
            "ohmmeter" => Some(Self::Ohmmeter),
            _ => None,
        }
    }

    /// Returns true for the ground symbol.
    pub const fn is_ground(&self) -> bool {
        matches!(self, Self::Ground)
    }

    /// Returns true for a measurement probe excluded from the netlist.
    pub const fn is_measurement(&self) -> bool {
        matches!(self, Self::Voltmeter | Self::Ammeter | Self::Ohmmeter)
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

/// One pin of a symbol definition, in local symbol coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinDef {
    pub name: String,
    pub at: GridPos,
}

impl PinDef {
    /// Creates a pin definition.
    pub fn new(name: impl Into<String>, at: GridPos) -> Self {
        Self {
            name: name.into(),
            at,
        }
    }
}

/// Validation error for a symbol definition or registry.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum SymbolError {
    #[error("invalid symbol kind name: {source}")]
    InvalidKindName { source: NameError },
    #[error("symbol kind '{kind}' is already registered")]
    DuplicateKind { kind: String },
    #[error("symbol has no pins")]
    NoPins,
    #[error("symbol has too many pins")]
    TooManyPins,
    #[error("invalid pin name '{pin}': {source}")]
    InvalidPinName { pin: String, source: NameError },
    #[error("duplicate pin name '{pin}'")]
    DuplicatePinName { pin: String },
    #[error("pin '{pin}' lies outside the supported grid")]
    PinOutOfBounds { pin: String },
    #[error("invalid default parameter name '{key}': {source}")]
    InvalidDefaultName { key: String, source: NameError },
    #[error("default parameter '{key}' is not finite")]
    NonFiniteDefault { key: String },
}

/// Complete definition of a symbol kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolDef {
    pub kind: SymbolKind,
    pub pins: Vec<PinDef>,
    pub defaults: ParamMap,
}

impl SymbolDef {
    /// Creates and validates a symbol definition.
    pub fn new(
        kind: SymbolKind,
        pins: Vec<PinDef>,
        defaults: ParamMap,
    ) -> Result<Self, SymbolError> {
        validate_name(kind.key()).map_err(|source| SymbolError::InvalidKindName { source })?;
        if pins.is_empty() {
            return Err(SymbolError::NoPins);
        }
        if pins.len() > usize::from(u16::MAX) {
            return Err(SymbolError::TooManyPins);
        }
        let mut names = BTreeSet::new();
        for pin in &pins {
            validate_name(&pin.name).map_err(|source| SymbolError::InvalidPinName {
                pin: pin.name.clone(),
                source,
            })?;
            if !pin.at.is_within_bounds() {
                return Err(SymbolError::PinOutOfBounds {
                    pin: pin.name.clone(),
                });
            }
            if !names.insert(pin.name.clone()) {
                return Err(SymbolError::DuplicatePinName {
                    pin: pin.name.clone(),
                });
            }
        }
        for (key, value) in &defaults {
            validate_name(key).map_err(|source| SymbolError::InvalidDefaultName {
                key: key.clone(),
                source,
            })?;
            if matches!(value, ParamValue::Number(number) if !number.is_finite()) {
                return Err(SymbolError::NonFiniteDefault { key: key.clone() });
            }
        }
        Ok(Self {
            kind,
            pins,
            defaults,
        })
    }

    /// Number of pins.
    pub fn pin_count(&self) -> usize {
        self.pins.len()
    }

    /// Finds a pin index by name.
    pub fn pin_index(&self, name: &str) -> Option<usize> {
        self.pins.iter().position(|pin| pin.name == name)
    }
}

/// Registry of symbol definitions available in the editor.
#[derive(Clone, Debug)]
pub struct SymbolRegistry {
    defs: BTreeMap<String, SymbolDef>,
}

impl SymbolRegistry {
    /// Creates a registry containing all built-in symbols.
    pub fn new() -> Self {
        Self::builtin()
    }

    /// Creates an empty registry for tests or custom applications.
    pub fn empty() -> Self {
        Self {
            defs: BTreeMap::new(),
        }
    }

    /// Creates a registry containing all built-in symbols.
    pub fn builtin() -> Self {
        let mut registry = Self::empty();
        for def in builtin_defs() {
            registry
                .register(def)
                .expect("built-in symbol definitions are unique and valid");
        }
        registry
    }

    /// Registers a symbol definition.
    pub fn register(&mut self, def: SymbolDef) -> Result<(), SymbolError> {
        let key = def.kind.key().to_owned();
        if self.defs.contains_key(&key) {
            return Err(SymbolError::DuplicateKind { kind: key });
        }
        self.defs.insert(key, def);
        Ok(())
    }

    /// Looks up a definition by kind.
    pub fn get(&self, kind: &SymbolKind) -> Option<&SymbolDef> {
        self.defs.get(kind.key())
    }

    /// Looks up a definition by registry key.
    pub fn get_key(&self, key: &str) -> Option<&SymbolDef> {
        self.defs.get(key)
    }

    /// Iterates over definitions in deterministic key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &SymbolDef)> {
        self.defs.iter().map(|(key, def)| (key.as_str(), def))
    }

    /// Number of registered definitions.
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// Returns true when no symbol is registered.
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

impl Default for SymbolRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

/// Placed symbol instance in a schematic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub kind: SymbolKind,
    pub at: GridPos,
    pub orientation: Orientation,
    pub mirrored: bool,
    pub label: String,
    pub params: ParamMap,
}

/// Identifier of a placed symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub u32);

impl Symbol {
    /// Creates a symbol with no parameters.
    pub fn new(id: SymbolId, kind: SymbolKind, at: GridPos, label: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            at,
            orientation: Orientation::R0,
            mirrored: false,
            label: label.into(),
            params: ParamMap::new(),
        }
    }

    /// Creates a symbol using a definition's defaults.
    pub fn from_def(
        id: SymbolId,
        def: &SymbolDef,
        at: GridPos,
        label: impl Into<String>,
        orientation: Orientation,
        mirrored: bool,
    ) -> Self {
        Self {
            id,
            kind: def.kind.clone(),
            at,
            orientation,
            mirrored,
            label: label.into(),
            params: def.defaults.clone(),
        }
    }

    /// Transforms a local pin position into schematic coordinates.
    pub fn pin_position(&self, local: GridPos) -> Option<GridPos> {
        let mirrored_x = if self.mirrored {
            local.x.checked_neg()?
        } else {
            local.x
        };
        let rotated = self.orientation.rotate(GridPos::new(mirrored_x, local.y));
        self.at.checked_add(rotated)
    }

    /// Returns the transformed position of a pin definition.
    pub fn pin_position_of(&self, pin: &PinDef) -> Option<GridPos> {
        self.pin_position(pin.at)
    }

    /// Iterates over pins and their transformed positions.
    pub fn pins<'a>(
        &'a self,
        def: &'a SymbolDef,
    ) -> impl Iterator<Item = (&'a PinDef, Option<GridPos>)> + 'a {
        def.pins.iter().map(|pin| (pin, self.pin_position_of(pin)))
    }
}

fn builtin_defs() -> Vec<SymbolDef> {
    vec![
        two_terminal(
            SymbolKind::Resistor,
            &[("resistance", 1_000.0), ("imax", 0.25)],
        ),
        two_terminal(
            SymbolKind::Capacitor,
            &[("capacitance", 1e-6), ("imax", 1.0)],
        ),
        two_terminal(
            SymbolKind::Inductor,
            &[("inductance", 1e-3), ("resistance", 0.01), ("imax", 0.5)],
        ),
        two_terminal(SymbolKind::VoltageSource, &[("dc", 5.0), ("imax", 10.0)]),
        two_terminal(SymbolKind::CurrentSource, &[("dc", 0.001), ("imax", 1.0)]),
        SymbolDef::new(
            SymbolKind::Ground,
            vec![PinDef::new("gnd", GridPos::new(0, 0))],
            ParamMap::new(),
        )
        .expect("ground definition is valid"),
        switch_def(),
        diode_def(),
        two_terminal(
            SymbolKind::Led,
            &[
                ("wavelength", 620.0),
                ("vf", 2.0),
                ("if", 0.02),
                ("imax", 0.03),
            ],
        ),
        two_terminal(
            SymbolKind::Zener,
            &[
                ("is", 1e-14),
                ("n", 1.0),
                ("bv", 5.1),
                ("ibv", 1e-3),
                ("nbv", 1.0),
                ("imax", 0.2),
            ],
        ),
        shunt_reference_def(),
        comparator_def(),
        SymbolDef::new(
            SymbolKind::OpAmp,
            vec![
                PinDef::new("in+", GridPos::new(-2, 1)),
                PinDef::new("in-", GridPos::new(-2, -1)),
                PinDef::new("out", GridPos::new(2, 0)),
                PinDef::new("v+", GridPos::new(0, 2)),
                PinDef::new("v-", GridPos::new(0, -2)),
            ],
            numeric_params(&[("gain", 1e5), ("v_sat_pos", 15.0), ("v_sat_neg", -15.0)]),
        )
        .expect("op-amp definition is valid"),
    ]
    .into_iter()
    .chain(probe_defs())
    .collect()
}

fn probe_defs() -> Vec<SymbolDef> {
    [
        SymbolKind::Voltmeter,
        SymbolKind::Ammeter,
        SymbolKind::Ohmmeter,
    ]
    .into_iter()
    .map(|kind| {
        SymbolDef::new(
            kind,
            vec![PinDef::new("probe", GridPos::new(0, 0))],
            ParamMap::new(),
        )
        .expect("probe definition is valid")
    })
    .collect()
}

fn diode_def() -> SymbolDef {
    let mut defaults = numeric_params(&[("vf", 0.7), ("if", 0.01), ("n", 1.0), ("imax", 0.2)]);
    defaults.insert("type".to_owned(), ParamValue::Text("standard".to_owned()));
    SymbolDef::new(
        SymbolKind::Diode,
        vec![
            PinDef::new("a", GridPos::new(-2, 0)),
            PinDef::new("b", GridPos::new(2, 0)),
        ],
        defaults,
    )
    .expect("diode definition is valid")
}

fn shunt_reference_def() -> SymbolDef {
    SymbolDef::new(
        SymbolKind::ShuntReference,
        vec![
            PinDef::new("ref", GridPos::new(0, 2)),
            PinDef::new("k", GridPos::new(-2, 0)),
            PinDef::new("a", GridPos::new(2, 0)),
        ],
        numeric_params(&[
            ("vref", 2.495),
            ("iref", 2e-6),
            ("ik_min", 1e-3),
            ("imax", 0.1),
            ("z_ka", 0.2),
        ]),
    )
    .expect("shunt reference definition is valid")
}

fn comparator_def() -> SymbolDef {
    SymbolDef::new(
        SymbolKind::Comparator,
        vec![
            PinDef::new("in+", GridPos::new(-2, 1)),
            PinDef::new("in-", GridPos::new(-2, -1)),
            PinDef::new("out", GridPos::new(2, 0)),
            PinDef::new("vcc", GridPos::new(0, 2)),
            PinDef::new("vss", GridPos::new(0, -2)),
        ],
        numeric_params(&[("v_offset", 2e-3), ("r_sink", 20.0), ("imax", 0.02)]),
    )
    .expect("comparator definition is valid")
}

fn two_terminal(kind: SymbolKind, defaults: &[(&str, f64)]) -> SymbolDef {
    SymbolDef::new(
        kind,
        vec![
            PinDef::new("a", GridPos::new(-2, 0)),
            PinDef::new("b", GridPos::new(2, 0)),
        ],
        numeric_params(defaults),
    )
    .expect("two-terminal definition is valid")
}

fn switch_def() -> SymbolDef {
    let mut defaults = numeric_params(&[("ron", 1e-3), ("roff", 1e18), ("imax", 1.0)]);
    defaults.insert("closed".to_owned(), ParamValue::Bool(false));
    SymbolDef::new(
        SymbolKind::Switch,
        vec![
            PinDef::new("a", GridPos::new(-2, 0)),
            PinDef::new("b", GridPos::new(2, 0)),
        ],
        defaults,
    )
    .expect("switch definition is valid")
}

fn numeric_params(values: &[(&str, f64)]) -> ParamMap {
    values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), ParamValue::Number(*value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_contains_expected_symbols() {
        let registry = SymbolRegistry::builtin();
        assert_eq!(registry.len(), 16);
        assert_eq!(
            registry
                .get(&SymbolKind::Resistor)
                .map(SymbolDef::pin_count),
            Some(2)
        );
        assert_eq!(
            registry.get(&SymbolKind::OpAmp).map(SymbolDef::pin_count),
            Some(5)
        );
        assert_eq!(
            registry.get(&SymbolKind::Ground).map(SymbolDef::pin_count),
            Some(1)
        );
        assert_eq!(
            registry
                .get(&SymbolKind::ShuntReference)
                .map(SymbolDef::pin_count),
            Some(3)
        );
    }

    #[test]
    fn led_and_inductor_expose_the_requested_parameters() {
        let registry = SymbolRegistry::builtin();
        let led = registry.get(&SymbolKind::Led).unwrap();
        for key in ["wavelength", "vf", "if", "imax"] {
            assert!(led.defaults.contains_key(key), "missing LED param {key}");
        }
        let inductor = registry.get(&SymbolKind::Inductor).unwrap();
        assert!(inductor.defaults.contains_key("inductance"));
        assert!(inductor.defaults.contains_key("resistance"));
        assert!(inductor.defaults.contains_key("imax"));
        for kind in [
            SymbolKind::Resistor,
            SymbolKind::Capacitor,
            SymbolKind::VoltageSource,
            SymbolKind::CurrentSource,
            SymbolKind::Switch,
            SymbolKind::Zener,
        ] {
            let definition = registry.get(&kind).unwrap();
            assert!(
                definition.defaults.contains_key("imax"),
                "missing imax for {kind:?}"
            );
        }
        let source = registry.get(&SymbolKind::VoltageSource).unwrap();
        assert_eq!(
            source.defaults.get("imax").and_then(ParamValue::as_number),
            Some(10.0)
        );
        let diode = registry.get(&SymbolKind::Diode).unwrap();
        for key in ["type", "vf", "if", "n", "imax"] {
            assert!(
                diode.defaults.contains_key(key),
                "missing diode param {key}"
            );
        }
        assert_eq!(
            diode.defaults.get("type").and_then(ParamValue::as_text),
            Some("standard")
        );
    }

    #[test]
    fn rejects_duplicate_pins_and_kinds() {
        let duplicate_pins = SymbolDef::new(
            SymbolKind::Custom("custom".to_owned()),
            vec![
                PinDef::new("a", GridPos::new(0, 0)),
                PinDef::new("a", GridPos::new(1, 0)),
            ],
            ParamMap::new(),
        );
        assert!(matches!(
            duplicate_pins,
            Err(SymbolError::DuplicatePinName { .. })
        ));

        let mut registry = SymbolRegistry::empty();
        let def = SymbolDef::new(
            SymbolKind::Custom("custom".to_owned()),
            vec![PinDef::new("a", GridPos::new(0, 0))],
            ParamMap::new(),
        )
        .unwrap();
        registry.register(def.clone()).unwrap();
        assert_eq!(
            registry.register(def),
            Err(SymbolError::DuplicateKind {
                kind: "custom".to_owned()
            })
        );
    }

    #[test]
    fn transforms_pins_with_rotation_and_mirror() {
        let registry = SymbolRegistry::builtin();
        let def = registry.get(&SymbolKind::Resistor).unwrap();
        let mut symbol = Symbol::from_def(
            SymbolId(1),
            def,
            GridPos::new(10, 20),
            "R1",
            Orientation::R0,
            false,
        );
        assert_eq!(
            symbol.pin_position_of(&def.pins[0]),
            Some(GridPos::new(8, 20))
        );
        symbol.orientation = Orientation::R90;
        assert_eq!(
            symbol.pin_position_of(&def.pins[0]),
            Some(GridPos::new(10, 18))
        );
        symbol.mirrored = true;
        assert_eq!(
            symbol.pin_position_of(&def.pins[0]),
            Some(GridPos::new(10, 22))
        );
    }

    #[test]
    fn default_parameters_are_copied() {
        let registry = SymbolRegistry::builtin();
        let def = registry.get(&SymbolKind::Resistor).unwrap();
        let symbol = Symbol::from_def(
            SymbolId(1),
            def,
            GridPos::new(0, 0),
            "R1",
            Orientation::R0,
            false,
        );
        assert_eq!(
            symbol
                .params
                .get("resistance")
                .and_then(ParamValue::as_number),
            Some(1_000.0)
        );
    }

    #[test]
    fn switch_and_zener_defaults_match_the_simulation_contract() {
        let registry = SymbolRegistry::builtin();
        let switch = registry.get(&SymbolKind::Switch).unwrap();
        assert_eq!(
            switch.defaults.get("closed").and_then(ParamValue::as_bool),
            Some(false)
        );
        assert_eq!(
            switch.defaults.get("ron").and_then(ParamValue::as_number),
            Some(1e-3)
        );
        let zener = registry.get(&SymbolKind::Zener).unwrap();
        assert_eq!(
            zener.defaults.get("ibv").and_then(ParamValue::as_number),
            Some(1e-3)
        );
        assert_eq!(
            zener.defaults.get("nbv").and_then(ParamValue::as_number),
            Some(1.0)
        );
    }

    #[test]
    fn shunt_reference_has_ordered_pins_and_tl431_defaults() {
        let registry = SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::ShuntReference).unwrap();
        assert_eq!(
            definition
                .pins
                .iter()
                .map(|pin| pin.name.as_str())
                .collect::<Vec<_>>(),
            ["ref", "k", "a"]
        );
        for (key, value) in [
            ("vref", 2.495),
            ("iref", 2e-6),
            ("ik_min", 1e-3),
            ("imax", 0.1),
            ("z_ka", 0.2),
        ] {
            assert_eq!(
                definition.defaults.get(key).and_then(ParamValue::as_number),
                Some(value)
            );
        }
        assert_eq!(
            SymbolKind::from_key("shunt_reference"),
            Some(SymbolKind::ShuntReference)
        );
    }

    #[test]
    fn comparator_has_lm393_pins_and_defaults() {
        let registry = SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::Comparator).unwrap();
        assert_eq!(
            definition
                .pins
                .iter()
                .map(|pin| pin.name.as_str())
                .collect::<Vec<_>>(),
            ["in+", "in-", "out", "vcc", "vss"]
        );
        for (key, value) in [("v_offset", 2e-3), ("r_sink", 20.0), ("imax", 0.02)] {
            assert_eq!(
                definition.defaults.get(key).and_then(ParamValue::as_number),
                Some(value)
            );
        }
        assert_eq!(
            SymbolKind::from_key("comparator"),
            Some(SymbolKind::Comparator)
        );
    }
}
