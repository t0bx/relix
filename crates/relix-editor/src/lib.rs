/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod buffer;
pub mod catalog;
pub mod completion;
pub mod context;
pub mod diagnostics;
pub mod editing;
pub mod highlight;
pub mod history;
pub mod movement;
pub mod search;

pub use buffer::{Position, TextBuffer};
pub use catalog::{CatalogColumn, CatalogTable, CompletionCatalog};
pub use completion::{CompletionItem, CompletionKind, CompletionResult, complete};
pub use diagnostics::{Diagnostic, Severity};
pub use highlight::{HighlightClass, HighlightSpan, highlight_lines};
pub use search::SearchState;
