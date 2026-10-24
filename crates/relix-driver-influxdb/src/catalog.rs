/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::connection::DATABASES_PATH;
use crate::json_lines::Record;
use crate::query_client::query_records;
use crate::query_language::QueryLanguage;
use relix_core::{DatabaseInfo, Dialect, ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;
use relix_driver_http::{HttpEndpoint, Method};
use serde_json::Value as Json;

pub(crate) const MEASUREMENT_SCHEMA: &str = "iox";
const DATABASE_KEY: &str = "iox::database";
const SCHEMA_ORDER: [&str; 3] = [MEASUREMENT_SCHEMA, "system", "information_schema"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogColumn {
    pub(crate) name: String,
    pub(crate) data_type: String,
    pub(crate) nullable: bool,
}

impl CatalogColumn {
    /// Describes the role of a column. Measurements store tags as dictionary-encoded
    /// strings and the point time as the `time` timestamp; everything else is a field.
    pub(crate) fn role(&self, schema: &str) -> &'static str {
        if schema != MEASUREMENT_SCHEMA {
            "column"
        } else if self.data_type.starts_with("Dictionary(") {
            "tag"
        } else if self.name == "time" && self.data_type.starts_with("Timestamp") {
            "time"
        } else {
            "field"
        }
    }
}

pub(crate) async fn databases(
    endpoint: &HttpEndpoint,
    default_database: Option<&str>,
) -> Result<Vec<DatabaseInfo>, DriverError> {
    let listed: Vec<Json> = endpoint
        .json(endpoint.request(Method::GET, DATABASES_PATH))
        .await?;
    let mut names: Vec<String> = listed
        .iter()
        .filter_map(|entry| entry.get(DATABASE_KEY)?.as_str().map(str::to_owned))
        .collect();
    names.sort_by(|left, right| {
        left.starts_with('_')
            .cmp(&right.starts_with('_'))
            .then_with(|| left.cmp(right))
    });
    Ok(names
        .into_iter()
        .map(|name| DatabaseInfo {
            is_default: default_database == Some(name.as_str()),
            name,
        })
        .collect())
}

pub(crate) async fn schemas(
    endpoint: &HttpEndpoint,
    database: &str,
) -> Result<Vec<String>, DriverError> {
    let records = catalog_query(
        endpoint,
        database,
        "SELECT DISTINCT table_schema FROM information_schema.tables",
    )
    .await?;
    let mut schemas: Vec<String> = records
        .iter()
        .filter_map(|record| text(record, "table_schema"))
        .collect();
    schemas.sort_by_key(|schema| {
        (
            SCHEMA_ORDER
                .iter()
                .position(|known| known == schema)
                .unwrap_or(SCHEMA_ORDER.len()),
            schema.clone(),
        )
    });
    Ok(schemas)
}

pub(crate) async fn objects(
    endpoint: &HttpEndpoint,
    database: &str,
    schema: &str,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let sql = format!(
        "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = {}",
        literal(schema)
    );
    let records = catalog_query(endpoint, database, &sql).await?;
    let comment = (schema == MEASUREMENT_SCHEMA).then(|| "measurement".to_owned());
    Ok(records
        .iter()
        .filter_map(|record| {
            let name = text(record, "table_name")?;
            let kind = match text(record, "table_type").as_deref() {
                Some("VIEW") => ObjectKind::View,
                _ => ObjectKind::Table,
            };
            Some(ObjectInfo {
                object: TableRef::new(Some(database.to_owned()), Some(schema.to_owned()), name),
                kind,
                comment: comment.clone(),
            })
        })
        .collect())
}

pub(crate) async fn columns(
    endpoint: &HttpEndpoint,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<CatalogColumn>, DriverError> {
    let sql = format!(
        "SELECT column_name, data_type, is_nullable FROM information_schema.columns \
         WHERE table_schema = {} AND table_name = {} ORDER BY ordinal_position",
        literal(schema),
        literal(table)
    );
    let records = catalog_query(endpoint, database, &sql).await?;
    Ok(records
        .iter()
        .filter_map(|record| {
            Some(CatalogColumn {
                name: text(record, "column_name")?,
                data_type: text(record, "data_type").unwrap_or_default(),
                nullable: text(record, "is_nullable").as_deref() != Some("NO"),
            })
        })
        .collect())
}

async fn catalog_query(
    endpoint: &HttpEndpoint,
    database: &str,
    sql: &str,
) -> Result<Vec<Record>, DriverError> {
    let (records, _) =
        query_records(endpoint, QueryLanguage::Sql, database, sql, usize::MAX).await?;
    Ok(records)
}

fn text(record: &Record, key: &str) -> Option<String> {
    record.get(key)?.as_str().map(str::to_owned)
}

pub(crate) fn literal(text: &str) -> String {
    Dialect::InfluxSql.quote_literal(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str) -> CatalogColumn {
        CatalogColumn {
            name: name.into(),
            data_type: data_type.into(),
            nullable: true,
        }
    }

    #[test]
    fn measurement_columns_are_tags_fields_or_time() {
        assert_eq!(column("host", "Dictionary(Int32, Utf8)").role("iox"), "tag");
        assert_eq!(column("time", "Timestamp(ns)").role("iox"), "time");
        assert_eq!(column("usage", "Float64").role("iox"), "field");
        assert_eq!(column("query_text", "Utf8").role("system"), "column");
    }

    #[test]
    fn literals_escape_quotes() {
        assert_eq!(literal("o'clock"), "'o''clock'");
    }
}
