/*
 * @author t0bx
 * @since 23.10.2026
 */

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogType {
    pub data_type: String,
    pub data_length: i64,
    pub char_length: i64,
    pub char_semantics: bool,
    pub precision: Option<i64>,
    pub scale: Option<i64>,
}

/// Renders a column type from `ALL_TAB_COLUMNS` as it would be declared. Types whose
/// catalog name already carries their modifiers (`TIMESTAMP(6)`, `INTERVAL …`) are kept.
pub fn declared_type(column: &CatalogType) -> String {
    let name = column.data_type.as_str();
    match name {
        "VARCHAR2" | "CHAR" if column.char_semantics => {
            format!("{name}({} CHAR)", column.char_length)
        }
        "VARCHAR2" | "CHAR" | "NVARCHAR2" | "NCHAR" => format!("{name}({})", column.char_length),
        "RAW" => format!("{name}({})", column.data_length),
        "FLOAT" => match column.precision {
            Some(precision) => format!("FLOAT({precision})"),
            None => name.to_owned(),
        },
        "NUMBER" => match (column.precision, column.scale) {
            (Some(precision), Some(0) | None) => format!("NUMBER({precision})"),
            (Some(precision), Some(scale)) => format!("NUMBER({precision},{scale})"),
            (None, Some(0)) => "NUMBER(*,0)".to_owned(),
            (None, _) => name.to_owned(),
        },
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(data_type: &str) -> CatalogType {
        CatalogType {
            data_type: data_type.into(),
            ..CatalogType::default()
        }
    }

    #[test]
    fn modifiers_follow_the_catalog() {
        let varchar = CatalogType {
            char_length: 20,
            ..catalog("VARCHAR2")
        };
        assert_eq!(declared_type(&varchar), "VARCHAR2(20)");
        let chars = CatalogType {
            char_semantics: true,
            ..varchar
        };
        assert_eq!(declared_type(&chars), "VARCHAR2(20 CHAR)");
        let amount = CatalogType {
            precision: Some(10),
            scale: Some(2),
            ..catalog("NUMBER")
        };
        assert_eq!(declared_type(&amount), "NUMBER(10,2)");
        let integer = CatalogType {
            scale: Some(0),
            ..catalog("NUMBER")
        };
        assert_eq!(declared_type(&integer), "NUMBER(*,0)");
        assert_eq!(declared_type(&catalog("NUMBER")), "NUMBER");
        assert_eq!(declared_type(&catalog("TIMESTAMP(6)")), "TIMESTAMP(6)");
        let raw = CatalogType {
            data_length: 16,
            ..catalog("RAW")
        };
        assert_eq!(declared_type(&raw), "RAW(16)");
    }
}
