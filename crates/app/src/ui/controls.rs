//! Inline parameter sliders anchored to components.

use editor_core::SymbolId;

/// One slider bound to a numeric component parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterControl {
    /// Target symbol.
    pub symbol: SymbolId,
    /// Parameter key.
    pub key: String,
    /// Last committed value.
    pub value: f64,
    /// Preview while dragging.
    pub draft: f64,
    /// Whether the pointer currently drags the slider.
    pub dragging: bool,
}

impl ParameterControl {
    /// Creates a control for a numeric parameter.
    pub fn new(symbol: SymbolId, key: String, value: f64) -> Self {
        Self {
            symbol,
            key,
            value,
            draft: value,
            dragging: false,
        }
    }

    /// Returns the editable range around the current value.
    pub fn range(&self) -> (f64, f64) {
        control_range(self.value)
    }
}

/// Collection of inline parameter controls.
#[derive(Clone, Debug, Default)]
pub struct ParameterControls {
    items: Vec<ParameterControl>,
}

impl ParameterControls {
    /// Adds a control or updates its current value when already present.
    pub fn add(&mut self, symbol: SymbolId, key: String, value: f64) {
        if let Some(control) = self
            .items
            .iter_mut()
            .find(|control| control.symbol == symbol && control.key == key)
        {
            control.value = value;
            control.draft = value;
            return;
        }
        self.items.push(ParameterControl::new(symbol, key, value));
    }

    /// Removes a control.
    pub fn remove(&mut self, symbol: SymbolId, key: &str) {
        self.items
            .retain(|control| !(control.symbol == symbol && control.key == key));
    }

    /// Returns every control.
    pub fn items(&self) -> &[ParameterControl] {
        &self.items
    }

    /// Returns every control mutably.
    pub fn items_mut(&mut self) -> &mut [ParameterControl] {
        &mut self.items
    }

    /// Returns true when no control is defined.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Returns a decade range around a finite value.
pub fn control_range(value: f64) -> (f64, f64) {
    if value > 0.0 && value.is_finite() {
        (value / 10.0, value * 10.0)
    } else if value < 0.0 && value.is_finite() {
        (value * 10.0, value / 10.0)
    } else {
        (-1.0, 1.0)
    }
}

/// Maps a value to `[0, 1]` inside a range.
pub fn control_fraction(value: f64, range: (f64, f64)) -> f64 {
    let (low, high) = range;
    if (high - low).abs() <= f64::MIN_POSITIVE {
        return 0.5;
    }
    if low > 0.0 && high > 0.0 && value > 0.0 {
        let ratio = (high / low).ln();
        if ratio.abs() > f64::MIN_POSITIVE {
            return ((value / low).ln() / ratio).clamp(0.0, 1.0);
        }
    }
    if low < 0.0 && high < 0.0 && value < 0.0 {
        let ratio = (low.abs() / high.abs()).ln();
        if ratio.abs() > f64::MIN_POSITIVE {
            return ((low.abs() / value.abs()).ln() / ratio).clamp(0.0, 1.0);
        }
    }
    ((value - low) / (high - low)).clamp(0.0, 1.0)
}

/// Maps a normalized position to a value inside a range.
pub fn control_value(fraction: f64, range: (f64, f64)) -> f64 {
    let (low, high) = range;
    let fraction = fraction.clamp(0.0, 1.0);
    if low > 0.0 && high > 0.0 {
        low * (high / low).powf(fraction)
    } else if low < 0.0 && high < 0.0 {
        -(low.abs() * (high.abs() / low.abs()).powf(fraction))
    } else {
        low + (high - low) * fraction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_ordered_around_the_value() {
        let positive = control_range(1_000.0);
        assert!(positive.0 < positive.1);
        assert_eq!(control_fraction(1_000.0, positive), 0.5);
        let negative = control_range(-10.0);
        assert!(negative.0 < negative.1);
        assert_eq!(control_fraction(-10.0, negative), 0.5);
        let zero = control_range(0.0);
        assert_eq!(zero, (-1.0, 1.0));
    }

    #[test]
    fn values_round_trip_through_fractions() {
        for (value, range) in [
            (1_000.0, control_range(1_000.0)),
            (-10.0, control_range(-10.0)),
            (0.0, (-1.0, 1.0)),
        ] {
            let fraction = control_fraction(value, range);
            let rebuilt = control_value(fraction, range);
            assert!((rebuilt - value).abs() < 1e-9);
        }
    }

    #[test]
    fn controls_are_unique_per_parameter() {
        let mut controls = ParameterControls::default();
        controls.add(SymbolId(1), "resistance".to_owned(), 1_000.0);
        controls.add(SymbolId(1), "resistance".to_owned(), 2_000.0);
        assert_eq!(controls.items().len(), 1);
        assert_eq!(controls.items()[0].value, 2_000.0);
        controls.remove(SymbolId(1), "resistance");
        assert!(controls.is_empty());
    }
}
