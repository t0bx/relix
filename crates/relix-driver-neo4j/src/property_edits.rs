/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::cypher::quote;
use crate::errors::map_error;
use crate::graph_object::{ELEMENT_ID, GraphEntity};
use crate::row_reader::{TransactionRows, bolt_value, optional_integer, read_rows};
use crate::values::{to_value, type_label};
use neo4rs::{BoltMap, BoltString, BoltType, Graph, Row, Txn, query};
use relix_core::{CellChange, ChangeReport, RowChange, Value};
use relix_driver_api::{DriverError, ErrorKind};

/// Applies grid changes to nodes or relationships inside one transaction that is
/// rolled back unless every change hits exactly the expected entity. Rows are
/// addressed by `elementId`; only property cells can be edited and new rows cannot be
/// created from the grid (a node needs labels, a relationship its end nodes).
pub(crate) async fn apply_change_set(
    graph: &Graph,
    database: &str,
    entity: GraphEntity,
    changes: &[RowChange],
) -> Result<ChangeReport, DriverError> {
    let mut txn = graph.start_txn_on(database).await.map_err(map_error)?;
    match apply_all(&mut txn, entity, changes).await {
        Ok(report) => {
            txn.commit().await.map_err(map_error)?;
            Ok(report)
        }
        Err(error) => {
            let _ = txn.rollback().await;
            Err(error)
        }
    }
}

async fn apply_all(
    txn: &mut Txn,
    entity: GraphEntity,
    changes: &[RowChange],
) -> Result<ChangeReport, DriverError> {
    let mut report = ChangeReport::default();
    for (index, change) in changes.iter().enumerate() {
        match change {
            RowChange::Insert { .. } => {
                return Err(DriverError::unsupported(
                    "new nodes and relationships cannot be created from the grid; \
                     use CREATE in the Cypher console",
                ));
            }
            RowChange::Update { key, cells } => {
                update(txn, entity, element_id(key)?, cells, index).await?;
                report.updated += 1;
            }
            RowChange::Delete { key } => {
                delete(txn, entity, element_id(key)?, index).await?;
                report.deleted += 1;
            }
        }
    }
    Ok(report)
}

/// Updates properties guarded by their original values. The edited properties are first
/// re-assigned to themselves, which write-locks the entity, and then read back; the
/// comparison with the grid's original values therefore cannot race with other writers.
/// New values keep the type of the stored value (integer, float, boolean or string).
async fn update(
    txn: &mut Txn,
    entity: GraphEntity,
    id: &str,
    cells: &[CellChange],
    index: usize,
) -> Result<(), DriverError> {
    let variable = entity.variable();
    let mut locks = Vec::with_capacity(cells.len());
    for cell in cells {
        if entity.is_fixed_column(&cell.column) {
            return Err(DriverError::unsupported(format!(
                "{} cannot be edited in the grid; only properties can",
                cell.column
            )));
        }
        let property = format!("{variable}.{}", quote(&cell.column));
        locks.push(format!("{property} = {property}"));
    }
    let lock = format!(
        "{} SET {} RETURN properties({variable}) AS properties",
        entity.match_by_element_id(),
        locks.join(", ")
    );
    let rows = run(txn, query(&lock).param("id", id)).await?;
    let Some(row) = rows.first() else {
        return Err(conflict(index, "the entity no longer exists"));
    };
    let current = match bolt_value(row, "properties")? {
        BoltType::Map(map) => map,
        _ => BoltMap::new(),
    };
    let mut assignments = Vec::with_capacity(cells.len());
    let mut parameters = Vec::with_capacity(cells.len());
    for (position, cell) in cells.iter().enumerate() {
        let stored = current.value.get(&BoltString::new(&cell.column));
        let stored_value = stored.map_or(Value::Null, to_value);
        if stored_value != cell.original {
            return Err(conflict(
                index,
                &format!("property {} was changed by someone else", cell.column),
            ));
        }
        let parameter = format!("value{position}");
        let value = typed_value(&cell.column, &cell.updated, stored)?;
        assignments.push(format!("{variable}.{} = ${parameter}", quote(&cell.column)));
        parameters.push((parameter, value));
    }
    let cypher = format!(
        "{} SET {} RETURN count(*) AS changed",
        entity.match_by_element_id(),
        assignments.join(", ")
    );
    let statement = parameters.into_iter().fold(
        query(&cypher).param("id", id),
        |statement, (name, value)| statement.param(&name, value),
    );
    let rows = run(txn, statement).await?;
    expect_one(&rows, index, "the entity no longer exists")
}

async fn delete(
    txn: &mut Txn,
    entity: GraphEntity,
    id: &str,
    index: usize,
) -> Result<(), DriverError> {
    let cypher = format!(
        "{} DELETE {} RETURN count(*) AS changed",
        entity.match_by_element_id(),
        entity.variable()
    );
    let rows = run(txn, query(&cypher).param("id", id)).await?;
    expect_one(&rows, index, "the entity no longer exists")
}

async fn run(txn: &mut Txn, statement: neo4rs::Query) -> Result<Vec<Row>, DriverError> {
    let stream = txn.execute(statement).await.map_err(map_error)?;
    let (rows, _) = read_rows(TransactionRows { stream, txn }, usize::MAX)
        .await
        .map_err(map_error)?;
    Ok(rows)
}

fn expect_one(rows: &[Row], index: usize, reason: &str) -> Result<(), DriverError> {
    let changed = match rows.first() {
        Some(row) => optional_integer(row, "changed")?.unwrap_or_default(),
        None => 0,
    };
    if changed == 1 {
        Ok(())
    } else {
        Err(conflict(index, reason))
    }
}

fn element_id(key: &[(String, Value)]) -> Result<&str, DriverError> {
    key.iter()
        .find(|(name, _)| name == ELEMENT_ID)
        .and_then(|(_, value)| value.as_text())
        .ok_or_else(|| {
            DriverError::new(
                ErrorKind::AmbiguousRow,
                "graph rows are identified by their elementId, which is missing",
            )
        })
}

fn typed_value(
    property: &str,
    updated: &Value,
    stored: Option<&BoltType>,
) -> Result<BoltType, DriverError> {
    let text = match updated {
        Value::Null => return Ok(BoltType::from(Option::<i64>::None)),
        Value::Bool(flag) => return Ok(BoltType::from(*flag)),
        Value::Int(number) => return Ok(BoltType::from(*number)),
        Value::Float(number) => return Ok(BoltType::from(*number)),
        Value::Bytes(bytes) => return Ok(BoltType::from(bytes.clone())),
        Value::UInt(number) => number.to_string(),
        Value::Text(text) => text.clone(),
    };
    let invalid = |kind: &str| {
        DriverError::query(format!(
            "'{text}' is not a valid {kind} for property {property}, which stores {kind} values"
        ))
    };
    match stored {
        None | Some(BoltType::Null(_)) | Some(BoltType::String(_)) => Ok(BoltType::from(text)),
        Some(BoltType::Integer(_)) => text
            .trim()
            .parse::<i64>()
            .map(BoltType::from)
            .map_err(|_| invalid("integer")),
        Some(BoltType::Float(_)) => text
            .trim()
            .parse::<f64>()
            .map(BoltType::from)
            .map_err(|_| invalid("float")),
        Some(BoltType::Boolean(_)) => match text.trim().to_lowercase().as_str() {
            "true" => Ok(BoltType::from(true)),
            "false" => Ok(BoltType::from(false)),
            _ => Err(invalid("boolean")),
        },
        Some(other) => Err(DriverError::unsupported(format!(
            "property {property} holds a {} value, which cannot be edited in the grid; \
             change it with SET in the Cypher console",
            type_label(other)
        ))),
    }
}

fn conflict(index: usize, reason: &str) -> DriverError {
    DriverError::new(
        ErrorKind::ConcurrentModification,
        format!("change #{}: {reason}; nothing was saved", index + 1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_values_keep_the_stored_type() {
        let integer = BoltType::from(3_i64);
        assert_eq!(
            typed_value("age", &Value::Text(" 42 ".into()), Some(&integer)).unwrap(),
            BoltType::from(42_i64)
        );
        let error = typed_value("age", &Value::Text("x".into()), Some(&integer)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(
            typed_value("name", &Value::Text("42".into()), None).unwrap(),
            BoltType::from("42")
        );
        assert_eq!(
            typed_value(
                "flag",
                &Value::Text("TRUE".into()),
                Some(&BoltType::from(false))
            )
            .unwrap(),
            BoltType::from(true)
        );
    }

    #[test]
    fn complex_values_are_not_edited_in_the_grid() {
        let mut list = neo4rs::BoltList::new();
        list.push(BoltType::from("a"));
        let error = typed_value(
            "tags",
            &Value::Text("[]".into()),
            Some(&BoltType::List(list)),
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
    }

    #[test]
    fn rows_need_an_element_id() {
        let key = vec![("elementId".to_owned(), Value::Text("4:abc:1".into()))];
        assert_eq!(element_id(&key).unwrap(), "4:abc:1");
        assert_eq!(element_id(&[]).unwrap_err().kind, ErrorKind::AmbiguousRow);
    }
}
