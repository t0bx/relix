/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Value;
use std::borrow::Cow;
use tiberius::{ColumnData, ToSql};

/// A statement parameter. NULL is sent as an untyped NVARCHAR NULL and unsigned values
/// beyond `bigint` as text, both of which SQL Server converts implicitly to the column type.
pub struct Param(ColumnData<'static>);

impl Param {
    pub fn from_value(value: &Value) -> Self {
        Self(match value {
            Value::Null => ColumnData::String(None),
            Value::Bool(flag) => ColumnData::Bit(Some(*flag)),
            Value::Int(number) => ColumnData::I64(Some(*number)),
            Value::UInt(number) => match i64::try_from(*number) {
                Ok(number) => ColumnData::I64(Some(number)),
                Err(_) => ColumnData::String(Some(Cow::Owned(number.to_string()))),
            },
            Value::Float(number) => ColumnData::F64(Some(*number)),
            Value::Text(text) => ColumnData::String(Some(Cow::Owned(text.clone()))),
            Value::Bytes(bytes) => ColumnData::Binary(Some(Cow::Owned(bytes.clone()))),
        })
    }
}

impl ToSql for Param {
    fn to_sql(&self) -> ColumnData<'_> {
        self.0.clone()
    }
}

pub fn params_of(values: &[Value]) -> Vec<Param> {
    values.iter().map(Param::from_value).collect()
}

pub fn as_sql_params(params: &[Param]) -> Vec<&dyn ToSql> {
    params.iter().map(|param| param as &dyn ToSql).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_become_typed_parameters() {
        assert_eq!(
            Param::from_value(&Value::Int(4)).to_sql(),
            ColumnData::I64(Some(4))
        );
        assert_eq!(
            Param::from_value(&Value::UInt(u64::MAX)).to_sql(),
            ColumnData::String(Some(Cow::Borrowed("18446744073709551615")))
        );
        assert_eq!(
            Param::from_value(&Value::Null).to_sql(),
            ColumnData::String(None)
        );
        assert_eq!(
            params_of(&[Value::Bool(true), Value::Bytes(vec![1])]).len(),
            2
        );
    }
}
