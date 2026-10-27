//! Configurable placement shortcuts and their conflict checks.

use editor_core::SymbolKind;
use eframe::egui::Key;

/// One placement key mapped to a symbol kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementBinding {
    /// Key used to arm the placement tool.
    pub key: Key,
    /// Symbol kind placed by the key.
    pub kind: KindSlot,
}

/// Copyable symbol-kind slot used by the keymap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindSlot {
    /// Resistor.
    Resistor,
    /// Capacitor.
    Capacitor,
    /// Inductor.
    Inductor,
    /// Diode.
    Diode,
    /// Voltage source.
    VoltageSource,
    /// Ground.
    Ground,
    /// Switch.
    Switch,
}

impl KindSlot {
    /// Returns the editor symbol kind.
    pub fn symbol_kind(self) -> SymbolKind {
        match self {
            Self::Resistor => SymbolKind::Resistor,
            Self::Capacitor => SymbolKind::Capacitor,
            Self::Inductor => SymbolKind::Inductor,
            Self::Diode => SymbolKind::Diode,
            Self::VoltageSource => SymbolKind::VoltageSource,
            Self::Ground => SymbolKind::Ground,
            Self::Switch => SymbolKind::Switch,
        }
    }

    /// Returns the French component name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Resistor => "Résistance",
            Self::Capacitor => "Condensateur",
            Self::Inductor => "Bobine",
            Self::Diode => "Diode",
            Self::VoltageSource => "Source de tension",
            Self::Ground => "Masse",
            Self::Switch => "Interrupteur",
        }
    }

    /// Returns the displayed key.
    pub const fn key_label(self) -> &'static str {
        match self {
            Self::Resistor => "R",
            Self::Capacitor => "C",
            Self::Inductor => "L",
            Self::Diode => "D",
            Self::VoltageSource => "V",
            Self::Ground => "G",
            Self::Switch => "S",
        }
    }
}

/// Placement keymap. The structure is ready for persisted remapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    bindings: [PlacementBinding; 7],
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: [
                PlacementBinding {
                    key: Key::R,
                    kind: KindSlot::Resistor,
                },
                PlacementBinding {
                    key: Key::C,
                    kind: KindSlot::Capacitor,
                },
                PlacementBinding {
                    key: Key::L,
                    kind: KindSlot::Inductor,
                },
                PlacementBinding {
                    key: Key::D,
                    kind: KindSlot::Diode,
                },
                PlacementBinding {
                    key: Key::V,
                    kind: KindSlot::VoltageSource,
                },
                PlacementBinding {
                    key: Key::G,
                    kind: KindSlot::Ground,
                },
                PlacementBinding {
                    key: Key::S,
                    kind: KindSlot::Switch,
                },
            ],
        }
    }
}

impl Keymap {
    /// Returns the placement kind bound to a key.
    pub fn placement(&self, key: Key) -> Option<KindSlot> {
        self.bindings
            .iter()
            .find_map(|binding| (binding.key == key).then_some(binding.kind))
    }

    /// Returns every placement binding in deterministic order.
    pub const fn bindings(&self) -> &[PlacementBinding; 7] {
        &self.bindings
    }

    /// Returns keys used by at least two bindings.
    pub fn conflicts(&self) -> Vec<Key> {
        let mut keys = self
            .bindings
            .iter()
            .map(|binding| binding.key)
            .collect::<Vec<_>>();
        keys.sort_by_key(|key| format!("{key:?}"));
        keys.windows(2)
            .filter_map(|pair| (pair[0] == pair[1]).then_some(pair[0]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keymap_has_no_conflict() {
        let keymap = Keymap::default();
        assert!(keymap.conflicts().is_empty());
        assert_eq!(keymap.placement(Key::R), Some(KindSlot::Resistor));
        assert_eq!(keymap.placement(Key::V), Some(KindSlot::VoltageSource));
        assert_eq!(keymap.placement(Key::L), Some(KindSlot::Inductor));
        assert_eq!(keymap.placement(Key::W), None);
    }

    #[test]
    fn table_covers_every_slot_once() {
        let keymap = Keymap::default();
        let mut kinds = keymap
            .bindings()
            .iter()
            .map(|binding| binding.kind)
            .collect::<Vec<_>>();
        kinds.sort_by_key(|kind| format!("{kind:?}"));
        kinds.dedup();
        assert_eq!(kinds.len(), 7);
    }
}
