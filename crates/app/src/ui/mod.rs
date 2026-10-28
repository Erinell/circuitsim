//! egui panels, canvas interaction, and application status messages.

pub mod canvas;
pub mod command_palette;
pub mod controls;
pub mod diagnostics;
pub mod erc;
pub mod glyph;
pub mod history;
pub mod instruments;
pub mod palette;
pub mod project;
pub mod properties;
pub mod scopes;
pub mod shortcuts;
pub mod theme;

use editor_core::SymbolKind;
use eframe::egui::{self, Color32};

/// Editing tool selected in the toolbar or palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Select, move, and edit existing objects.
    Select,
    /// Place a symbol of the selected kind.
    Place(SymbolKind),
    /// Draw one or two orthogonal wire segments.
    Wire,
    /// Place an explicit junction.
    Junction,
    /// Place a named net label.
    Label,
}

impl Tool {
    /// French label shown in the interface.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Select => "Sélection",
            Self::Place(_) => "Placement",
            Self::Wire => "Fil",
            Self::Junction => "Jonction",
            Self::Label => "Label",
        }
    }

    /// Returns true when the tool changes the document on click.
    pub const fn is_placement(&self) -> bool {
        !matches!(self, Self::Select)
    }
}

/// Severity of a status-bar message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusLevel {
    /// Neutral information.
    Info,
    /// Completed action.
    Success,
    /// Refused action or validation error.
    Error,
}

/// One status-bar message.
#[derive(Clone, Debug)]
pub struct Status {
    level: StatusLevel,
    text: String,
}

impl Status {
    /// Creates a neutral message.
    pub fn new_info(text: impl Into<String>) -> Self {
        Self {
            level: StatusLevel::Info,
            text: text.into(),
        }
    }

    /// Creates a success message.
    pub fn new_success(text: impl Into<String>) -> Self {
        Self {
            level: StatusLevel::Success,
            text: text.into(),
        }
    }

    /// Creates an error message.
    pub fn new_error(text: impl Into<String>) -> Self {
        Self {
            level: StatusLevel::Error,
            text: text.into(),
        }
    }

    /// Replaces the message with neutral information.
    pub fn info(&mut self, text: impl Into<String>) {
        self.level = StatusLevel::Info;
        self.text = text.into();
    }

    /// Replaces the message with a success report.
    pub fn success(&mut self, text: impl Into<String>) {
        self.level = StatusLevel::Success;
        self.text = text.into();
    }

    /// Replaces the message with an error report.
    pub fn error(&mut self, text: impl Into<String>) {
        self.level = StatusLevel::Error;
        self.text = text.into();
    }

    /// Returns the message severity.
    pub const fn level(&self) -> StatusLevel {
        self.level
    }

    /// Returns the message text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the display color for the current severity.
    pub const fn color(&self) -> Color32 {
        match self.level {
            StatusLevel::Info => Color32::from_rgb(170, 184, 202),
            StatusLevel::Success => Color32::from_rgb(112, 210, 155),
            StatusLevel::Error => Color32::from_rgb(240, 128, 128),
        }
    }

    /// Paints the message in the status bar.
    pub fn show(&self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(&self.text).color(self.color()));
    }
}

impl Default for Status {
    fn default() -> Self {
        Self::new_info("Prêt.")
    }
}

/// French display name for a symbol kind.
pub fn symbol_kind_name(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Resistor => "Résistance",
        SymbolKind::Capacitor => "Condensateur",
        SymbolKind::Inductor => "Bobine",
        SymbolKind::VoltageSource => "Source de tension",
        SymbolKind::CurrentSource => "Source de courant",
        SymbolKind::Ground => "Masse",
        SymbolKind::Switch => "Interrupteur",
        SymbolKind::Diode => "Diode",
        SymbolKind::Led => "LED",
        SymbolKind::Zener => "Zener",
        SymbolKind::ShuntReference => "Référence shunt programmable",
        SymbolKind::Comparator => "Comparateur à collecteur ouvert",
        SymbolKind::OpAmp => "Amplificateur opérationnel",
        SymbolKind::Voltmeter => "Voltmètre",
        SymbolKind::Ammeter => "Ampèremètre",
        SymbolKind::Ohmmeter => "Ohmmètre",
        SymbolKind::Custom(_) => "Composant personnalisé",
    }
}

/// Shared accent color for selected objects.
pub const ACCENT: Color32 = Color32::from_rgb(255, 190, 92);
/// Color for hovered objects.
pub const HOVER: Color32 = Color32::from_rgb(120, 210, 220);
/// Amber overcurrent warning shared by panels and scopes.
pub const WARNING: Color32 = Color32::from_rgb(240, 176, 64);
/// Red critical overcurrent shared by panels and scopes.
pub const CRITICAL: Color32 = Color32::from_rgb(235, 82, 82);
/// Background behind the schematic.
pub const CANVAS_BACKGROUND: Color32 = Color32::from_rgb(15, 21, 31);
/// Wire color.
pub const WIRE: Color32 = Color32::from_rgb(96, 196, 158);
/// Symbol body color.
pub const SYMBOL: Color32 = Color32::from_rgb(120, 184, 244);
/// Pin terminal color.
pub const PIN: Color32 = Color32::from_rgb(200, 220, 166);
/// Label color.
pub const LABEL: Color32 = Color32::from_rgb(245, 210, 128);
/// Muted value text.
pub const MUTED: Color32 = Color32::from_rgb(146, 164, 188);

/// Returns a secondary text color with readable contrast in the active theme.
pub(crate) fn muted_text_color(ui: &egui::Ui) -> Color32 {
    let visuals = ui.visuals();
    muted_text_color_for_theme(visuals.dark_mode, visuals.override_text_color.is_some())
}

fn muted_text_color_for_theme(dark_mode: bool, has_text_override: bool) -> Color32 {
    if !dark_mode {
        Color32::from_rgb(82, 96, 114)
    } else if has_text_override {
        Color32::WHITE
    } else {
        MUTED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_labels_are_stable() {
        assert_eq!(Tool::Select.label(), "Sélection");
        assert_eq!(Tool::Place(SymbolKind::Resistor).label(), "Placement");
        assert_eq!(Tool::Wire.label(), "Fil");
    }

    #[test]
    fn only_editing_tools_are_placements() {
        assert!(!Tool::Select.is_placement());
        assert!(Tool::Place(SymbolKind::Diode).is_placement());
        assert!(Tool::Junction.is_placement());
    }

    #[test]
    fn secondary_text_keeps_contrast_in_each_theme() {
        let palettes = [
            (
                muted_text_color_for_theme(true, false),
                Color32::from_rgb(20, 27, 39),
            ),
            (
                muted_text_color_for_theme(false, false),
                Color32::from_rgb(244, 247, 251),
            ),
            (muted_text_color_for_theme(true, true), Color32::BLACK),
        ];
        for (text, background) in palettes {
            assert!(
                contrast_ratio(text, background) >= 4.5,
                "secondary text contrast is {:.2}",
                contrast_ratio(text, background)
            );
        }
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f64 {
        let left = luminance(a);
        let right = luminance(b);
        (left.max(right) + 0.05) / (left.min(right) + 0.05)
    }

    fn luminance(color: Color32) -> f64 {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }
}
