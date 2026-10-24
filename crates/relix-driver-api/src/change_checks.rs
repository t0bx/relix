/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::error::{DriverError, ErrorKind};

pub fn ensure_single_row(index: usize, operation: &str, affected: u64) -> Result<(), DriverError> {
    if affected == 1 {
        return Ok(());
    }
    Err(DriverError::new(
        ErrorKind::ConcurrentModification,
        format!(
            "change #{} ({operation}) matched {affected} rows instead of exactly one; \
             all changes were rolled back",
            index + 1
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_row_check_reports_failing_change() {
        assert!(ensure_single_row(0, "update", 1).is_ok());
        let error = ensure_single_row(2, "delete", 0).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ConcurrentModification);
        assert!(error.message.contains("change #3 (delete) matched 0 rows"));
        assert!(
            ensure_single_row(1, "update", 2)
                .unwrap_err()
                .message
                .contains("2 rows")
        );
    }
}
