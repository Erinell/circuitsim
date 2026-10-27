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

/// One documented shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShortcutEntry {
    /// Displayed key combination.
    pub keys: &'static str,
    /// French action description.
    pub action: &'static str,
}

/// Single shortcut table used by the help window and tooltips.
pub const SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry {
        keys: "W / J / N",
        action: "outils Fil / Jonction / Label",
    },
    ShortcutEntry {
        keys: "R C L D V G S",
        action: "placer résistance, condensateur, bobine, diode, source, masse, interrupteur",
    },
    ShortcutEntry {
        keys: "Maj+R / M",
        action: "pivoter / miroir",
    },
    ShortcutEntry {
        keys: "R / F en pose",
        action: "tourner / miroir de l’aperçu",
    },
    ShortcutEntry {
        keys: "Ctrl+K",
        action: "palette de commandes",
    },
    ShortcutEntry {
        keys: "Alt+clic",
        action: "sélectionner tout le net",
    },
    ShortcutEntry {
        keys: "Ctrl+C / Ctrl+V",
        action: "copier / coller avec aperçu",
    },
    ShortcutEntry {
        keys: "Ctrl+D",
        action: "dupliquer la sélection",
    },
    ShortcutEntry {
        keys: "Espace",
        action: "marche/arrêt de la simulation",
    },
    ShortcutEntry {
        keys: "F5 / Ctrl+Entrée",
        action: "reset de la session",
    },
    ShortcutEntry {
        keys: "Suppr",
        action: "supprimer la sélection",
    },
    ShortcutEntry {
        keys: "Ctrl+Z",
        action: "annuler",
    },
    ShortcutEntry {
        keys: "Ctrl+Y",
        action: "rétablir",
    },
    ShortcutEntry {
        keys: "Ctrl+A",
        action: "tout sélectionner",
    },
    ShortcutEntry {
        keys: "Échap",
        action: "annuler l'outil, le collage ou la sélection",
    },
    ShortcutEntry {
        keys: "? / F1",
        action: "ouvrir ou fermer cette aide",
    },
    ShortcutEntry {
        keys: "Ctrl+Maj+A",
        action: "préférences d’affichage et accessibilité",
    },
    ShortcutEntry {
        keys: "Home",
        action: "ajuster la vue au schéma",
    },
    ShortcutEntry {
        keys: "Ctrl+Home",
        action: "cadrer la sélection",
    },
    ShortcutEntry {
        keys: "+ / -",
        action: "zoom avant / arrière",
    },
    ShortcutEntry {
        keys: "0",
        action: "zoom 100 %",
    },
    ShortcutEntry {
        keys: "Flèches",
        action: "déplacer la vue (Maj : pas rapide)",
    },
    ShortcutEntry {
        keys: "Ctrl+1…4",
        action: "Tout, Édition, Simulation, Canevas",
    },
];

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

    #[test]
    fn shortcut_table_is_non_empty_and_unique() {
        assert!(SHORTCUTS.len() >= 20);
        let mut keys = SHORTCUTS.iter().map(|entry| entry.keys).collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), SHORTCUTS.len());
    }
}
