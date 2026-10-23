/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::models::TableField;

/// GoogleSQL type of a schema field, translating the legacy names the REST API reports
/// (`INTEGER`, `RECORD`, …) and spelling out `STRUCT<…>` and `ARRAY<…>`.
pub(crate) fn type_label(field: &TableField) -> String {
    let base = if field.is_record() {
        let members: Vec<String> = field
            .fields
            .iter()
            .map(|member| format!("{} {}", member.name, type_label(member)))
            .collect();
        format!("STRUCT<{}>", members.join(", "))
    } else {
        standard_name(&field.field_type).to_owned()
    };
    if field.is_repeated() {
        format!("ARRAY<{base}>")
    } else {
        base
    }
}

fn standard_name(legacy: &str) -> &str {
    match legacy.to_ascii_uppercase().as_str() {
        "INTEGER" => "INT64",
        "FLOAT" => "FLOAT64",
        "BOOLEAN" => "BOOL",
        _ => legacy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(
        name: &str,
        field_type: &str,
        mode: Option<&str>,
        fields: Vec<TableField>,
    ) -> TableField {
        TableField {
            name: name.into(),
            field_type: field_type.into(),
            mode: mode.map(str::to_owned),
            fields,
            ..TableField::default()
        }
    }

    #[test]
    fn nested_and_repeated_fields_use_standard_sql_names() {
        assert_eq!(type_label(&field("id", "INTEGER", None, vec![])), "INT64");
        assert_eq!(
            type_label(&field("tags", "STRING", Some("REPEATED"), vec![])),
            "ARRAY<STRING>"
        );
        let record = field(
            "meta",
            "RECORD",
            Some("NULLABLE"),
            vec![
                field("a", "INTEGER", None, vec![]),
                field("ok", "BOOLEAN", Some("REPEATED"), vec![]),
            ],
        );
        assert_eq!(type_label(&record), "STRUCT<a INT64, ok ARRAY<BOOL>>");
    }
}
