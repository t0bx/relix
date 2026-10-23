/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::time::Duration;

const FIRST_DELAY: Duration = Duration::from_millis(50);
const MAX_DELAY: Duration = Duration::from_secs(1);

/// Delays between status polls of an asynchronous server job: short at first so quick
/// statements return promptly, doubling up to one second for long-running ones.
#[derive(Debug, Clone)]
pub struct PollBackoff {
    next: Duration,
}

impl Default for PollBackoff {
    fn default() -> Self {
        Self { next: FIRST_DELAY }
    }
}

impl PollBackoff {
    pub fn next_delay(&mut self) -> Duration {
        let current = self.next;
        self.next = (current * 2).min(MAX_DELAY);
        current
    }

    pub async fn wait(&mut self) {
        tokio::time::sleep(self.next_delay()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_up_to_one_second() {
        let mut backoff = PollBackoff::default();
        let delays: Vec<u128> = (0..7).map(|_| backoff.next_delay().as_millis()).collect();
        assert_eq!(delays, vec![50, 100, 200, 400, 800, 1000, 1000]);
    }
}
