/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::text_at;
use crate::connection::sql_builder;
use crate::errors::map_error;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row};
use relix_core::{ObjectKind, TableRef};
use relix_driver_api::DriverError;

pub async fn show_create(
    conn: &mut Conn,
    kind: ObjectKind,
    object: &TableRef,
) -> Result<Option<String>, DriverError> {
    let Some((sql, column)) = show_create_statement(kind, &sql_builder().qualified_table(object))
    else {
        return Ok(None);
    };
    let row: Option<Row> = conn.query_first(sql).await.map_err(map_error)?;
    Ok(row.and_then(|row| text_at(&row, column)))
}

fn show_create_statement(kind: ObjectKind, qualified_name: &str) -> Option<(String, usize)> {
    let (object_type, column) = match kind {
        ObjectKind::Table => ("TABLE", 1),
        ObjectKind::View | ObjectKind::MaterializedView => ("VIEW", 1),
        ObjectKind::Sequence => ("SEQUENCE", 1),
        ObjectKind::Function => ("FUNCTION", 2),
        ObjectKind::Procedure => ("PROCEDURE", 2),
        ObjectKind::Trigger => ("TRIGGER", 2),
        ObjectKind::NodeLabel | ObjectKind::RelationshipType => return None,
    };
    Some((
        format!("SHOW CREATE {object_type} {qualified_name}"),
        column,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_quote_names_and_pick_definition_column() {
        let table = TableRef::new(Some("shop".into()), None, "or`ders");
        let qualified = sql_builder().qualified_table(&table);
        assert_eq!(
            show_create_statement(ObjectKind::Table, &qualified),
            Some(("SHOW CREATE TABLE `shop`.`or``ders`".to_owned(), 1))
        );
        assert_eq!(
            show_create_statement(ObjectKind::Procedure, "`p`"),
            Some(("SHOW CREATE PROCEDURE `p`".to_owned(), 2))
        );
    }
}
