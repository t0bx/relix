/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::time::{Duration, Instant};

const MAX_NOTIFICATIONS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

impl Level {
    fn lifetime(self) -> Duration {
        match self {
            Self::Info | Self::Success => Duration::from_secs(4),
            Self::Warning => Duration::from_secs(8),
            Self::Error => Duration::from_secs(12),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub message: String,
    pub level: Level,
    pub created: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct Notifications {
    items: Vec<Notification>,
}

impl Notifications {
    pub fn push(&mut self, level: Level, message: impl Into<String>) {
        let message = message.into();
        tracing::debug!(?level, %message, "notification");
        self.items.retain(|item| item.message != message);
        self.items.push(Notification {
            message,
            level,
            created: Instant::now(),
        });
        let overflow = self.items.len().saturating_sub(MAX_NOTIFICATIONS);
        self.items.drain(..overflow);
    }

    pub fn expire(&mut self) -> bool {
        let before = self.items.len();
        self.items
            .retain(|item| item.created.elapsed() < item.level.lifetime());
        before != self.items.len()
    }

    pub fn dismiss_all(&mut self) {
        self.items.clear();
    }

    pub fn items(&self) -> &[Notification] {
        &self.items
    }

    pub fn latest(&self) -> Option<&Notification> {
        self.items.last()
    }
}
