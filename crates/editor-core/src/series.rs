//! Preferred-number series used by direct parameter editing.

/// Normalized E12 preferred values.
pub const E12: [f64; 12] = [1.0, 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2];

/// Normalized E24 preferred values.
pub const E24: [f64; 24] = [
    1.0, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2.0, 2.2, 2.4, 2.7, 3.0, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1, 5.6,
    6.2, 6.8, 7.5, 8.2, 9.1,
];

/// Normalized E96 preferred values.
pub const E96: [f64; 96] = [
    1.00, 1.02, 1.05, 1.07, 1.10, 1.13, 1.15, 1.18, 1.21, 1.24, 1.27, 1.30, 1.33, 1.37, 1.40, 1.43,
    1.47, 1.50, 1.54, 1.58, 1.62, 1.65, 1.69, 1.74, 1.78, 1.82, 1.87, 1.91, 1.96, 2.00, 2.05, 2.10,
    2.15, 2.21, 2.26, 2.32, 2.37, 2.43, 2.49, 2.55, 2.61, 2.67, 2.74, 2.80, 2.87, 2.94, 3.01, 3.09,
    3.16, 3.24, 3.32, 3.40, 3.48, 3.57, 3.65, 3.74, 3.83, 3.92, 4.02, 4.12, 4.22, 4.32, 4.42, 4.53,
    4.64, 4.75, 4.87, 4.99, 5.11, 5.23, 5.36, 5.49, 5.62, 5.76, 5.90, 6.04, 6.19, 6.34, 6.49, 6.65,
    6.81, 6.98, 7.15, 7.32, 7.50, 7.68, 7.87, 8.06, 8.25, 8.45, 8.66, 8.87, 9.09, 9.31, 9.53, 9.76,
];

/// Preferred-number series offered by the inspector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Series {
    /// E12, 10 % tolerance.
    E12,
    /// E24, 5 % tolerance.
    #[default]
    E24,
    /// E96, 1 % tolerance.
    E96,
}

impl Series {
    /// Every series in increasing precision.
    pub const ALL: [Self; 3] = [Self::E12, Self::E24, Self::E96];

    /// Short display name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::E12 => "E12",
            Self::E24 => "E24",
            Self::E96 => "E96",
        }
    }

    /// Normalized preferred values between one and ten.
    pub const fn values(self) -> &'static [f64] {
        match self {
            Self::E12 => &E12,
            Self::E24 => &E24,
            Self::E96 => &E96,
        }
    }

    /// Returns the nearest preferred value, preserving sign and decade.
    pub fn nearest(self, value: f64) -> Option<f64> {
        let sign = value.signum();
        let magnitude = value.abs();
        let (exponent, index) = self.locate(magnitude)?;
        preferred(sign, exponent, index, self.values())
    }

    /// Moves by whole preferred-value steps across decade boundaries.
    pub fn cycle(self, value: f64, steps: i32) -> Option<f64> {
        if steps == 0 {
            return self.nearest(value);
        }
        let sign = value.signum();
        let (exponent, index) = self.locate(value.abs())?;
        let count = i32::try_from(self.values().len()).ok()?;
        let shifted = i32::try_from(index).ok()?.checked_add(steps)?;
        let next_exponent = exponent.checked_add(shifted.div_euclid(count))?;
        let next_index = usize::try_from(shifted.rem_euclid(count)).ok()?;
        preferred(sign, next_exponent, next_index, self.values())
    }

    fn locate(self, magnitude: f64) -> Option<(i32, usize)> {
        if !magnitude.is_finite() || magnitude <= 0.0 {
            return None;
        }
        let base = magnitude.log10().floor() as i32;
        let mut best: Option<(f64, i32, usize)> = None;
        for exponent in (base - 1)..=(base + 1) {
            for index in 0..self.values().len() {
                let Some(candidate) = preferred(1.0, exponent, index, self.values()) else {
                    continue;
                };
                let distance = (candidate - magnitude).abs();
                if best.is_none_or(|(best_distance, _, _)| distance < best_distance) {
                    best = Some((distance, exponent, index));
                }
            }
        }
        best.map(|(_, exponent, index)| (exponent, index))
    }
}

fn preferred(sign: f64, exponent: i32, index: usize, values: &[f64]) -> Option<f64> {
    let nominal = *values.get(index)?;
    let scale = 10_f64.powi(exponent);
    let value = sign * nominal * scale;
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= expected.abs().max(f64::MIN_POSITIVE) * 1e-9,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn nearest_value_preserves_decade_and_sign() {
        close(Series::E12.nearest(4_700.0).unwrap(), 4_700.0);
        close(Series::E24.nearest(-1_050.0).unwrap(), -1_000.0);
        close(Series::E96.nearest(100e-9).unwrap(), 100e-9);
        assert_eq!(Series::E12.nearest(0.0), None);
    }

    #[test]
    fn cycling_crosses_decade_boundaries() {
        close(Series::E12.cycle(4_700.0, 1).unwrap(), 5_600.0);
        close(Series::E12.cycle(4_700.0, -1).unwrap(), 3_900.0);
        close(Series::E12.cycle(8_200.0, 1).unwrap(), 10_000.0);
        close(Series::E12.cycle(1_000.0, -1).unwrap(), 820.0);
        close(Series::E96.cycle(100e-9, 1).unwrap(), 102e-9);
        close(Series::E24.cycle(-4_700.0, 1).unwrap(), -5_100.0);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn cycling_up_then_down_returns_the_same_preferred_value(
            magnitude in 1e-9_f64..1e9,
            steps in 1_i32..20,
        ) {
            for series in Series::ALL {
                let start = series.nearest(magnitude).unwrap();
                let up = series.cycle(start, steps).unwrap();
                let back = series.cycle(up, -steps).unwrap();
                prop_assert!(
                    (back - start).abs() <= start.abs().max(f64::MIN_POSITIVE) * 1e-9,
                    "{series:?}: {start} -> {up} -> {back}"
                );
            }
        }
    }
}
