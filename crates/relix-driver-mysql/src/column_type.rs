/*
 * @author t0bx
 * @since 23.10.2026
 */

use mysql_async::Column;
use mysql_async::consts::{ColumnFlags, ColumnType};

pub const BINARY_CHARSET: u16 = 63;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueClass {
    Null,
    SignedInteger,
    UnsignedInteger,
    Float,
    Bit,
    Textual,
    Binary,
    Character,
}

pub fn is_binary(column: &Column) -> bool {
    column.character_set() == BINARY_CHARSET
}

pub fn value_class(column: &Column) -> ValueClass {
    use ColumnType::*;
    match column.column_type() {
        MYSQL_TYPE_NULL => ValueClass::Null,
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG
        | MYSQL_TYPE_LONGLONG | MYSQL_TYPE_YEAR => {
            if column.flags().contains(ColumnFlags::UNSIGNED_FLAG) {
                ValueClass::UnsignedInteger
            } else {
                ValueClass::SignedInteger
            }
        }
        MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE => ValueClass::Float,
        MYSQL_TYPE_BIT => ValueClass::Bit,
        MYSQL_TYPE_DECIMAL
        | MYSQL_TYPE_NEWDECIMAL
        | MYSQL_TYPE_DATE
        | MYSQL_TYPE_NEWDATE
        | MYSQL_TYPE_TIME
        | MYSQL_TYPE_TIME2
        | MYSQL_TYPE_DATETIME
        | MYSQL_TYPE_DATETIME2
        | MYSQL_TYPE_TIMESTAMP
        | MYSQL_TYPE_TIMESTAMP2
        | MYSQL_TYPE_JSON
        | MYSQL_TYPE_ENUM
        | MYSQL_TYPE_SET => ValueClass::Textual,
        MYSQL_TYPE_GEOMETRY | MYSQL_TYPE_VECTOR => ValueClass::Binary,
        MYSQL_TYPE_VARCHAR
        | MYSQL_TYPE_VAR_STRING
        | MYSQL_TYPE_STRING
        | MYSQL_TYPE_TINY_BLOB
        | MYSQL_TYPE_MEDIUM_BLOB
        | MYSQL_TYPE_LONG_BLOB
        | MYSQL_TYPE_BLOB => {
            if is_binary(column) {
                ValueClass::Binary
            } else {
                ValueClass::Character
            }
        }
        MYSQL_TYPE_TYPED_ARRAY | MYSQL_TYPE_UNKNOWN => ValueClass::Character,
    }
}

pub fn type_name(column: &Column) -> String {
    use ColumnType::*;
    let flags = column.flags();
    let binary = is_binary(column);
    let pick = |binary_name: &'static str, text_name: &'static str| {
        if binary { binary_name } else { text_name }
    };
    let name = match column.column_type() {
        MYSQL_TYPE_TINY => "TINYINT",
        MYSQL_TYPE_SHORT => "SMALLINT",
        MYSQL_TYPE_INT24 => "MEDIUMINT",
        MYSQL_TYPE_LONG => "INT",
        MYSQL_TYPE_LONGLONG => "BIGINT",
        MYSQL_TYPE_YEAR => "YEAR",
        MYSQL_TYPE_FLOAT => "FLOAT",
        MYSQL_TYPE_DOUBLE => "DOUBLE",
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        MYSQL_TYPE_NULL => "NULL",
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => "DATE",
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => "TIME",
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => "DATETIME",
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        MYSQL_TYPE_BIT => "BIT",
        MYSQL_TYPE_JSON => "JSON",
        MYSQL_TYPE_ENUM => "ENUM",
        MYSQL_TYPE_SET => "SET",
        MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        MYSQL_TYPE_VECTOR => "VECTOR",
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => pick("VARBINARY", "VARCHAR"),
        MYSQL_TYPE_STRING if flags.contains(ColumnFlags::ENUM_FLAG) => "ENUM",
        MYSQL_TYPE_STRING if flags.contains(ColumnFlags::SET_FLAG) => "SET",
        MYSQL_TYPE_STRING => pick("BINARY", "CHAR"),
        MYSQL_TYPE_TINY_BLOB => pick("TINYBLOB", "TINYTEXT"),
        MYSQL_TYPE_MEDIUM_BLOB => pick("MEDIUMBLOB", "MEDIUMTEXT"),
        MYSQL_TYPE_LONG_BLOB => pick("LONGBLOB", "LONGTEXT"),
        MYSQL_TYPE_BLOB => pick("BLOB", "TEXT"),
        MYSQL_TYPE_TYPED_ARRAY | MYSQL_TYPE_UNKNOWN => "UNKNOWN",
    };
    if value_class(column) == ValueClass::UnsignedInteger {
        format!("{name} UNSIGNED")
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTF8MB4: u16 = 255;

    fn column(column_type: ColumnType, charset: u16, flags: ColumnFlags) -> Column {
        Column::new(column_type)
            .with_character_set(charset)
            .with_flags(flags)
    }

    #[test]
    fn integer_names_include_signedness() {
        let signed = column(
            ColumnType::MYSQL_TYPE_LONG,
            BINARY_CHARSET,
            ColumnFlags::empty(),
        );
        let unsigned = column(
            ColumnType::MYSQL_TYPE_LONGLONG,
            BINARY_CHARSET,
            ColumnFlags::UNSIGNED_FLAG,
        );
        assert_eq!(type_name(&signed), "INT");
        assert_eq!(value_class(&signed), ValueClass::SignedInteger);
        assert_eq!(type_name(&unsigned), "BIGINT UNSIGNED");
        assert_eq!(value_class(&unsigned), ValueClass::UnsignedInteger);
    }

    #[test]
    fn string_names_depend_on_binary_charset() {
        let varchar = column(
            ColumnType::MYSQL_TYPE_VAR_STRING,
            UTF8MB4,
            ColumnFlags::empty(),
        );
        let varbinary = column(
            ColumnType::MYSQL_TYPE_VAR_STRING,
            BINARY_CHARSET,
            ColumnFlags::BINARY_FLAG,
        );
        let blob = column(
            ColumnType::MYSQL_TYPE_BLOB,
            BINARY_CHARSET,
            ColumnFlags::BLOB_FLAG,
        );
        let text = column(ColumnType::MYSQL_TYPE_BLOB, UTF8MB4, ColumnFlags::BLOB_FLAG);
        assert_eq!(type_name(&varchar), "VARCHAR");
        assert_eq!(value_class(&varchar), ValueClass::Character);
        assert_eq!(type_name(&varbinary), "VARBINARY");
        assert_eq!(value_class(&varbinary), ValueClass::Binary);
        assert_eq!(type_name(&blob), "BLOB");
        assert_eq!(type_name(&text), "TEXT");
    }

    #[test]
    fn enum_and_set_are_detected_from_flags() {
        let enumeration = column(
            ColumnType::MYSQL_TYPE_STRING,
            UTF8MB4,
            ColumnFlags::ENUM_FLAG,
        );
        let set = column(
            ColumnType::MYSQL_TYPE_STRING,
            UTF8MB4,
            ColumnFlags::SET_FLAG,
        );
        assert_eq!(type_name(&enumeration), "ENUM");
        assert_eq!(type_name(&set), "SET");
    }

    #[test]
    fn temporal_and_decimal_types_are_textual() {
        for column_type in [
            ColumnType::MYSQL_TYPE_NEWDECIMAL,
            ColumnType::MYSQL_TYPE_DATETIME,
            ColumnType::MYSQL_TYPE_DATE,
            ColumnType::MYSQL_TYPE_JSON,
        ] {
            let column = column(column_type, BINARY_CHARSET, ColumnFlags::BINARY_FLAG);
            assert_eq!(value_class(&column), ValueClass::Textual);
        }
    }
}
