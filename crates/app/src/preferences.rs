//! Versioned native and web persistence for interface preferences.

use editor_core::SymbolKind;
use serde::{Deserialize, Serialize};

use crate::ui::theme::CanvasSettings;

/// Current preference schema version.
pub const PREFERENCES_VERSION: u32 = 1;
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "circuitsim.preferences.v1";

/// Persisted interface preferences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersistedPreferences {
    /// Schema version.
    pub version: u32,
    /// Interface scale.
    pub scale: f32,
    /// Reduce animations.
    pub reduce_motion: bool,
    /// Canvas theme, layers, palettes and display options.
    pub canvas: CanvasSettings,
    /// Component palette visibility.
    pub show_palette: bool,
    /// Inspector visibility.
    pub show_inspector: bool,
    /// Classic instrument panel visibility.
    pub show_instruments: bool,
    /// Inline scopes visibility.
    pub show_inline_scopes: bool,
    /// Playback speed.
    pub realtime_speed: f64,
    /// Current animation speed.
    pub animation_speed: f64,
    /// Power glow strength.
    pub power_brightness: f32,
    /// Favorite component kinds.
    pub favorites: Vec<SymbolKind>,
}

impl Default for PersistedPreferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            scale: 1.0,
            reduce_motion: false,
            canvas: CanvasSettings::default(),
            show_palette: true,
            show_inspector: true,
            show_instruments: false,
            show_inline_scopes: true,
            realtime_speed: 1.0,
            animation_speed: 1.0,
            power_brightness: 0.6,
            favorites: Vec::new(),
        }
    }
}

impl PersistedPreferences {
    /// Serializes the preferences.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|error| error.to_string())
    }

    /// Parses preferences, rejecting unsupported versions.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let parsed: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if parsed.version != PREFERENCES_VERSION {
            return Err(format!(
                "version de préférences non prise en charge : {}",
                parsed.version
            ));
        }
        Ok(parsed)
    }
}

/// Loads preferences, returning `None` when absent or invalid.
pub fn load() -> Option<PersistedPreferences> {
    let text = load_text()?;
    PersistedPreferences::from_json(&text).ok()
}

/// Saves preferences to the native config file or web local storage.
pub fn save(preferences: &PersistedPreferences) -> Result<(), String> {
    let text = preferences.to_json()?;
    save_text(&text)
}

#[cfg(not(target_arch = "wasm32"))]
fn config_path() -> Option<std::path::PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(
            std::path::PathBuf::from(appdata)
                .join("CircuitSim")
                .join("preferences.json"),
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Some(
            std::path::PathBuf::from(home)
                .join(".config")
                .join("circuitsim")
                .join("preferences.json"),
        );
    }
    std::env::current_dir()
        .ok()
        .map(|path| path.join(".circuitsim-preferences.json"))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_text() -> Option<String> {
    std::fs::read_to_string(config_path()?).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn save_text(text: &str) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "aucun chemin de configuration".to_owned())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, text).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
fn load_text() -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item(STORAGE_KEY).ok()?
}

#[cfg(target_arch = "wasm32")]
fn save_text(text: &str) -> Result<(), String> {
    let storage = web_sys::window()
        .and_then(|window| window.local_storage().ok())
        .flatten()
        .ok_or_else(|| "stockage local indisponible".to_owned())?;
    storage
        .set_item(STORAGE_KEY, text)
        .map_err(|error| format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip() {
        let mut preferences = PersistedPreferences {
            scale: 1.15,
            ..PersistedPreferences::default()
        };
        preferences.canvas.small_grid = true;
        preferences.favorites.push(SymbolKind::Led);
        let text = preferences.to_json().unwrap();
        let restored = PersistedPreferences::from_json(&text).unwrap();
        assert_eq!(restored, preferences);
    }

    #[test]
    fn corrupt_or_future_preferences_are_ignored() {
        assert!(PersistedPreferences::from_json("{").is_err());
        let preferences = PersistedPreferences {
            version: PREFERENCES_VERSION + 1,
            ..PersistedPreferences::default()
        };
        let text = preferences.to_json().unwrap();
        assert!(PersistedPreferences::from_json(&text).is_err());
    }

    #[test]
    fn defaults_are_safe() {
        let preferences = PersistedPreferences::default();
        assert_eq!(preferences.version, PREFERENCES_VERSION);
        assert_eq!(preferences.scale, 1.0);
        assert!(preferences.show_inline_scopes);
        assert!(!preferences.show_instruments);
    }
}
