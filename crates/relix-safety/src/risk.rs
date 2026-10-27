/*
 * @author t0bx
 * @since 27.10.2026
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    UnboundedWrite,
    DestructiveDefinition,
    DataRemoval,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Self::UnboundedWrite => "no effective WHERE clause",
            Self::DestructiveDefinition => "destructive schema change",
            Self::DataRemoval => "removes data",
        }
    }
}
