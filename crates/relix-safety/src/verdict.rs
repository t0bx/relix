/*
 * @author t0bx
 * @since 27.10.2026
 */

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Confirm(Confirmation),
    Deny(Denial),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    pub title: String,
    pub lines: Vec<String>,
    pub confirm_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub reason: String,
}

impl Verdict {
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Deny(_))
    }
}
