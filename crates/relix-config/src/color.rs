/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ConfigError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

const NAMED_COLORS: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "gray",
    "darkgray",
    "lightred",
    "lightgreen",
    "lightyellow",
    "lightblue",
    "lightmagenta",
    "lightcyan",
    "white",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorSpec {
    Reset,
    Named(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    pub fn named_name(index: u8) -> &'static str {
        NAMED_COLORS
            .get(usize::from(index))
            .copied()
            .unwrap_or("reset")
    }
}

impl FromStr for ColorSpec {
    type Err = ConfigError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || ConfigError::InvalidColor(text.to_owned());
        let normalized = text
            .trim()
            .to_ascii_lowercase()
            .replace(['_', '-', ' '], "");
        if normalized == "reset" || normalized == "default" {
            return Ok(Self::Reset);
        }
        if let Some(hex) = normalized.strip_prefix('#') {
            if hex.len() != 6 {
                return Err(invalid());
            }
            let channel = |range: std::ops::Range<usize>| {
                u8::from_str_radix(&hex[range], 16).map_err(|_| invalid())
            };
            return Ok(Self::Rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?));
        }
        if let Ok(index) = normalized.parse::<u8>() {
            return Ok(Self::Indexed(index));
        }
        let alias = match normalized.as_str() {
            "grey" => "gray",
            "darkgrey" => "darkgray",
            other => other,
        };
        NAMED_COLORS
            .iter()
            .position(|name| *name == alias)
            .and_then(|index| u8::try_from(index).ok())
            .map(Self::Named)
            .ok_or_else(invalid)
    }
}

impl fmt::Display for ColorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reset => f.write_str("reset"),
            Self::Named(index) => f.write_str(Self::named_name(*index)),
            Self::Indexed(index) => write!(f, "{index}"),
            Self::Rgb(red, green, blue) => write!(f, "#{red:02x}{green:02x}{blue:02x}"),
        }
    }
}

impl Serialize for ColorSpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_color_forms() {
        assert_eq!(
            "#1e2030".parse::<ColorSpec>().unwrap(),
            ColorSpec::Rgb(0x1e, 0x20, 0x30)
        );
        assert_eq!(
            "Light-Blue".parse::<ColorSpec>().unwrap(),
            ColorSpec::Named(12)
        );
        assert_eq!("236".parse::<ColorSpec>().unwrap(), ColorSpec::Indexed(236));
        assert_eq!("reset".parse::<ColorSpec>().unwrap(), ColorSpec::Reset);
        assert!("#12345".parse::<ColorSpec>().is_err());
        assert!("chartreuse".parse::<ColorSpec>().is_err());
    }

    #[test]
    fn display_roundtrips() {
        for text in ["#a0b1c2", "darkgray", "17", "reset"] {
            assert_eq!(text.parse::<ColorSpec>().unwrap().to_string(), text);
        }
    }
}
