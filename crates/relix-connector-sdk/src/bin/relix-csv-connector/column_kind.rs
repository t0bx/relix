/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnKind {
    Integer,
    Real,
    Text,
}

impl ColumnKind {
    /// Chooses the narrowest SQLite type that every non-empty value of the column parses
    /// as; empty fields become NULL and do not influence the choice.
    pub(crate) fn infer<'a>(values: impl Iterator<Item = &'a str>) -> Self {
        let mut kind = Self::Integer;
        for value in values.filter(|value| !value.is_empty()) {
            if kind == Self::Integer && value.parse::<i64>().is_err() {
                kind = Self::Real;
            }
            if kind == Self::Real && value.parse::<f64>().is_err() {
                return Self::Text;
            }
        }
        kind
    }

    pub(crate) fn sql_type(self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
        }
    }

    pub(crate) fn value(self, raw: &str) -> Value {
        if raw.is_empty() {
            return Value::Null;
        }
        match self {
            Self::Integer => raw
                .parse()
                .map_or_else(|_| Value::Text(raw.to_owned()), Value::Int),
            Self::Real => raw
                .parse()
                .map_or_else(|_| Value::Text(raw.to_owned()), Value::Float),
            Self::Text => Value::Text(raw.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_narrowest_type() {
        assert_eq!(
            ColumnKind::infer(["1", "", "-3"].into_iter()),
            ColumnKind::Integer
        );
        assert_eq!(
            ColumnKind::infer(["1", "2.5"].into_iter()),
            ColumnKind::Real
        );
        assert_eq!(ColumnKind::infer(["1", "x"].into_iter()), ColumnKind::Text);
        assert_eq!(ColumnKind::Integer.value(""), Value::Null);
        assert_eq!(ColumnKind::Real.value("2.5"), Value::Float(2.5));
    }
}
