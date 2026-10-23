/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::{DriverError, ErrorKind};
use std::future::Future;
use std::time::Duration;

pub async fn with_timeout<T, F>(duration: Duration, operation: F) -> Result<T, DriverError>
where
    F: Future<Output = Result<T, DriverError>>,
{
    match tokio::time::timeout(duration, operation).await {
        Ok(result) => result,
        Err(_) => Err(DriverError::new(
            ErrorKind::Timeout,
            format!("operation exceeded {} seconds", duration.as_secs_f32()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn elapsed_operations_become_timeout_errors() {
        let result: Result<(), _> = with_timeout(Duration::from_millis(5), async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(())
        })
        .await;
        assert_eq!(result.unwrap_err().kind, ErrorKind::Timeout);
    }
}
