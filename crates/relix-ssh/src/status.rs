/*
 * @author t0bx
 * @since 23.10.2026
 */

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelStatus {
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Failed(String),
    Closed,
}

impl TunnelStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Connecting => "connecting".into(),
            Self::Connected => "connected".into(),
            Self::Reconnecting { attempt } => format!("reconnecting (attempt {attempt})"),
            Self::Failed(message) => format!("failed: {message}"),
            Self::Closed => "closed".into(),
        }
    }

    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Connected)
    }
}
