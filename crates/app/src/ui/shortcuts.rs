//! Documented keyboard shortcuts shared by the help window.

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
        keys: "R / M",
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
    fn shortcut_table_is_non_empty_and_unique() {
        assert!(SHORTCUTS.len() >= 20);
        let mut keys = SHORTCUTS.iter().map(|entry| entry.keys).collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), SHORTCUTS.len());
    }

    #[test]
    fn component_placement_has_no_letter_shortcut() {
        assert!(!SHORTCUTS.iter().any(|entry| entry.keys == "R C L D V G S"));
    }

    #[test]
    fn essential_shortcuts_are_documented() {
        for expected in ["R / M", "Espace", "Ctrl+K", "F5 / Ctrl+Entrée"] {
            assert!(
                SHORTCUTS.iter().any(|entry| entry.keys == expected),
                "{expected} must be documented"
            );
        }
    }
}
