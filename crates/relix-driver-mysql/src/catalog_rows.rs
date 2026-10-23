/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::values::{plain_text, statement_params};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row};
use relix_driver_api::DriverError;

pub async fn catalog_query(
    conn: &mut Conn,
    sql: &str,
    params: &[&str],
) -> Result<Vec<Row>, DriverError> {
    let params = statement_params(params.iter().map(|param| (*param).into()).collect());
    conn.exec(sql, params).await.map_err(map_error)
}

pub fn text_at(row: &Row, index: usize) -> Option<String> {
    row.as_ref(index).and_then(plain_text)
}

pub fn non_empty_text_at(row: &Row, index: usize) -> Option<String> {
    text_at(row, index).filter(|text| !text.is_empty())
}

pub fn number_at(row: &Row, index: usize) -> Option<u64> {
    text_at(row, index).and_then(|text| text.parse().ok())
}
