//! Typed SI quantities and engineering-notation parsing.

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Physical unit attached to a typed quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Unit {
    Volt,
    Ampere,
    Ohm,
    Farad,
    Henry,
    Second,
    Hertz,
    Watt,
    Coulomb,
    Celsius,
}

impl Unit {
    /// Canonical display symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Volt => "V",
            Self::Ampere => "A",
            Self::Ohm => "Ω",
            Self::Farad => "F",
            Self::Henry => "H",
            Self::Second => "s",
            Self::Hertz => "Hz",
            Self::Watt => "W",
            Self::Coulomb => "C",
            Self::Celsius => "°C",
        }
    }

    const fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Volt => &["V"],
            Self::Ampere => &["A"],
            Self::Ohm => &["Ω", "ohm", "Ohm"],
            Self::Farad => &["F"],
            Self::Henry => &["H"],
            Self::Second => &["s"],
            Self::Hertz => &["Hz"],
            Self::Watt => &["W"],
            Self::Coulomb => &["C"],
            Self::Celsius => &["°C"],
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.symbol())
    }
}

const ALL_UNITS: [Unit; 10] = [
    Unit::Volt,
    Unit::Ampere,
    Unit::Ohm,
    Unit::Farad,
    Unit::Henry,
    Unit::Second,
    Unit::Hertz,
    Unit::Watt,
    Unit::Coulomb,
    Unit::Celsius,
];

/// Error returned when parsing an engineering-notation value.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum UnitParseError {
    #[error("value is empty")]
    Empty,
    #[error("invalid number: {0}")]
    InvalidNumber(String),
    #[error("expected unit {expected:?}, found {found:?}")]
    MismatchedUnit { expected: Unit, found: Unit },
    #[error("value is not finite")]
    NotFinite,
    #[error("value overflows the representable range")]
    Overflow,
}

/// Trait implemented by every typed SI quantity.
pub trait Si: Copy {
    /// Unit represented by this quantity.
    const UNIT: Unit;

    /// Builds the quantity from a value expressed in SI base units.
    fn from_si(value: f64) -> Self;

    /// Returns the value expressed in SI base units.
    fn to_si(self) -> f64;

    /// Formats the quantity using engineering notation.
    fn format(self) -> String {
        format_si(self.to_si(), Self::UNIT)
    }

    /// Parses a quantity and checks its unit suffix.
    fn parse(input: &str) -> Result<Self, UnitParseError>
    where
        Self: Sized,
    {
        parse_quantity::<Self>(input)
    }
}

macro_rules! si_unit {
    ($name:ident, $unit:expr) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(f64);

        impl $name {
            /// Builds the quantity from an SI value.
            pub const fn new(value: f64) -> Self {
                Self(value)
            }

            /// Returns the SI value.
            pub const fn value(self) -> f64 {
                self.0
            }
        }

        impl Si for $name {
            const UNIT: Unit = $unit;

            fn from_si(value: f64) -> Self {
                Self(value)
            }

            fn to_si(self) -> f64 {
                self.0
            }
        }

        impl Add for $name {
            type Output = Self;

            fn add(self, rhs: Self) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }

        impl Sub for $name {
            type Output = Self;

            fn sub(self, rhs: Self) -> Self::Output {
                Self(self.0 - rhs.0)
            }
        }

        impl Mul<f64> for $name {
            type Output = Self;

            fn mul(self, rhs: f64) -> Self::Output {
                Self(self.0 * rhs)
            }
        }

        impl Div<f64> for $name {
            type Output = Self;

            fn div(self, rhs: f64) -> Self::Output {
                Self(self.0 / rhs)
            }
        }

        impl Neg for $name {
            type Output = Self;

            fn neg(self) -> Self::Output {
                Self(-self.0)
            }
        }

        impl From<$name> for f64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&format_si(self.0, $unit))
            }
        }
    };
}

si_unit!(Volts, Unit::Volt);
si_unit!(Amps, Unit::Ampere);
si_unit!(Ohms, Unit::Ohm);
si_unit!(Farads, Unit::Farad);
si_unit!(Henrys, Unit::Henry);
si_unit!(Seconds, Unit::Second);
si_unit!(Hertz, Unit::Hertz);
si_unit!(Watts, Unit::Watt);
si_unit!(Coulombs, Unit::Coulomb);
si_unit!(Celsius, Unit::Celsius);

impl Celsius {
    /// Converts a Kelvin temperature to Celsius.
    pub fn from_kelvin(value: f64) -> Self {
        Self(value - 273.15)
    }

    /// Converts this temperature to Kelvin.
    pub fn to_kelvin(self) -> f64 {
        self.0 + 273.15
    }
}

/// Parses a value and ignores any recognized unit suffix.
pub fn parse_si(input: &str) -> Result<f64, UnitParseError> {
    let (number, _unit) = split_unit_suffix(input, None);
    parse_si_number(number)
}

/// Parses a typed quantity, rejecting a mismatched unit suffix.
pub fn parse_quantity<Q: Si>(input: &str) -> Result<Q, UnitParseError> {
    let (number, suffix) = split_unit_suffix(input, Some(Q::UNIT));
    if let Some(found) = suffix {
        if found != Q::UNIT {
            return Err(UnitParseError::MismatchedUnit {
                expected: Q::UNIT,
                found,
            });
        }
    }
    Ok(Q::from_si(parse_si_number(number)?))
}

/// Formats a value with four significant digits.
pub fn format_si(value: f64, unit: Unit) -> String {
    format_si_precision(value, unit, 4)
}

/// Formats a value with the requested number of significant digits.
pub fn format_si_precision(value: f64, unit: Unit, significant_digits: usize) -> String {
    let digits = significant_digits.clamp(1, 12);
    if value.is_nan() {
        return format!("NaN {}", unit.symbol());
    }
    if value.is_infinite() {
        let sign = if value.is_sign_negative() { "-" } else { "" };
        return format!("{sign}inf {}", unit.symbol());
    }
    if value == 0.0 {
        return format!("0 {}", unit.symbol());
    }
    if unit == Unit::Celsius {
        let number = format_significant(value, digits);
        return format!("{number} {}", unit.symbol());
    }

    let mut exponent = ((value.abs().log10() / 3.0).floor() as i32) * 3;
    exponent = exponent.clamp(-12, 12);

    loop {
        let scaled = value / 10_f64.powi(exponent);
        let number = format_significant(scaled, digits);
        if exponent < 12 && number.trim_start_matches('-') == "1000" {
            exponent += 3;
            continue;
        }
        let prefix = prefix_for_exponent(exponent);
        return if prefix.is_empty() {
            format!("{number} {}", unit.symbol())
        } else {
            format!("{number} {prefix}{}", unit.symbol())
        };
    }
}

fn format_significant(value: f64, digits: usize) -> String {
    let magnitude = value.abs();
    let order = if magnitude == 0.0 {
        0
    } else {
        magnitude.log10().floor() as i32
    };
    let decimals = (digits as i32 - 1 - order).max(0) as usize;
    trim_number(format!("{value:.decimals$}"))
}

fn trim_number(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        "0".to_owned()
    } else {
        text
    }
}

const fn prefix_for_exponent(exponent: i32) -> &'static str {
    match exponent {
        -12 => "p",
        -9 => "n",
        -6 => "µ",
        -3 => "m",
        3 => "k",
        6 => "M",
        9 => "G",
        12 => "T",
        _ => "",
    }
}

fn split_unit_suffix(input: &str, expected: Option<Unit>) -> (&str, Option<Unit>) {
    let trimmed = input.trim_end();
    let mut candidates = Vec::new();
    if let Some(unit) = expected {
        for alias in unit.aliases() {
            candidates.push((*alias, unit));
        }
        if unit == Unit::Celsius {
            candidates.push(("C", Unit::Celsius));
        }
    }
    for unit in ALL_UNITS {
        for alias in unit.aliases() {
            candidates.push((*alias, unit));
        }
    }
    candidates.sort_by_key(|(alias, _)| std::cmp::Reverse(alias.chars().count()));

    for (alias, unit) in candidates {
        if let Some(stem) = trimmed.strip_suffix(alias) {
            if ends_with_numberish(stem) {
                return (stem.trim_end(), Some(unit));
            }
        }
    }
    (trimmed, None)
}

fn ends_with_numberish(input: &str) -> bool {
    match input.trim_end().chars().next_back() {
        Some(character) => {
            character.is_ascii_digit() || character == '.' || prefix_factor(character).is_some()
        }
        None => false,
    }
}

fn prefix_factor(character: char) -> Option<f64> {
    match character {
        'p' => Some(1e-12),
        'n' => Some(1e-9),
        'u' | 'µ' => Some(1e-6),
        'm' => Some(1e-3),
        'k' | 'K' => Some(1e3),
        'M' => Some(1e6),
        'G' => Some(1e9),
        'T' => Some(1e12),
        _ => None,
    }
}

fn parse_si_number(input: &str) -> Result<f64, UnitParseError> {
    let original = input.trim();
    if original.is_empty() {
        return Err(UnitParseError::Empty);
    }
    let compact: String = original.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err(UnitParseError::Empty);
    }

    if let Ok(value) = compact.parse::<f64>() {
        return finite_value(value);
    }
    if compact.contains(['e', 'E']) {
        return Err(UnitParseError::InvalidNumber(original.to_owned()));
    }

    let mut prefix = None;
    for (index, character) in compact.char_indices() {
        if prefix_factor(character).is_some() {
            if prefix.is_some() {
                return Err(UnitParseError::InvalidNumber(original.to_owned()));
            }
            prefix = Some((index, character));
        }
    }
    let Some((index, character)) = prefix else {
        return Err(UnitParseError::InvalidNumber(original.to_owned()));
    };
    let factor = prefix_factor(character).unwrap_or(1.0);
    let left = &compact[..index];
    let right = &compact[index + character.len_utf8()..];

    if left.is_empty() {
        return Err(UnitParseError::InvalidNumber(original.to_owned()));
    }

    let base = if right.is_empty() {
        left.parse::<f64>()
            .map_err(|_| UnitParseError::InvalidNumber(original.to_owned()))?
    } else {
        let has_digit = left.chars().any(|c| c.is_ascii_digit());
        let left_is_integer = !left.contains('.');
        let right_is_digits = right.chars().all(|c| c.is_ascii_digit());
        if !has_digit || !left_is_integer || !right_is_digits {
            return Err(UnitParseError::InvalidNumber(original.to_owned()));
        }
        let combined = format!("{left}.{right}");
        combined
            .parse::<f64>()
            .map_err(|_| UnitParseError::InvalidNumber(original.to_owned()))?
    };

    finite_value(base * factor)
}

fn finite_value(value: f64) -> Result<f64, UnitParseError> {
    if value.is_nan() {
        Err(UnitParseError::NotFinite)
    } else if value.is_infinite() {
        Err(UnitParseError::Overflow)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(left: f64, right: f64) {
        assert!(
            (left - right).abs() <= right.abs().max(1.0) * 1e-12,
            "{left} != {right}"
        );
    }

    #[test]
    fn parses_plain_and_prefixed_values() {
        close(parse_si("4700").unwrap(), 4700.0);
        close(parse_si("4.7k").unwrap(), 4700.0);
        close(parse_si("4k7").unwrap(), 4700.0);
        close(parse_si("1m").unwrap(), 0.001);
        close(parse_si("2u2").unwrap(), 2.2e-6);
        close(parse_si("3M3").unwrap(), 3.3e6);
        close(parse_si("-4k7").unwrap(), -4700.0);
        close(parse_si("1e-3").unwrap(), 0.001);
        close(parse_si("1E3").unwrap(), 1000.0);
    }

    #[test]
    fn parses_and_checks_units() {
        close(parse_quantity::<Ohms>("4.7kΩ").unwrap().to_si(), 4700.0);
        close(parse_quantity::<Volts>("4.7kV").unwrap().to_si(), 4700.0);
        close(parse_quantity::<Farads>("2.2uF").unwrap().to_si(), 2.2e-6);
        close(parse_quantity::<Hertz>("1MHz").unwrap().to_si(), 1e6);
        close(parse_quantity::<Celsius>("25°C").unwrap().to_si(), 25.0);
        close(parse_quantity::<Celsius>("25C").unwrap().to_si(), 25.0);
        close(parse_quantity::<Ohms>("1kohm").unwrap().to_si(), 1000.0);
        assert_eq!(
            parse_quantity::<Volts>("1A").unwrap_err(),
            UnitParseError::MismatchedUnit {
                expected: Unit::Volt,
                found: Unit::Ampere
            }
        );
    }

    #[test]
    fn rejects_invalid_values() {
        assert_eq!(parse_si(""), Err(UnitParseError::Empty));
        assert_eq!(
            parse_si("1.2.3"),
            Err(UnitParseError::InvalidNumber("1.2.3".into()))
        );
        assert_eq!(
            parse_si("4k7.5"),
            Err(UnitParseError::InvalidNumber("4k7.5".into()))
        );
        assert_eq!(parse_si("NaN"), Err(UnitParseError::NotFinite));
        assert_eq!(parse_si("1e400"), Err(UnitParseError::Overflow));
    }

    #[test]
    fn formats_engineering_notation() {
        assert_eq!(format_si(0.0, Unit::Volt), "0 V");
        assert_eq!(format_si(4_700.0, Unit::Ohm), "4.7 kΩ");
        assert_eq!(format_si(0.001, Unit::Ampere), "1 mA");
        assert_eq!(format_si(0.000_001, Unit::Farad), "1 µF");
        assert_eq!(format_si(1e9, Unit::Ohm), "1 GΩ");
        assert_eq!(format_si(-0.001, Unit::Ampere), "-1 mA");
        assert_eq!(format_si(25.0, Unit::Celsius), "25 °C");
        assert_eq!(format_si(999.95, Unit::Volt), "1 kV");
        assert_eq!(format_si(0.5, Unit::Volt), "500 mV");
    }

    #[test]
    fn display_uses_si_formatting() {
        assert_eq!(Volts::new(4_700.0).to_string(), "4.7 kV");
        assert_eq!(Farads::new(0.000_001).to_string(), "1 µF");
    }

    #[test]
    fn converts_celsius_and_kelvin() {
        close(Celsius::from_kelvin(300.0).to_si(), 26.85);
        close(Celsius::new(100.0).to_kelvin(), 373.15);
    }
}
