/*
 * @author t0bx
 * @since 23.10.2026
 */

use scylla::frame::response::result::{CollectionType, ColumnType, NativeType};

pub(crate) fn type_name(column_type: &ColumnType<'_>) -> String {
    match column_type {
        ColumnType::Native(native) => native_name(native).to_owned(),
        ColumnType::Collection { frozen, typ } => {
            let inner = match typ {
                CollectionType::List(element) => format!("list<{}>", type_name(element)),
                CollectionType::Set(element) => format!("set<{}>", type_name(element)),
                CollectionType::Map(key, value) => {
                    format!("map<{}, {}>", type_name(key), type_name(value))
                }
                _ => "collection".to_owned(),
            };
            frozen_wrapped(*frozen, inner)
        }
        ColumnType::Vector { typ, dimensions } => {
            format!("vector<{}, {dimensions}>", type_name(typ))
        }
        ColumnType::UserDefinedType { frozen, definition } => {
            frozen_wrapped(*frozen, definition.name.to_string())
        }
        ColumnType::Tuple(elements) => {
            let names: Vec<String> = elements.iter().map(type_name).collect();
            format!("tuple<{}>", names.join(", "))
        }
        _ => "unknown".to_owned(),
    }
}

fn frozen_wrapped(frozen: bool, inner: String) -> String {
    if frozen {
        format!("frozen<{inner}>")
    } else {
        inner
    }
}

fn native_name(native: &NativeType) -> &'static str {
    match native {
        NativeType::Ascii => "ascii",
        NativeType::Boolean => "boolean",
        NativeType::Blob => "blob",
        NativeType::Counter => "counter",
        NativeType::Date => "date",
        NativeType::Decimal => "decimal",
        NativeType::Double => "double",
        NativeType::Duration => "duration",
        NativeType::Float => "float",
        NativeType::Int => "int",
        NativeType::BigInt => "bigint",
        NativeType::Text => "text",
        NativeType::Timestamp => "timestamp",
        NativeType::Inet => "inet",
        NativeType::SmallInt => "smallint",
        NativeType::TinyInt => "tinyint",
        NativeType::Time => "time",
        NativeType::Timeuuid => "timeuuid",
        NativeType::Uuid => "uuid",
        NativeType::Varint => "varint",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_types_are_rendered_in_cql_syntax() {
        let map = ColumnType::Collection {
            frozen: true,
            typ: CollectionType::Map(
                Box::new(ColumnType::Native(NativeType::Text)),
                Box::new(ColumnType::Collection {
                    frozen: false,
                    typ: CollectionType::List(Box::new(ColumnType::Native(NativeType::Int))),
                }),
            ),
        };
        assert_eq!(type_name(&map), "frozen<map<text, list<int>>>");
        let tuple = ColumnType::Tuple(vec![
            ColumnType::Native(NativeType::Uuid),
            ColumnType::Native(NativeType::Timestamp),
        ]);
        assert_eq!(type_name(&tuple), "tuple<uuid, timestamp>");
    }
}
