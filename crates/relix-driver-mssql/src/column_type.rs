/*
 * @author t0bx
 * @since 23.10.2026
 */

use tiberius::ColumnType;

pub fn column_type_name(column_type: ColumnType) -> &'static str {
    match column_type {
        ColumnType::Null => "null",
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Intn => "int",
        ColumnType::Datetime4 => "smalldatetime",
        ColumnType::Datetime | ColumnType::Datetimen => "datetime",
        ColumnType::Float4 => "real",
        ColumnType::Float8 | ColumnType::Floatn => "float",
        ColumnType::Money => "money",
        ColumnType::Money4 => "smallmoney",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::Decimaln => "decimal",
        ColumnType::Numericn => "numeric",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::BigVarBin => "varbinary",
        ColumnType::BigVarChar => "varchar",
        ColumnType::BigBinary => "binary",
        ColumnType::BigChar => "char",
        ColumnType::NVarchar => "nvarchar",
        ColumnType::NChar => "nchar",
        ColumnType::Xml => "xml",
        ColumnType::Udt => "udt",
        ColumnType::Text => "text",
        ColumnType::Image => "image",
        ColumnType::NText => "ntext",
        ColumnType::SSVariant => "sql_variant",
    }
}

/// Renders a column type as it would be declared, from the `sys.columns` length,
/// precision and scale. `max_length` is in bytes (-1 for `max`), so Unicode lengths are
/// halved; alias types are shown by name only.
pub fn declared_type(
    type_name: &str,
    max_length: i32,
    precision: i32,
    scale: i32,
    user_defined: bool,
) -> String {
    if user_defined {
        return type_name.to_owned();
    }
    let length = |bytes_per_char: i32| {
        if max_length == -1 {
            "max".to_owned()
        } else {
            (max_length / bytes_per_char).to_string()
        }
    };
    match type_name.to_ascii_lowercase().as_str() {
        "varchar" | "char" | "varbinary" | "binary" => format!("{type_name}({})", length(1)),
        "nvarchar" | "nchar" => format!("{type_name}({})", length(2)),
        "decimal" | "numeric" => format!("{type_name}({precision},{scale})"),
        "datetime2" | "time" | "datetimeoffset" => format!("{type_name}({scale})"),
        _ => type_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_precision_and_scale_are_rendered() {
        assert_eq!(declared_type("nvarchar", 40, 0, 0, false), "nvarchar(20)");
        assert_eq!(declared_type("varchar", -1, 0, 0, false), "varchar(max)");
        assert_eq!(declared_type("decimal", 9, 10, 2, false), "decimal(10,2)");
        assert_eq!(declared_type("datetime2", 8, 27, 7, false), "datetime2(7)");
        assert_eq!(declared_type("int", 4, 10, 0, false), "int");
        assert_eq!(declared_type("phone", 40, 0, 0, true), "phone");
    }

    #[test]
    fn wire_types_have_sql_names() {
        assert_eq!(column_type_name(ColumnType::NVarchar), "nvarchar");
        assert_eq!(column_type_name(ColumnType::Decimaln), "decimal");
        assert_eq!(
            column_type_name(ColumnType::DatetimeOffsetn),
            "datetimeoffset"
        );
    }
}
