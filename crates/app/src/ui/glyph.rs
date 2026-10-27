//! Shared symbol glyph painting for the canvas and the palette.

use std::f32::consts::PI;

use editor_core::{Symbol, SymbolDef, SymbolKind};
use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Stroke, Vec2};
use sim_core::netlist::ParamValue;

use crate::document::{format_parameter, parameter_unit};
use crate::ui::theme::ResistorStyle;

/// Colors used to paint one symbol glyph.
#[derive(Clone, Copy, Debug)]
pub struct GlyphColors {
    /// Body and lead color.
    pub body: Color32,
    /// Pin terminal color.
    pub pin: Color32,
    /// Reference label color.
    pub label: Color32,
    /// Parameter value color.
    pub value: Color32,
    /// Resistor body style.
    pub resistor: ResistorStyle,
}

/// Text layers painted beside a symbol.
#[derive(Clone, Copy, Debug)]
pub struct SymbolText {
    /// Draw the reference label.
    pub labels: bool,
    /// Draw the engineering value.
    pub values: bool,
    /// Multiplier applied to the value font.
    pub scale: f32,
}

impl SymbolText {
    /// No text at all.
    pub const NONE: Self = Self {
        labels: false,
        values: false,
        scale: 1.0,
    };
}

/// Paints a placed symbol around a screen-space center.
pub fn paint_symbol(
    painter: &Painter,
    symbol: &Symbol,
    definition: Option<&SymbolDef>,
    center: Pos2,
    scale: f32,
    colors: GlyphColors,
    text: SymbolText,
) {
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let stroke_width = (scale * 0.085).clamp(1.25, 2.2);
    let stroke = Stroke::new(stroke_width, colors.body);
    paint_body(painter, symbol, center, scale, stroke, colors.resistor);

    if let Some(definition) = definition {
        let pin_radius = (scale * 0.11).clamp(1.4, 2.8);
        for pin in &definition.pins {
            let position = paint_position(
                symbol,
                center,
                scale,
                Vec2::new(pin.at.x as f32, pin.at.y as f32),
            );
            painter.circle_filled(position, pin_radius, colors.pin);
        }
    } else {
        painter.rect_stroke(
            egui_rect(center, scale, 1.6, 1.1),
            2.0,
            stroke,
            eframe::egui::StrokeKind::Middle,
        );
    }

    if text.labels && !symbol.label.is_empty() {
        let font = FontId::proportional((scale * 0.72).clamp(9.0, 16.0));
        let (position, anchor) = if symbol.kind.is_ground() {
            (
                center + Vec2::new(scale * 1.35, scale * 0.15),
                Align2::LEFT_CENTER,
            )
        } else {
            (
                center + Vec2::new(0.0, -scale * (text_half_height(symbol) + 0.25)),
                Align2::CENTER_BOTTOM,
            )
        };
        painter.text(position, anchor, &symbol.label, font, colors.label);
    }

    if text.values && scale >= 13.0 {
        if let Some(value) = symbol_value(symbol) {
            let font = FontId::proportional((scale * 0.56 * text.scale).clamp(8.0, 18.0));
            let (position, anchor) = if symbol.kind.is_ground() {
                (
                    center + Vec2::new(scale * 1.35, scale * 1.0),
                    Align2::LEFT_CENTER,
                )
            } else {
                (
                    center + Vec2::new(0.0, scale * (text_half_height(symbol) + 0.15)),
                    Align2::CENTER_TOP,
                )
            };
            painter.text(position, anchor, value, font, colors.value);
        }
    }
}

/// Perpendicular half-extent of a glyph body in grid units.
pub(crate) fn body_half_extent(kind: &SymbolKind) -> f32 {
    match kind {
        SymbolKind::Resistor | SymbolKind::Inductor => 0.65,
        SymbolKind::Capacitor => 0.9,
        SymbolKind::Diode | SymbolKind::Led | SymbolKind::Zener | SymbolKind::Switch => 0.8,
        SymbolKind::OpAmp => 1.5,
        SymbolKind::Ground => 1.4,
        _ => 1.0,
    }
}

/// Half-height of a glyph in grid units, used to keep text close to its body.
fn text_half_height(symbol: &Symbol) -> f32 {
    match symbol.orientation {
        editor_core::Orientation::R90 | editor_core::Orientation::R270 => 2.0,
        _ => body_half_extent(&symbol.kind).max(0.95),
    }
}

fn egui_rect(center: Pos2, scale: f32, half_width: f32, half_height: f32) -> eframe::egui::Rect {
    eframe::egui::Rect::from_center_size(
        center,
        Vec2::new(scale * half_width * 2.0, scale * half_height * 2.0),
    )
}

fn paint_body(
    painter: &Painter,
    symbol: &Symbol,
    center: Pos2,
    scale: f32,
    stroke: Stroke,
    resistor: ResistorStyle,
) {
    match &symbol.kind {
        SymbolKind::Resistor => match resistor {
            ResistorStyle::European => {
                line_local(
                    painter,
                    symbol,
                    center,
                    scale,
                    &[(-2.0, 0.0), (-1.0, 0.0)],
                    stroke,
                );
                line_local(
                    painter,
                    symbol,
                    center,
                    scale,
                    &[
                        (-1.0, -0.62),
                        (1.0, -0.62),
                        (1.0, 0.62),
                        (-1.0, 0.62),
                        (-1.0, -0.62),
                    ],
                    stroke,
                );
                line_local(
                    painter,
                    symbol,
                    center,
                    scale,
                    &[(1.0, 0.0), (2.0, 0.0)],
                    stroke,
                );
            }
            ResistorStyle::American => {
                let points = [
                    (-2.0, 0.0),
                    (-1.35, 0.0),
                    (-1.05, -0.62),
                    (-0.55, 0.62),
                    (-0.05, -0.62),
                    (0.45, 0.62),
                    (0.95, -0.62),
                    (1.35, 0.0),
                    (2.0, 0.0),
                ];
                line_local(painter, symbol, center, scale, &points, stroke);
            }
        },
        SymbolKind::Capacitor => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-2.0, 0.0), (-0.38, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.38, 0.0), (2.0, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-0.38, -0.9), (-0.38, 0.9)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.38, -0.9), (0.38, 0.9)],
                stroke,
            );
        }
        SymbolKind::Inductor => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-2.0, 0.0), (-1.2, 0.0)],
                stroke,
            );
            let mut arcs = Vec::new();
            for index in 0..3 {
                let start = -1.2 + index as f32 * 0.8;
                for step in 0..=12 {
                    let t = step as f32 / 12.0;
                    let x = start + t * 0.8;
                    let y = -(PI * t).sin() * 0.58;
                    arcs.push((x, y));
                }
            }
            line_local(painter, symbol, center, scale, &arcs, stroke);
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(1.2, 0.0), (2.0, 0.0)],
                stroke,
            );
        }
        SymbolKind::VoltageSource => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-2.0, 0.0), (-1.0, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(1.0, 0.0), (2.0, 0.0)],
                stroke,
            );
            painter.circle_stroke(center, scale, stroke);
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-0.68, 0.0), (-0.22, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-0.45, -0.23), (-0.45, 0.23)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.22, 0.0), (0.68, 0.0)],
                stroke,
            );
        }
        SymbolKind::CurrentSource => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-2.0, 0.0), (-1.0, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(1.0, 0.0), (2.0, 0.0)],
                stroke,
            );
            painter.circle_stroke(center, scale, stroke);
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-0.42, 0.0), (0.45, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.45, 0.0), (0.12, -0.28)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.45, 0.0), (0.12, 0.28)],
                stroke,
            );
        }
        SymbolKind::Ground => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.0, 0.0), (0.0, 0.8)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-0.9, 0.8), (0.9, 0.8)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-0.55, 1.1), (0.55, 1.1)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-0.25, 1.4), (0.25, 1.4)],
                stroke,
            );
        }
        SymbolKind::Switch => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-2.0, 0.0), (-0.95, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.95, 0.0), (2.0, 0.0)],
                stroke,
            );
            let closed = matches!(symbol.params.get("closed"), Some(ParamValue::Bool(true)));
            if closed {
                line_local(
                    painter,
                    symbol,
                    center,
                    scale,
                    &[(-0.8, 0.0), (0.8, 0.0)],
                    stroke,
                );
            } else {
                line_local(
                    painter,
                    symbol,
                    center,
                    scale,
                    &[(-0.8, 0.0), (0.65, -0.8)],
                    stroke,
                );
            }
            painter.circle_filled(
                paint_position(symbol, center, scale, Vec2::new(-0.95, 0.0)),
                (scale * 0.13).clamp(1.5, 3.0),
                stroke.color,
            );
            painter.circle_filled(
                paint_position(symbol, center, scale, Vec2::new(0.95, 0.0)),
                (scale * 0.13).clamp(1.5, 3.0),
                stroke.color,
            );
        }
        SymbolKind::Diode => paint_diode(painter, symbol, center, scale, stroke, false),
        SymbolKind::Led => paint_diode(painter, symbol, center, scale, stroke, true),
        SymbolKind::Zener => paint_zener(painter, symbol, center, scale, stroke),
        SymbolKind::OpAmp => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-1.5, -1.5), (-1.5, 1.5), (1.5, 0.0), (-1.5, -1.5)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-2.0, -1.0), (-1.5, -1.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-2.0, 1.0), (-1.5, 1.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(1.5, 0.0), (2.0, 0.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.0, -2.0), (0.0, -0.75)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(0.0, 0.75), (0.0, 2.0)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-1.3, -0.62), (-0.8, -0.62)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-1.05, -0.87), (-1.05, -0.37)],
                stroke,
            );
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[(-1.3, 0.62), (-0.8, 0.62)],
                stroke,
            );
        }
        SymbolKind::Voltmeter | SymbolKind::Ammeter | SymbolKind::Ohmmeter => {
            painter.circle_stroke(center, scale * 0.9, stroke);
            let (letter, color) = match symbol.kind {
                SymbolKind::Voltmeter => ("V", Color32::from_rgb(255, 190, 92)),
                SymbolKind::Ammeter => ("A", Color32::from_rgb(120, 210, 220)),
                SymbolKind::Ohmmeter => ("Ω", Color32::from_rgb(112, 210, 155)),
                _ => ("", stroke.color),
            };
            painter.text(
                center,
                Align2::CENTER_CENTER,
                letter,
                FontId::proportional((scale * 1.05).clamp(9.0, 18.0)),
                color,
            );
        }
        SymbolKind::Custom(_) => {
            line_local(
                painter,
                symbol,
                center,
                scale,
                &[
                    (-1.4, -1.0),
                    (1.4, -1.0),
                    (1.4, 1.0),
                    (-1.4, 1.0),
                    (-1.4, -1.0),
                ],
                stroke,
            );
        }
    }
}

fn paint_diode(
    painter: &Painter,
    symbol: &Symbol,
    center: Pos2,
    scale: f32,
    stroke: Stroke,
    led: bool,
) {
    line_local(
        painter,
        symbol,
        center,
        scale,
        &[(-2.0, 0.0), (-0.55, 0.0)],
        stroke,
    );
    line_local(
        painter,
        symbol,
        center,
        scale,
        &[(0.75, 0.0), (2.0, 0.0)],
        stroke,
    );
    line_local(
        painter,
        symbol,
        center,
        scale,
        &[(-0.55, -0.8), (-0.55, 0.8), (0.6, 0.0), (-0.55, -0.8)],
        stroke,
    );
    line_local(
        painter,
        symbol,
        center,
        scale,
        &[(0.7, -0.8), (0.7, 0.8)],
        stroke,
    );
    if led {
        line_local(
            painter,
            symbol,
            center,
            scale,
            &[(0.65, -1.0), (1.15, -1.55)],
            stroke,
        );
        line_local(
            painter,
            symbol,
            center,
            scale,
            &[(1.15, -1.55), (0.95, -1.35)],
            stroke,
        );
        line_local(
            painter,
            symbol,
            center,
            scale,
            &[(1.15, -1.55), (1.0, -1.6)],
            stroke,
        );
        line_local(
            painter,
            symbol,
            center,
            scale,
            &[(1.0, -0.75), (1.5, -1.3)],
            stroke,
        );
        line_local(
            painter,
            symbol,
            center,
            scale,
            &[(1.5, -1.3), (1.3, -1.1)],
            stroke,
        );
        line_local(
            painter,
            symbol,
            center,
            scale,
            &[(1.5, -1.3), (1.35, -1.35)],
            stroke,
        );
    }
}

fn paint_zener(painter: &Painter, symbol: &Symbol, center: Pos2, scale: f32, stroke: Stroke) {
    paint_diode(painter, symbol, center, scale, stroke, false);
    line_local(
        painter,
        symbol,
        center,
        scale,
        &[(0.7, 0.8), (0.95, 0.8)],
        stroke,
    );
    line_local(
        painter,
        symbol,
        center,
        scale,
        &[(0.7, -0.8), (0.45, -0.8)],
        stroke,
    );
}

fn line_local(
    painter: &Painter,
    symbol: &Symbol,
    center: Pos2,
    scale: f32,
    points: &[(f32, f32)],
    stroke: Stroke,
) {
    if points.len() < 2 {
        return;
    }
    let points = points
        .iter()
        .map(|(x, y)| paint_position(symbol, center, scale, Vec2::new(*x, *y)))
        .collect();
    painter.line(points, stroke);
}

fn paint_position(symbol: &Symbol, center: Pos2, scale: f32, local: Vec2) -> Pos2 {
    let x = if symbol.mirrored { -local.x } else { local.x };
    let (x, y) = match symbol.orientation {
        editor_core::Orientation::R0 => (x, local.y),
        editor_core::Orientation::R90 => (-local.y, x),
        editor_core::Orientation::R180 => (-x, -local.y),
        editor_core::Orientation::R270 => (local.y, -x),
    };
    center + Vec2::new(x, y) * scale
}

fn symbol_value(symbol: &Symbol) -> Option<String> {
    if symbol.kind.is_ground() {
        return None;
    }
    let primary = match symbol.kind {
        SymbolKind::Resistor => Some("resistance"),
        SymbolKind::Capacitor => Some("capacitance"),
        SymbolKind::Inductor => Some("inductance"),
        SymbolKind::VoltageSource | SymbolKind::CurrentSource => Some("dc"),
        SymbolKind::Switch => Some("closed"),
        SymbolKind::Led => Some("vf"),
        SymbolKind::Diode => Some("is"),
        SymbolKind::Zener => Some("bv"),
        SymbolKind::OpAmp => Some("gain"),
        _ => None,
    }
    .and_then(|key| symbol.params.get_key_value(key));
    let (key, value) = primary.or_else(|| symbol.params.iter().next())?;
    match value {
        ParamValue::Number(number) => {
            Some(format_parameter(*number, parameter_unit(&symbol.kind, key)))
        }
        ParamValue::Bool(closed) => {
            if key == "closed" {
                Some(if *closed { "fermé" } else { "ouvert" }.to_owned())
            } else {
                Some(closed.to_string())
            }
        }
        ParamValue::Text(text) => Some(text.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::{GridPos, Orientation, SymbolId, SymbolRegistry};

    #[test]
    fn value_preview_uses_engineering_units() {
        let registry = SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::Resistor).unwrap();
        let symbol = Symbol::from_def(
            SymbolId(1),
            definition,
            GridPos::new(0, 0),
            "R1",
            Orientation::R0,
            false,
        );
        assert_eq!(symbol_value(&symbol).as_deref(), Some("1 kΩ"));
    }

    #[test]
    fn text_stays_close_to_the_symbol_body() {
        let registry = SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::Resistor).unwrap();
        let horizontal = Symbol::from_def(
            SymbolId(1),
            definition,
            GridPos::new(0, 0),
            "R1",
            Orientation::R0,
            false,
        );
        assert!((text_half_height(&horizontal) - 0.95).abs() < f32::EPSILON);
        let vertical = Symbol::from_def(
            SymbolId(2),
            definition,
            GridPos::new(0, 0),
            "R2",
            Orientation::R90,
            false,
        );
        assert_eq!(text_half_height(&vertical), 2.0);
    }

    #[test]
    fn switch_preview_uses_open_and_closed_words() {
        let registry = SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::Switch).unwrap();
        let mut symbol = Symbol::from_def(
            SymbolId(1),
            definition,
            GridPos::new(0, 0),
            "SW1",
            Orientation::R0,
            false,
        );
        assert_eq!(symbol_value(&symbol).as_deref(), Some("ouvert"));
        symbol
            .params
            .insert("closed".to_owned(), ParamValue::Bool(true));
        assert_eq!(symbol_value(&symbol).as_deref(), Some("fermé"));
    }
}
