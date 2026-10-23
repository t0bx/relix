/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::runner::CommandRunner;
use relix_core::keyvalue::{KeyName, KeyScanPage};
use relix_driver_api::DriverError;

pub async fn scan_keys(
    runner: &CommandRunner,
    cursor: u64,
    pattern: &str,
    count: u32,
) -> Result<KeyScanPage, DriverError> {
    let mut command = redis::cmd("SCAN");
    command.arg(cursor);
    if !pattern.is_empty() {
        command.arg("MATCH").arg(pattern);
    }
    command.arg("COUNT").arg(count.max(1));
    let (next_cursor, keys): (u64, Vec<Vec<u8>>) = runner.query(&command).await?;
    Ok(KeyScanPage {
        next_cursor,
        keys: keys.into_iter().map(KeyName).collect(),
    })
}
