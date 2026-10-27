/*
 * @author t0bx
 * @since 27.10.2026
 */

use crate::lexer::{Token, TokenKind};

const PREDICATE_TERMINATORS: [&str; 4] = ["RETURNING", "ORDER", "LIMIT", "OPTION"];

/// Whether an `UPDATE`/`DELETE` restricts its rows with a top-level `WHERE` clause that is
/// not trivially true. A `WHERE` inside a subquery does not count, and neither do
/// predicates such as `WHERE 1 = 1`, `WHERE 'a' = 'a'`, `WHERE TRUE` or `WHERE 1`.
pub(crate) fn has_effective_predicate(words: &[Token<'_>]) -> bool {
    let mut depth = 0usize;
    let mut predicate: Option<Vec<Token<'_>>> = None;
    for token in words {
        if token.is_symbol("(") {
            depth += 1;
        } else if token.is_symbol(")") {
            depth = depth.saturating_sub(1);
        }
        let top_level = depth == 0 && !token.is_symbol(")");
        match predicate.as_mut() {
            None if top_level && token.is_keyword("WHERE") => predicate = Some(Vec::new()),
            Some(collected) => {
                let ends = top_level
                    && (token.is_symbol(";")
                        || PREDICATE_TERMINATORS
                            .iter()
                            .any(|keyword| token.is_keyword(keyword)));
                if ends {
                    break;
                }
                collected.push(*token);
            }
            None => {}
        }
    }
    predicate.is_some_and(|tokens| !tokens.is_empty() && !is_trivially_true(&tokens))
}

fn is_trivially_true(tokens: &[Token<'_>]) -> bool {
    match tokens {
        [single] if single.is_keyword("TRUE") => true,
        [single] if single.kind == TokenKind::Number => {
            !single.text.trim_start_matches('0').is_empty()
        }
        [left, operator, right] => {
            operator.is_symbol("=")
                && left.text == right.text
                && matches!(left.kind, TokenKind::Number | TokenKind::String)
                && left.kind == right.kind
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{significant, tokenize};
    use relix_core::Dialect;

    fn effective(sql: &str) -> bool {
        let tokens = tokenize(sql, Dialect::Postgres);
        let words: Vec<Token<'_>> = significant(&tokens).collect();
        has_effective_predicate(&words)
    }

    #[test]
    fn real_predicates_restrict_rows() {
        assert!(effective("DELETE FROM t WHERE id = 3"));
        assert!(effective("UPDATE t SET a = 1 WHERE (id = 3) RETURNING *"));
        assert!(effective("DELETE FROM t WHERE 1 = 1 AND id = 3"));
    }

    #[test]
    fn missing_or_trivial_predicates_do_not() {
        assert!(!effective("DELETE FROM t"));
        assert!(!effective("DELETE FROM t WHERE 1=1"));
        assert!(!effective("DELETE FROM t WHERE 'x' = 'x' RETURNING id"));
        assert!(!effective("UPDATE t SET a = 1 WHERE true"));
        assert!(!effective("UPDATE t SET a = 1 WHERE 1;"));
        assert!(!effective(
            "UPDATE t SET a = (SELECT b FROM u WHERE u.id = 1)"
        ));
        assert!(!effective("DELETE FROM t WHERE"));
    }

    #[test]
    fn zero_is_not_trivially_true() {
        assert!(effective("DELETE FROM t WHERE 0"));
    }
}
