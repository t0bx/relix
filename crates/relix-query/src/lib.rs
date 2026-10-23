/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod classify;
pub mod executor;
pub mod export;
pub mod history;
pub mod lexer;
pub mod ordering;
pub mod splitter;
pub mod statement_words;
pub mod vocabulary;
pub mod wrapping;

pub use classify::{StatementKind, StatementTraits, classify};
pub use executor::{ScriptOutcome, ScriptRequest, StatementReport, run_script};
pub use history::{HistoryEntry, QueryHistory, RecentList};
pub use lexer::{Token, TokenKind, tokenize};
pub use splitter::{StatementSpan, split_statements, statement_at};
pub use statement_words::{TopLevelWords, top_level_words};
