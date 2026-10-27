//! Canvas colors, layers, and voltage palettes.

use eframe::egui::Color32;

/// Visual theme applied to the schematic canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasTheme {
    /// Default dark palette.
    #[default]
    Dark,
    /// Maximum contrast for low-vision use.
    HighContrast,
    /// Light background for daylight or print-like use.
    Light,
}

impl CanvasTheme {
    /// Returns the complete canvas palette for this theme.
    pub const fn palette(self) -> CanvasPalette {
        match self {
            Self::Dark => CanvasPalette {
                background: Color32::from_rgb(15, 21, 31),
                minor_grid: Color32::from_rgb(24, 32, 46),
                major_grid: Color32::from_rgb(34, 45, 64),
                axis: Color32::from_rgb(58, 78, 108),
                wire: Color32::from_rgb(96, 196, 158),
                symbol: Color32::from_rgb(120, 184, 244),
                pin: Color32::from_rgb(200, 220, 166),
                label: Color32::from_rgb(245, 210, 128),
                muted: Color32::from_rgb(146, 164, 188),
                accent: Color32::from_rgb(255, 190, 92),
                hover: Color32::from_rgb(120, 210, 220),
                selection_fill: Color32::from_rgba_premultiplied(112, 82, 24, 120),
                panel_badge: Color32::from_rgba_premultiplied(24, 34, 48, 220),
                minimap_background: Color32::from_rgba_premultiplied(12, 18, 28, 230),
                live_badge: Color32::from_rgba_premultiplied(20, 30, 44, 228),
                wire_error: Color32::from_rgb(255, 92, 92),
                flow_color: Color32::from_rgb(255, 236, 140),
                flow_halo: Color32::from_rgba_premultiplied(20, 24, 12, 210),
            },
            Self::HighContrast => CanvasPalette {
                background: Color32::from_rgb(0, 0, 0),
                minor_grid: Color32::from_rgb(64, 64, 64),
                major_grid: Color32::from_rgb(112, 112, 112),
                axis: Color32::from_rgb(190, 190, 190),
                wire: Color32::from_rgb(0, 235, 160),
                symbol: Color32::from_rgb(255, 255, 255),
                pin: Color32::from_rgb(255, 255, 120),
                label: Color32::from_rgb(255, 255, 255),
                muted: Color32::from_rgb(220, 220, 220),
                accent: Color32::from_rgb(255, 215, 0),
                hover: Color32::from_rgb(120, 240, 255),
                selection_fill: Color32::from_rgba_premultiplied(140, 110, 0, 160),
                panel_badge: Color32::from_rgba_premultiplied(0, 0, 0, 235),
                minimap_background: Color32::from_rgba_premultiplied(0, 0, 0, 240),
                live_badge: Color32::from_rgba_premultiplied(0, 0, 0, 235),
                wire_error: Color32::from_rgb(255, 80, 80),
                flow_color: Color32::from_rgb(255, 255, 0),
                flow_halo: Color32::from_rgba_premultiplied(0, 0, 0, 230),
            },
            Self::Light => CanvasPalette {
                background: Color32::from_rgb(248, 249, 251),
                minor_grid: Color32::from_rgb(232, 235, 240),
                major_grid: Color32::from_rgb(212, 218, 226),
                axis: Color32::from_rgb(164, 174, 190),
                wire: Color32::from_rgb(30, 126, 92),
                symbol: Color32::from_rgb(28, 86, 156),
                pin: Color32::from_rgb(94, 108, 48),
                label: Color32::from_rgb(130, 78, 0),
                muted: Color32::from_rgb(92, 102, 118),
                accent: Color32::from_rgb(196, 112, 0),
                hover: Color32::from_rgb(0, 132, 150),
                selection_fill: Color32::from_rgba_premultiplied(255, 214, 120, 170),
                panel_badge: Color32::from_rgba_premultiplied(250, 250, 252, 235),
                minimap_background: Color32::from_rgba_premultiplied(238, 241, 246, 240),
                live_badge: Color32::from_rgba_premultiplied(250, 250, 252, 235),
                wire_error: Color32::from_rgb(196, 40, 40),
                flow_color: Color32::from_rgb(180, 96, 0),
                flow_halo: Color32::from_rgba_premultiplied(255, 250, 235, 220),
            },
        }
    }
}

/// Full color set used by one canvas theme.
#[derive(Clone, Copy, Debug)]
pub struct CanvasPalette {
    /// Canvas background.
    pub background: Color32,
    /// Minor grid lines.
    pub minor_grid: Color32,
    /// Major grid lines.
    pub major_grid: Color32,
    /// Axes at the origin.
    pub axis: Color32,
    /// Default wire color.
    pub wire: Color32,
    /// Symbol body color.
    pub symbol: Color32,
    /// Pin terminal color.
    pub pin: Color32,
    /// Net label color.
    pub label: Color32,
    /// Secondary text color.
    pub muted: Color32,
    /// Selection color.
    pub accent: Color32,
    /// Hover color.
    pub hover: Color32,
    /// Selection rectangle fill.
    pub selection_fill: Color32,
    /// Tool and status badge background.
    pub panel_badge: Color32,
    /// Minimap background.
    pub minimap_background: Color32,
    /// Simulation badge background.
    pub live_badge: Color32,
    /// ERC-problem wire color.
    pub wire_error: Color32,
    /// Current-flow dot color.
    pub flow_color: Color32,
    /// Current-flow dot halo.
    pub flow_halo: Color32,
}

/// Color palette used for net voltages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoltagePalette {
    /// Blue, neutral grey, and orange: readable for common color-vision variants.
    #[default]
    Colorblind,
    /// Cyan, neutral grey, and magenta: higher separation on light backgrounds.
    HighVisibility,
}

impl VoltagePalette {
    /// Human-readable name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Colorblind => "Daltonien (bleu/gris/orange)",
            Self::HighVisibility => "Haute visibilité (cyan/gris/magenta)",
        }
    }
}

/// Direction used by the current animation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CurrentDirection {
    /// Conventional current, from positive to negative.
    #[default]
    Conventional,
    /// Electron flow, opposite to conventional current.
    Electrons,
}

impl CurrentDirection {
    /// Human-readable name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Conventional => "Sens conventionnel",
            Self::Electrons => "Sens des électrons",
        }
    }
}

/// Body style of the resistor symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResistorStyle {
    /// Rectangular IEC symbol.
    #[default]
    European,
    /// Zig-zag ANSI symbol.
    American,
}

impl ResistorStyle {
    /// Human-readable name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::European => "Européen",
            Self::American => "Américain",
        }
    }
}

/// Independently toggleable schematic layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerSettings {
    /// Tension-based wire colors.
    pub voltage: bool,
    /// Animated current dots.
    pub current: bool,
    /// Power glow around components.
    pub power: bool,
    /// Component values.
    pub values: bool,
    /// Component references.
    pub references: bool,
    /// Net labels.
    pub net_labels: bool,
}

impl Default for LayerSettings {
    fn default() -> Self {
        Self {
            voltage: true,
            current: true,
            power: true,
            values: true,
            references: true,
            net_labels: true,
        }
    }
}

/// Canvas display settings independent from the egui panel theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasSettings {
    /// Background and base colors.
    pub theme: CanvasTheme,
    /// Independently toggleable layers.
    pub layers: LayerSettings,
    /// Voltage color palette.
    pub voltage_palette: VoltagePalette,
    /// Current animation direction.
    pub current_direction: CurrentDirection,
    /// Resistor glyph style.
    pub resistor_style: ResistorStyle,
    /// Use a finer minor grid at low zoom.
    pub small_grid: bool,
    /// Multiplier applied to value text.
    pub value_scale: f32,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            theme: CanvasTheme::Dark,
            layers: LayerSettings::default(),
            voltage_palette: VoltagePalette::default(),
            current_direction: CurrentDirection::default(),
            resistor_style: ResistorStyle::default(),
            small_grid: false,
            value_scale: 1.0,
        }
    }
}

impl CanvasSettings {
    /// Returns the active palette.
    pub const fn palette(self) -> CanvasPalette {
        self.theme.palette()
    }
}

/// Symmetric voltage range used to normalize wire colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoltageScale {
    max_abs: f64,
    min: f64,
    max: f64,
}

impl VoltageScale {
    /// Builds a scale from a finite voltage iterator.
    pub fn from_values(values: impl IntoIterator<Item = f64>) -> Option<Self> {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut max_abs: f64 = 0.0;
        let mut any = false;
        for value in values {
            if !value.is_finite() {
                continue;
            }
            any = true;
            min = min.min(value);
            max = max.max(value);
            max_abs = max_abs.max(value.abs());
        }
        any.then_some(Self { max_abs, min, max })
    }

    /// Returns the normalized position in `[-1, 1]`.
    pub fn normalize(&self, voltage: f64) -> f64 {
        if self.max_abs <= f64::MIN_POSITIVE {
            0.0
        } else {
            (voltage / self.max_abs).clamp(-1.0, 1.0)
        }
    }

    /// Returns the wire color for a voltage.
    pub fn color(&self, voltage: f64, palette: VoltagePalette) -> Color32 {
        voltage_color(self.normalize(voltage), palette)
    }

    /// Smallest displayed voltage.
    pub const fn min(&self) -> f64 {
        self.min
    }

    /// Largest displayed voltage.
    pub const fn max(&self) -> f64 {
        self.max
    }

    /// Largest absolute voltage.
    pub const fn max_abs(&self) -> f64 {
        self.max_abs
    }

    /// Returns true when the range is usable for coloring.
    pub fn is_visible(&self) -> bool {
        self.max_abs > f64::MIN_POSITIVE
    }
}

/// Continuous negative/grey/positive voltage color.
pub fn voltage_color(normalized: f64, palette: VoltagePalette) -> Color32 {
    let (negative, zero, positive) = match palette {
        VoltagePalette::Colorblind => (
            Color32::from_rgb(70, 120, 255),
            Color32::from_rgb(145, 150, 160),
            Color32::from_rgb(255, 145, 40),
        ),
        VoltagePalette::HighVisibility => (
            Color32::from_rgb(0, 190, 220),
            Color32::from_rgb(170, 170, 170),
            Color32::from_rgb(255, 70, 160),
        ),
    };
    let t = normalized.clamp(-1.0, 1.0);
    if t < 0.0 {
        lerp_color(zero, negative, -t)
    } else {
        lerp_color(zero, positive, t)
    }
}

fn lerp_color(from: Color32, to: Color32, t: f64) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voltage_scale_is_symmetric_and_bounded() {
        let scale = VoltageScale::from_values([-2.0, 1.0, 4.0]).unwrap();
        assert_eq!(scale.max_abs(), 4.0);
        assert_eq!(scale.normalize(4.0), 1.0);
        assert_eq!(scale.normalize(-8.0), -1.0);
        assert_eq!(scale.normalize(0.0), 0.0);
    }

    #[test]
    fn voltage_color_is_monotone_around_zero() {
        let scale = VoltageScale::from_values([-5.0, 0.0, 5.0]).unwrap();
        let negative = scale.color(-5.0, VoltagePalette::Colorblind);
        let zero = scale.color(0.0, VoltagePalette::Colorblind);
        let positive = scale.color(5.0, VoltagePalette::Colorblind);
        assert_ne!(negative, zero);
        assert_ne!(zero, positive);
        assert_eq!(
            scale.color(-5.0, VoltagePalette::Colorblind),
            scale.color(-10.0, VoltagePalette::Colorblind)
        );
    }

    #[test]
    fn zero_is_identical_for_every_positive_scale() {
        let left = VoltageScale::from_values([0.0, 1.0]).unwrap();
        let right = VoltageScale::from_values([0.0, 100.0]).unwrap();
        assert_eq!(
            left.color(0.0, VoltagePalette::HighVisibility),
            right.color(0.0, VoltagePalette::HighVisibility)
        );
    }

    #[test]
    fn display_layers_and_palettes_are_independent() {
        let mut settings = CanvasSettings::default();
        assert!(settings.layers.voltage);
        settings.layers.voltage = false;
        assert!(!settings.layers.voltage);
        assert!(settings.layers.current);
        settings.voltage_palette = VoltagePalette::HighVisibility;
        assert_eq!(settings.voltage_palette, VoltagePalette::HighVisibility);
        assert_ne!(
            voltage_color(-1.0, VoltagePalette::Colorblind),
            voltage_color(-1.0, VoltagePalette::HighVisibility)
        );
        assert_ne!(
            CanvasTheme::Dark.palette().background,
            CanvasTheme::Light.palette().background
        );
    }
}
