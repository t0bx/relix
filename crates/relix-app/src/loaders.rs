/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::explorer::{DetailKind, FolderKind, NewNode, NodeKind};
use relix_core::{DataSourceId, Dialect, ObjectInfo, ObjectKind, TableDetails};
use relix_driver_api::{
    DatabaseConnection, DriverCapabilities, DriverError, KeyValueConnection, SqlConnection,
};
use relix_editor::{CatalogColumn, CatalogTable, CompletionCatalog};

const SYSTEM_SCHEMAS: &[&str] = &[
    "pg_catalog",
    "information_schema",
    "mysql",
    "performance_schema",
    "sys",
];
const MAX_CATALOG_SCHEMAS: usize = 20;

pub fn sql(connection: &dyn DatabaseConnection) -> Result<&dyn SqlConnection, DriverError> {
    connection
        .as_sql()
        .ok_or_else(|| DriverError::unsupported("this data source does not support SQL"))
}

pub fn key_value(
    connection: &dyn DatabaseConnection,
) -> Result<&dyn KeyValueConnection, DriverError> {
    connection
        .as_key_value()
        .ok_or_else(|| DriverError::unsupported("this data source is not a key-value store"))
}

/// Loads the children of an explorer node. Each level is fetched lazily; schema level
/// results are grouped into folders (tables, views, routines, …) whose contents arrive
/// preloaded, and tables expand into their columns, indexes and foreign keys.
pub async fn load_children(
    connection: &dyn DatabaseConnection,
    capabilities: DriverCapabilities,
    kind: &NodeKind,
    parent_key: &str,
) -> Result<Vec<NewNode>, DriverError> {
    match kind {
        NodeKind::DataSource(source) if !capabilities.is_relational() => Ok(vec![
            NewNode::new(
                format!("{parent_key}/keys"),
                "keys",
                NodeKind::KeySpace(*source),
            )
            .with_detail("browse keyspace"),
        ]),
        NodeKind::DataSource(source) => {
            let databases = sql(connection)?.databases().await?;
            let mut nodes: Vec<NewNode> = databases
                .into_iter()
                .map(|database| {
                    let node = NewNode::new(
                        format!("{parent_key}/db:{}", database.name),
                        database.name.clone(),
                        NodeKind::Database {
                            source: *source,
                            name: database.name,
                        },
                    );
                    if database.is_default {
                        node.with_detail("default")
                    } else {
                        node
                    }
                })
                .collect();
            nodes.sort_by_key(|node| node.detail.is_none());
            Ok(nodes)
        }
        NodeKind::Database { source, name } if capabilities.schemas => {
            let schemas = sql(connection)?.schemas(name).await?;
            Ok(schemas
                .into_iter()
                .map(|schema| {
                    NewNode::new(
                        format!("{parent_key}/schema:{schema}"),
                        schema.clone(),
                        NodeKind::Schema {
                            source: *source,
                            database: name.clone(),
                            name: schema,
                        },
                    )
                })
                .collect())
        }
        NodeKind::Database { source, name } => {
            let objects = sql(connection)?.objects(name, None).await?;
            Ok(object_folders(*source, parent_key, objects))
        }
        NodeKind::Schema {
            source,
            database,
            name,
        } => {
            let objects = sql(connection)?.objects(database, Some(name)).await?;
            Ok(object_folders(*source, parent_key, objects))
        }
        NodeKind::Object { source, info } if info.kind.is_relation() => {
            let details = sql(connection)?.table_details(&info.object).await?;
            Ok(table_children(*source, parent_key, &details))
        }
        _ => Ok(Vec::new()),
    }
}

fn object_folders(
    source: DataSourceId,
    parent_key: &str,
    objects: Vec<ObjectInfo>,
) -> Vec<NewNode> {
    let order = [
        FolderKind::Tables,
        FolderKind::Views,
        FolderKind::Routines,
        FolderKind::Sequences,
        FolderKind::Triggers,
    ];
    order
        .into_iter()
        .filter_map(|folder| {
            let mut members: Vec<&ObjectInfo> = objects
                .iter()
                .filter(|object| FolderKind::of(object.kind) == folder)
                .collect();
            if members.is_empty() && folder != FolderKind::Tables {
                return None;
            }
            members.sort_by(|a, b| a.object.name.cmp(&b.object.name));
            let folder_key = format!("{parent_key}/{}", folder.label());
            let children = members
                .iter()
                .map(|object| {
                    let node = NewNode::new(
                        format!("{folder_key}/{}", object.object.name),
                        object.object.name.clone(),
                        NodeKind::Object {
                            source,
                            info: (*object).clone(),
                        },
                    );
                    match object.kind {
                        ObjectKind::MaterializedView => node.with_detail("materialized"),
                        ObjectKind::Procedure => node.with_detail("procedure"),
                        _ => node,
                    }
                })
                .collect();
            Some(
                NewNode::new(
                    folder_key,
                    folder.label(),
                    NodeKind::Folder { source, folder },
                )
                .with_detail(members.len().to_string())
                .with_children(children),
            )
        })
        .collect()
}

fn table_children(source: DataSourceId, parent_key: &str, details: &TableDetails) -> Vec<NewNode> {
    let detail_node = |detail: DetailKind, count: usize, children: Vec<NewNode>| {
        NewNode::new(
            format!("{parent_key}/{}", detail.label()),
            detail.label(),
            NodeKind::Detail {
                source,
                table: details.table.clone(),
                detail,
            },
        )
        .with_detail(count.to_string())
        .with_children(children)
    };
    let columns = details
        .columns
        .iter()
        .map(|column| {
            let mut flags = vec![column.data_type.clone()];
            if column.is_primary_key {
                flags.push("PK".into());
            }
            if !column.nullable {
                flags.push("NOT NULL".into());
            }
            NewNode::new(
                format!("{parent_key}/columns/{}", column.name),
                column.name.clone(),
                NodeKind::Column(column.clone()),
            )
            .with_detail(flags.join(" "))
        })
        .collect();
    let indexes = details
        .indexes
        .iter()
        .map(|index| {
            let kind = if index.is_primary {
                "primary"
            } else if index.is_unique {
                "unique"
            } else {
                "index"
            };
            NewNode::new(
                format!("{parent_key}/indexes/{}", index.name),
                index.name.clone(),
                NodeKind::Index(index.clone()),
            )
            .with_detail(format!("{kind} ({})", index.columns.join(", ")))
        })
        .collect();
    let foreign_keys = details
        .foreign_keys
        .iter()
        .map(|key| {
            NewNode::new(
                format!("{parent_key}/fks/{}", key.name),
                key.name.clone(),
                NodeKind::ForeignKey(key.clone()),
            )
            .with_detail(format!(
                "({}) → {}({})",
                key.columns.join(", "),
                key.referenced_table,
                key.referenced_columns.join(", ")
            ))
        })
        .collect();
    vec![
        detail_node(DetailKind::Columns, details.columns.len(), columns),
        detail_node(DetailKind::Indexes, details.indexes.len(), indexes),
        detail_node(
            DetailKind::ForeignKeys,
            details.foreign_keys.len(),
            foreign_keys,
        ),
    ]
}

/// Builds the completion catalog from live metadata of the default database: its
/// schemas, tables and routines. Columns are fetched later, on demand.
pub async fn load_catalog(
    connection: &dyn DatabaseConnection,
    capabilities: DriverCapabilities,
    dialect: Dialect,
    configured_database: Option<String>,
) -> Result<CompletionCatalog, DriverError> {
    let sql = sql(connection)?;
    let mut catalog = CompletionCatalog::empty(dialect);
    let databases = sql.databases().await?;
    let database = configured_database
        .filter(|name| databases.iter().any(|database| &database.name == name))
        .or_else(|| {
            databases
                .iter()
                .find(|database| database.is_default)
                .map(|database| database.name.clone())
        })
        .or_else(|| databases.first().map(|database| database.name.clone()));
    let Some(database) = database else {
        return Ok(catalog);
    };
    let schemas: Vec<Option<String>> = if capabilities.schemas {
        let schemas = sql.schemas(&database).await?;
        catalog.schemas = schemas.clone();
        catalog.default_schema = schemas
            .iter()
            .find(|schema| schema.as_str() == "public")
            .or_else(|| schemas.first())
            .cloned();
        schemas
            .into_iter()
            .filter(|schema| !SYSTEM_SCHEMAS.contains(&schema.as_str()))
            .take(MAX_CATALOG_SCHEMAS)
            .map(Some)
            .collect()
    } else {
        catalog.schemas = databases
            .iter()
            .map(|database| database.name.clone())
            .collect();
        catalog.default_schema = Some(database.clone());
        vec![None]
    };
    for schema in schemas {
        for object in sql.objects(&database, schema.as_deref()).await? {
            if object.kind.is_relation() {
                catalog.tables.push(CatalogTable {
                    schema: object
                        .object
                        .schema
                        .clone()
                        .or_else(|| object.object.database.clone()),
                    name: object.object.name.clone(),
                    kind: object.kind,
                    columns: None,
                });
            } else if matches!(object.kind, ObjectKind::Function | ObjectKind::Procedure) {
                catalog.routines.push(object.object.name.clone());
            }
        }
    }
    Ok(catalog)
}

pub fn catalog_columns(details: &TableDetails) -> Vec<CatalogColumn> {
    details
        .columns
        .iter()
        .map(|column| CatalogColumn {
            name: column.name.clone(),
            data_type: column.data_type.clone(),
        })
        .collect()
}
