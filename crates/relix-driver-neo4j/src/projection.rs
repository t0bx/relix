/*
 * @author t0bx
 * @since 24.10.2026
 */

const PROJECTION_KEYWORDS: [&str; 2] = ["RETURN", "YIELD"];
const PROJECTION_END_KEYWORDS: [&str; 7] = [
    "ORDER", "SKIP", "OFFSET", "LIMIT", "UNION", "WHERE", "RETURN",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    text: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Default)]
struct Scan {
    words: Vec<Word>,
    commas: Vec<usize>,
}

/// Returns the column names of the statement's final projection: the last top-level
/// `RETURN`, or `YIELD` for procedure calls and `SHOW` commands. neo4rs does not expose
/// the field list of a result, so this is how result columns keep the order the user
/// wrote. Aliases (`AS name`) are used as written; other items keep their expression
/// text, which is how Neo4j names such columns. `RETURN *` yields `None`.
pub(crate) fn projection_columns(statement: &str) -> Option<Vec<String>> {
    let scan = scan_top_level(statement);
    let keyword = scan.words.iter().rposition(|word| {
        PROJECTION_KEYWORDS
            .iter()
            .any(|key| word.text.eq_ignore_ascii_case(key))
    })?;
    let start = scan.words[keyword].end;
    let end = scan.words[keyword + 1..]
        .iter()
        .find(|word| {
            PROJECTION_END_KEYWORDS
                .iter()
                .any(|key| word.text.eq_ignore_ascii_case(key))
        })
        .map_or(statement.len(), |word| word.start);
    let mut items = Vec::new();
    let mut item_start = start;
    for comma in scan
        .commas
        .iter()
        .filter(|comma| **comma > start && **comma < end)
    {
        items.push((item_start, *comma));
        item_start = comma + 1;
    }
    items.push((item_start, end));
    let mut columns = Vec::with_capacity(items.len());
    for (index, (from, to)) in items.into_iter().enumerate() {
        let mut text = statement[from..to].trim().trim_end_matches(';').trim();
        if index == 0
            && let Some(rest) = strip_keyword(text, "DISTINCT")
        {
            text = rest;
        }
        if text.is_empty() || text == "*" {
            return None;
        }
        columns.push(column_name(statement, &scan, from, to, text));
    }
    Some(columns)
}

/// Orders result keys by their position in the final projection; keys that cannot be
/// matched (procedure output without `YIELD`, `RETURN *`) follow alphabetically.
pub(crate) fn order_keys(mut keys: Vec<String>, statement: &str) -> Vec<String> {
    let projection = projection_columns(statement).unwrap_or_default();
    keys.sort_by(|left, right| {
        let rank = |key: &String| projection.iter().position(|column| column == key);
        match (rank(left), rank(right)) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left.cmp(right),
        }
    });
    keys
}

fn column_name(statement: &str, scan: &Scan, from: usize, to: usize, text: &str) -> String {
    let alias = scan.words.iter().rposition(|word| {
        word.start >= from && word.end <= to && word.text.eq_ignore_ascii_case("AS")
    });
    match alias {
        Some(index) => unquote(
            statement[scan.words[index].end..to]
                .trim()
                .trim_end_matches(';')
                .trim(),
        ),
        None => text.to_owned(),
    }
}

fn unquote(identifier: &str) -> String {
    identifier
        .strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .map(|inner| inner.replace("``", "`"))
        .unwrap_or_else(|| identifier.to_owned())
}

fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let head = text.get(..keyword.len())?;
    let rest = text.get(keyword.len()..)?;
    (head.eq_ignore_ascii_case(keyword) && rest.starts_with(char::is_whitespace))
        .then(|| rest.trim_start())
}

/// Collects bare words and commas outside of strings, quoted identifiers, comments and
/// brackets, recording their byte offsets.
fn scan_top_level(statement: &str) -> Scan {
    let bytes = statement.as_bytes();
    let mut scan = Scan::default();
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'\'' | b'"' => index = skip_string(bytes, index, byte),
            b'`' => index = skip_until(bytes, index + 1, b'`'),
            b'/' if bytes.get(index + 1) == Some(&b'/') => index = skip_until(bytes, index, b'\n'),
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = statement[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| index + 2 + end + 2);
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                index += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            b',' => {
                if depth == 0 {
                    scan.commas.push(index);
                }
                index += 1;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric()
                        || bytes[index] == b'_'
                        || bytes[index] == b'.')
                {
                    index += 1;
                }
                let preceded_by_dot = start > 0 && bytes[start - 1] == b'.';
                if depth == 0 && !preceded_by_dot {
                    scan.words.push(Word {
                        text: statement[start..index].to_owned(),
                        start,
                        end: index,
                    });
                }
            }
            _ => index += 1,
        }
    }
    scan
}

fn skip_string(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn skip_until(bytes: &[u8], start: usize, end: u8) -> usize {
    let mut index = start;
    while index < bytes.len() && bytes[index] != end {
        index += 1;
    }
    index + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(statement: &str) -> Option<Vec<String>> {
        projection_columns(statement)
    }

    #[test]
    fn aliases_and_expressions_name_the_columns() {
        assert_eq!(
            columns("MATCH (n:Person) RETURN n.name AS name, count(*), n ORDER BY name LIMIT 5"),
            Some(vec!["name".into(), "count(*)".into(), "n".into()])
        );
        assert_eq!(
            columns("match (n) return distinct n.`first name` as `first name`;"),
            Some(vec!["first name".into()])
        );
    }

    #[test]
    fn nested_returns_and_strings_are_ignored() {
        let statement = "MATCH (p) CALL { WITH p RETURN 'RETURN x, y' AS inner } \
                         RETURN p.name, [1, 2] AS list, {a: 1, b: 2} AS map";
        assert_eq!(
            columns(statement),
            Some(vec!["p.name".into(), "list".into(), "map".into()])
        );
    }

    #[test]
    fn yield_is_used_for_procedures_and_show_commands() {
        assert_eq!(
            columns("SHOW DATABASES YIELD name, currentStatus WHERE name <> 'system'"),
            Some(vec!["name".into(), "currentStatus".into()])
        );
        assert_eq!(
            columns("CALL db.labels() YIELD label RETURN label AS l, 1 AS one"),
            Some(vec!["l".into(), "one".into()])
        );
        assert_eq!(columns("CREATE (n:Thing)"), None);
        assert_eq!(columns("MATCH (n) RETURN *"), None);
    }

    #[test]
    fn keys_follow_the_projection_then_the_alphabet() {
        let keys = vec![
            "b".to_owned(),
            "zeta".to_owned(),
            "a".to_owned(),
            "c".to_owned(),
        ];
        assert_eq!(
            order_keys(keys, "MATCH (x) RETURN zeta, b"),
            ["zeta", "b", "a", "c"]
        );
    }
}
