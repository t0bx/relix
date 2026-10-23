/*
 * @author t0bx
 * @since 23.10.2026
 */

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StatementScan {
    pub(crate) keywords: Vec<String>,
    pub(crate) has_multiple_statements: bool,
}

/// Lexes SQL just far enough to know its top-level words: comments, string literals
/// (including `E''` escapes), quoted identifiers and dollar-quoted bodies are skipped and
/// parenthesised parts are ignored. Words are returned upper-cased in source order, and
/// any content after a top-level `;` marks the text as containing several statements.
pub(crate) fn scan_statement(sql: &str) -> StatementScan {
    let chars: Vec<char> = sql.chars().collect();
    let mut scan = StatementScan::default();
    let mut depth = 0usize;
    let mut statement_ended = false;
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        if current.is_whitespace() {
            index += 1;
            continue;
        }
        if current == '-' && next == Some('-') {
            index = skip_line_comment(&chars, index);
            continue;
        }
        if current == '/' && next == Some('*') {
            index = skip_block_comment(&chars, index);
            continue;
        }
        if statement_ended {
            if current != ';' {
                scan.has_multiple_statements = true;
            }
            index += 1;
            continue;
        }
        index = match current {
            '\'' => skip_quoted(&chars, index, '\'', false),
            '"' => skip_quoted(&chars, index, '"', false),
            '$' => skip_dollar_quote(&chars, index),
            '(' => {
                depth += 1;
                index + 1
            }
            ')' => {
                depth = depth.saturating_sub(1);
                index + 1
            }
            ';' if depth == 0 => {
                statement_ended = true;
                index + 1
            }
            ch if ch.is_alphabetic() || ch == '_' => {
                let end = word_end(&chars, index);
                if matches!(current, 'e' | 'E') && end == index + 1 && next == Some('\'') {
                    skip_quoted(&chars, end, '\'', true)
                } else {
                    if depth == 0 {
                        let word: String = chars[index..end].iter().collect();
                        scan.keywords.push(word.to_uppercase());
                    }
                    end
                }
            }
            _ => index + 1,
        };
    }
    scan
}

fn word_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && (chars[end].is_alphanumeric() || matches!(chars[end], '_' | '$')) {
        end += 1;
    }
    end
}

fn skip_line_comment(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && chars[index] != '\n' {
        index += 1;
    }
    index
}

fn skip_block_comment(chars: &[char], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    while index < chars.len() {
        match (chars[index], chars.get(index + 1).copied()) {
            ('/', Some('*')) => {
                depth += 1;
                index += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    index
}

fn skip_quoted(chars: &[char], start: usize, quote: char, backslash_escapes: bool) -> usize {
    let mut index = start + 1;
    while index < chars.len() {
        let current = chars[index];
        if backslash_escapes && current == '\\' {
            index += 2;
        } else if current == quote {
            if chars.get(index + 1) == Some(&quote) {
                index += 2;
            } else {
                return index + 1;
            }
        } else {
            index += 1;
        }
    }
    index
}

fn skip_dollar_quote(chars: &[char], start: usize) -> usize {
    let mut tag_end = start + 1;
    while tag_end < chars.len() && (chars[tag_end].is_alphanumeric() || chars[tag_end] == '_') {
        tag_end += 1;
    }
    let starts_like_tag = chars
        .get(start + 1)
        .is_some_and(|first| !first.is_ascii_digit());
    if !starts_like_tag || chars.get(tag_end) != Some(&'$') {
        return start + 1;
    }
    let delimiter = &chars[start..=tag_end];
    let mut index = tag_end + 1;
    while index + delimiter.len() <= chars.len() {
        if &chars[index..index + delimiter.len()] == delimiter {
            return index + delimiter.len();
        }
        index += 1;
    }
    chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keywords(sql: &str) -> Vec<String> {
        scan_statement(sql).keywords
    }

    #[test]
    fn collects_top_level_words_only() {
        assert_eq!(
            keywords("select a from (select b from t) x"),
            vec!["SELECT", "A", "FROM", "X"]
        );
    }

    #[test]
    fn skips_comments_strings_and_quoted_identifiers() {
        let sql = "-- delete\n/* update /* nested */ */ insert into \"drop\" values ('select', E'it\\'s')";
        assert_eq!(keywords(sql), vec!["INSERT", "INTO", "VALUES"]);
    }

    #[test]
    fn skips_dollar_quoted_bodies() {
        let sql = "create function f() returns int as $body$ select 1; $body$ language sql";
        let scan = scan_statement(sql);
        assert_eq!(
            scan.keywords,
            vec![
                "CREATE", "FUNCTION", "F", "RETURNS", "INT", "AS", "LANGUAGE", "SQL"
            ]
        );
        assert!(!scan.has_multiple_statements);
    }

    #[test]
    fn positional_parameters_are_not_dollar_quotes() {
        assert_eq!(keywords("select $1, $2"), vec!["SELECT"]);
    }

    #[test]
    fn detects_multiple_statements() {
        assert!(!scan_statement("select 1;  -- trailing\n").has_multiple_statements);
        assert!(!scan_statement("select ';'").has_multiple_statements);
        assert!(scan_statement("select 1; select 2").has_multiple_statements);
    }
}
