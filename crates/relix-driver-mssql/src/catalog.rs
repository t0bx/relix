/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::{in_database, query_rows, text_at};
use crate::connector::MsSqlClient;
use relix_core::{ObjectInfo, ObjectKind, TableRef};
use relix_driver_api::DriverError;

const DATABASES_SQL: &str = "SELECT name FROM sys.databases \
     WHERE state = 0 AND HAS_DBACCESS(name) = 1 ORDER BY name";

const SCHEMAS_SQL: &str = "SELECT s.name FROM {db}.sys.schemas s \
     WHERE s.name NOT IN (N'sys', N'INFORMATION_SCHEMA', N'guest') \
     AND s.schema_id NOT BETWEEN 16384 AND 16393 \
     ORDER BY s.name";

const OBJECTS_SQL: &str = "SELECT s.name, o.name, RTRIM(o.type), CAST(ep.value AS nvarchar(4000)) \
     FROM {db}.sys.objects o \
     JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id \
     LEFT JOIN {db}.sys.extended_properties ep ON ep.class = 1 AND ep.major_id = o.object_id \
       AND ep.minor_id = 0 AND ep.name = N'MS_Description' \
     WHERE (@P1 IS NULL OR s.name = @P1) AND o.is_ms_shipped = 0 \
       AND o.type IN ('U', 'V', 'P', 'PC', 'FN', 'IF', 'TF', 'FS', 'FT', 'TR', 'SO') \
     ORDER BY s.name, o.name";

pub async fn list_databases(client: &mut MsSqlClient) -> Result<Vec<String>, DriverError> {
    let rows = query_rows(client, DATABASES_SQL, &[]).await?;
    Ok(rows.iter().filter_map(|row| text_at(row, 0)).collect())
}

pub async fn list_schemas(
    client: &mut MsSqlClient,
    database: &str,
) -> Result<Vec<String>, DriverError> {
    let rows = query_rows(client, &in_database(SCHEMAS_SQL, database), &[]).await?;
    Ok(rows.iter().filter_map(|row| text_at(row, 0)).collect())
}

pub async fn list_objects(
    client: &mut MsSqlClient,
    database: &str,
    schema: Option<&str>,
) -> Result<Vec<ObjectInfo>, DriverError> {
    let rows = query_rows(client, &in_database(OBJECTS_SQL, database), &[&schema]).await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some(ObjectInfo {
                object: TableRef::new(
                    Some(database.to_owned()),
                    Some(text_at(row, 0)?),
                    text_at(row, 1)?,
                ),
                kind: object_kind(&text_at(row, 2)?)?,
                comment: text_at(row, 3).filter(|comment| !comment.is_empty()),
            })
        })
        .collect())
}

pub fn object_kind(object_type: &str) -> Option<ObjectKind> {
    match object_type {
        "U" => Some(ObjectKind::Table),
        "V" => Some(ObjectKind::View),
        "P" | "PC" => Some(ObjectKind::Procedure),
        "FN" | "IF" | "TF" | "FS" | "FT" => Some(ObjectKind::Function),
        "TR" => Some(ObjectKind::Trigger),
        "SO" => Some(ObjectKind::Sequence),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_types_map_to_kinds() {
        assert_eq!(object_kind("U"), Some(ObjectKind::Table));
        assert_eq!(object_kind("IF"), Some(ObjectKind::Function));
        assert_eq!(object_kind("PC"), Some(ObjectKind::Procedure));
        assert_eq!(object_kind("SO"), Some(ObjectKind::Sequence));
        assert_eq!(object_kind("TR"), Some(ObjectKind::Trigger));
        assert_eq!(object_kind("IT"), None);
    }
}
