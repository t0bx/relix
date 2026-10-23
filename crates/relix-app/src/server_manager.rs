/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ConfigScope, ServerConfigId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerKind {
    Ssh,
    Vpn,
}

impl ServerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Vpn => "VPN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRow {
    pub id: ServerConfigId,
    pub kind: ServerKind,
    pub scope: ConfigScope,
    pub name: String,
    pub detail: String,
    pub status: String,
    pub usages: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerManagerState {
    pub selected: usize,
    pub message: Option<String>,
}

impl ServerManagerState {
    pub fn clamp(&mut self, rows: usize) {
        self.selected = self.selected.min(rows.saturating_sub(1));
    }

    pub fn move_selection(&mut self, delta: isize, rows: usize) {
        if rows == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(rows as isize) as usize;
    }
}

pub const SERVER_MANAGER_HINTS: &str = "n new SSH · v new VPN · Enter edit · d delete · y duplicate · r rename · t test · m move / c copy scope · Space (dis)connect VPN · Esc close";
