/*
 * @author t0bx
 * @since 23.10.2026
 */

use duckdb::arrow::datatypes::DataType;
use duckdb::core::{LogicalTypeHandle, LogicalTypeId};

/// The DuckDB type of a result column together with its nested member types, so
/// values inside lists, structs and maps can be rendered with their real type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnType {
    pub(crate) id: LogicalTypeId,
    pub(crate) name: String,
    pub(crate) children: Vec<ColumnType>,
}

impl ColumnType {
    /// Reads the logical type and uses the Arrow carrier only for what the logical type
    /// handle does not expose (the size of fixed-length `ARRAY` columns).
    pub(crate) fn of(handle: &LogicalTypeHandle, carrier: Option<&DataType>) -> Self {
        let id = handle.id();
        let carriers = child_carriers(carrier);
        let children: Vec<ColumnType> = (0..handle.num_children())
            .map(|index| ColumnType::of(&handle.child(index), carriers.get(index).copied()))
            .collect();
        let name = handle
            .get_alias()
            .unwrap_or_else(|| type_name(handle, id, &children, carrier));
        Self { id, name, children }
    }

    pub(crate) fn child(&self, index: usize) -> Option<&ColumnType> {
        self.children.get(index)
    }

    pub(crate) fn contains(&self, id: LogicalTypeId) -> bool {
        self.id == id || self.children.iter().any(|child| child.contains(id))
    }

    pub(crate) fn has_time_zone(&self) -> bool {
        self.id == LogicalTypeId::TimestampTZ
    }
}

fn child_carriers(carrier: Option<&DataType>) -> Vec<&DataType> {
    match carrier {
        Some(DataType::List(field) | DataType::LargeList(field)) => vec![field.data_type()],
        Some(DataType::FixedSizeList(field, _)) => vec![field.data_type()],
        Some(DataType::Struct(fields)) => fields.iter().map(|field| field.data_type()).collect(),
        Some(DataType::Map(entries, _)) => match entries.data_type() {
            DataType::Struct(fields) => fields.iter().map(|field| field.data_type()).collect(),
            _ => Vec::new(),
        },
        Some(DataType::Union(fields, _)) => {
            fields.iter().map(|(_, field)| field.data_type()).collect()
        }
        _ => Vec::new(),
    }
}

fn type_name(
    handle: &LogicalTypeHandle,
    id: LogicalTypeId,
    children: &[ColumnType],
    carrier: Option<&DataType>,
) -> String {
    let child_name = |index: usize| {
        children
            .get(index)
            .map_or_else(|| "ANY".to_owned(), |child| child.name.clone())
    };
    let member_list = || {
        children
            .iter()
            .enumerate()
            .map(|(index, child)| format!("{} {}", quoted_member(handle, index), child.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match id {
        LogicalTypeId::Decimal => format!(
            "DECIMAL({},{})",
            handle.decimal_width(),
            handle.decimal_scale()
        ),
        LogicalTypeId::List => format!("{}[]", child_name(0)),
        LogicalTypeId::Array => match carrier {
            Some(DataType::FixedSizeList(_, size)) => format!("{}[{size}]", child_name(0)),
            _ => format!("{}[]", child_name(0)),
        },
        LogicalTypeId::Map => format!("MAP({}, {})", child_name(0), child_name(1)),
        LogicalTypeId::Struct => format!("STRUCT({})", member_list()),
        LogicalTypeId::Union => format!("UNION({})", member_list()),
        other => scalar_name(other).to_owned(),
    }
}

fn quoted_member(handle: &LogicalTypeHandle, index: usize) -> String {
    let name = handle.child_name(index);
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && !name.starts_with(|ch: char| ch.is_ascii_digit());
    if plain {
        name
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

fn scalar_name(id: LogicalTypeId) -> &'static str {
    match id {
        LogicalTypeId::Boolean => "BOOLEAN",
        LogicalTypeId::Tinyint => "TINYINT",
        LogicalTypeId::Smallint => "SMALLINT",
        LogicalTypeId::Integer => "INTEGER",
        LogicalTypeId::Bigint => "BIGINT",
        LogicalTypeId::Hugeint => "HUGEINT",
        LogicalTypeId::UTinyint => "UTINYINT",
        LogicalTypeId::USmallint => "USMALLINT",
        LogicalTypeId::UInteger => "UINTEGER",
        LogicalTypeId::UBigint => "UBIGINT",
        LogicalTypeId::UHugeint => "UHUGEINT",
        LogicalTypeId::Float => "FLOAT",
        LogicalTypeId::Double => "DOUBLE",
        LogicalTypeId::Varchar | LogicalTypeId::StringLiteral => "VARCHAR",
        LogicalTypeId::Blob => "BLOB",
        LogicalTypeId::Date => "DATE",
        LogicalTypeId::Time => "TIME",
        LogicalTypeId::TimeNs => "TIME_NS",
        LogicalTypeId::TimeTZ => "TIME WITH TIME ZONE",
        LogicalTypeId::Timestamp => "TIMESTAMP",
        LogicalTypeId::TimestampS => "TIMESTAMP_S",
        LogicalTypeId::TimestampMs => "TIMESTAMP_MS",
        LogicalTypeId::TimestampNs => "TIMESTAMP_NS",
        LogicalTypeId::TimestampTZ => "TIMESTAMP WITH TIME ZONE",
        LogicalTypeId::Interval => "INTERVAL",
        LogicalTypeId::Uuid => "UUID",
        LogicalTypeId::Enum => "ENUM",
        LogicalTypeId::Bit => "BIT",
        LogicalTypeId::Bignum => "BIGNUM",
        LogicalTypeId::Geometry => "GEOMETRY",
        LogicalTypeId::Variant => "VARIANT",
        LogicalTypeId::SqlNull => "NULL",
        LogicalTypeId::IntegerLiteral => "INTEGER_LITERAL",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column_type(sql: &str) -> ColumnType {
        let connection = duckdb::Connection::open_in_memory().unwrap();
        let mut statement = connection.prepare(sql).unwrap();
        let _ = statement.query([]).unwrap();
        ColumnType::of(
            &statement.column_logical_type(0),
            Some(&statement.column_type(0)),
        )
    }

    #[test]
    fn nested_types_are_named_like_duckdb() {
        assert_eq!(
            column_type("SELECT 1.50::DECIMAL(9,2)").name,
            "DECIMAL(9,2)"
        );
        assert_eq!(column_type("SELECT [1, 2]").name, "INTEGER[]");
        assert_eq!(column_type("SELECT [1, 2]::INTEGER[2]").name, "INTEGER[2]");
        assert_eq!(
            column_type("SELECT {'a': 1, 'B c': 'x'}").name,
            "STRUCT(a INTEGER, \"B c\" VARCHAR)"
        );
        assert_eq!(
            column_type("SELECT MAP {'k': [1.5]}").name,
            "MAP(VARCHAR, DECIMAL(2,1)[])"
        );
        assert_eq!(
            column_type("SELECT now()::TIMESTAMPTZ").name,
            "TIMESTAMP WITH TIME ZONE"
        );
    }
}
