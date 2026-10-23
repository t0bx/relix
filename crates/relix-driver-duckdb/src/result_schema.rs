/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::type_names::ColumnType;
use duckdb::arrow::datatypes::{DataType, IntervalUnit, TimeUnit};
use duckdb::core::LogicalTypeId;
use relix_driver_api::DriverError;

/// Rejects result columns the DuckDB client library cannot decode faithfully: Arrow
/// carriers it would abort on instead of returning an error, and `TIME WITH TIME ZONE`,
/// whose offset the carrier drops. The user is asked to cast them to text instead.
pub(crate) fn ensure_decodable(
    name: &str,
    carrier: &DataType,
    column: &ColumnType,
) -> Result<(), DriverError> {
    if is_decodable(carrier) && !column.contains(LogicalTypeId::TimeTZ) {
        return Ok(());
    }
    Err(DriverError::unsupported(format!(
        "column \"{name}\" has type {} which the DuckDB client cannot decode; \
         cast it to VARCHAR in the query",
        column.name
    )))
}

fn is_decodable(carrier: &DataType) -> bool {
    match carrier {
        DataType::Utf8
        | DataType::LargeUtf8
        | DataType::Binary
        | DataType::LargeBinary
        | DataType::FixedSizeBinary(_)
        | DataType::Boolean
        | DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Float32
        | DataType::Float64
        | DataType::Decimal32(..)
        | DataType::Decimal64(..)
        | DataType::Decimal128(..)
        | DataType::Timestamp(..)
        | DataType::Date32
        | DataType::Time64(TimeUnit::Microsecond)
        | DataType::Interval(IntervalUnit::MonthDayNano) => true,
        DataType::List(field) | DataType::LargeList(field) | DataType::FixedSizeList(field, _) => {
            is_decodable(field.data_type())
        }
        DataType::Struct(fields) => fields.iter().all(|field| is_decodable(field.data_type())),
        DataType::Map(entries, _) => is_decodable(entries.data_type()),
        DataType::Union(fields, _) => fields
            .iter()
            .all(|(_, field)| is_decodable(field.data_type())),
        DataType::Dictionary(key, value) => {
            matches!(
                key.as_ref(),
                DataType::UInt8 | DataType::UInt16 | DataType::UInt32
            ) && matches!(value.as_ref(), DataType::Utf8)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::arrow::datatypes::Field;
    use std::sync::Arc;

    #[test]
    fn nested_carriers_are_checked_recursively() {
        let list = DataType::List(Arc::new(Field::new("item", DataType::Int32, true)));
        assert!(is_decodable(&list));
        let nanos = DataType::List(Arc::new(Field::new(
            "item",
            DataType::Time64(TimeUnit::Nanosecond),
            true,
        )));
        assert!(!is_decodable(&nanos));
        assert!(!is_decodable(&DataType::Float16));
    }

    #[test]
    fn time_zone_offsets_are_never_dropped_silently() {
        let column = ColumnType {
            id: LogicalTypeId::List,
            name: "TIME WITH TIME ZONE[]".into(),
            children: vec![ColumnType {
                id: LogicalTypeId::TimeTZ,
                name: "TIME WITH TIME ZONE".into(),
                children: Vec::new(),
            }],
        };
        let carrier = DataType::List(Arc::new(Field::new(
            "item",
            DataType::Time64(TimeUnit::Microsecond),
            true,
        )));
        let error = ensure_decodable("t", &carrier, &column).unwrap_err();
        assert_eq!(error.kind, relix_driver_api::ErrorKind::Unsupported);
        assert!(error.message.contains("TIME WITH TIME ZONE[]"));
    }
}
