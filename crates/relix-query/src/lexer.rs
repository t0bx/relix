/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::vocabulary::is_keyword;
use relix_core::Dialect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Whitespace,
    LineComment,
    BlockComment,
    String,
    QuotedIdentifier,
    Identifier,
    Keyword,
    Number,
    Parameter,
    Operator,
    Punctuation,
    Unterminated,
    Unknown,
}

impl TokenKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }

    pub fn is_word(self) -> bool {
        matches!(self, Self::Identifier | Self::Keyword)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
    pub start: usize,
    pub end: usize,
}

impl Token<'_> {
    pub fn is_keyword(&self, keyword: &str) -> bool {
        self.kind.is_word() && self.text.eq_ignore_ascii_case(keyword)
    }

    pub fn is_symbol(&self, symbol: &str) -> bool {
        matches!(self.kind, TokenKind::Operator | TokenKind::Punctuation) && self.text == symbol
    }
}

pub fn tokenize(source: &str, dialect: Dialect) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        source,
        bytes: source.as_bytes(),
        position: 0,
        dialect,
    };
    let mut tokens = Vec::new();
    while lexer.position < source.len() {
        let start = lexer.position;
        let kind = lexer.next_kind();
        if lexer.position <= start {
            lexer.position = next_char_boundary(source, start);
        }
        tokens.push(Token {
            kind,
            text: &source[start..lexer.position],
            start,
            end: lexer.position,
        });
    }
    tokens
}

pub fn significant<'s, 'a>(tokens: &'s [Token<'a>]) -> impl Iterator<Item = Token<'a>> + 's {
    tokens
        .iter()
        .copied()
        .filter(|token| !token.kind.is_trivia())
}

fn next_char_boundary(source: &str, index: usize) -> usize {
    source[index..]
        .chars()
        .next()
        .map_or(source.len(), |ch| index + ch.len_utf8())
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    position: usize,
    dialect: Dialect,
}

impl Lexer<'_> {
    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.position + offset).copied()
    }

    fn next_kind(&mut self) -> TokenKind {
        let Some(current) = self.peek(0) else {
            return TokenKind::Unknown;
        };
        match current {
            byte if byte.is_ascii_whitespace() => self.whitespace(),
            b'-' if self.peek(1) == Some(b'-') => self.line_comment(),
            b'#' if self.dialect == Dialect::MySql => self.line_comment(),
            b'/' if self.peek(1) == Some(b'*') => self.block_comment(),
            b'\'' => self.quoted(b'\'', TokenKind::String, self.dialect == Dialect::MySql),
            b'"' if self.dialect == Dialect::MySql => self.quoted(b'"', TokenKind::String, true),
            b'"' => self.quoted(b'"', TokenKind::QuotedIdentifier, false),
            b'`' => self.quoted(b'`', TokenKind::QuotedIdentifier, false),
            b'[' if self.dialect == Dialect::Sqlite => self.bracket_identifier(),
            b'e' | b'E' if self.dialect == Dialect::Postgres && self.peek(1) == Some(b'\'') => {
                self.position += 1;
                self.quoted(b'\'', TokenKind::String, true)
            }
            b'x' | b'X' | b'b' | b'B' if self.peek(1) == Some(b'\'') => {
                self.position += 1;
                self.quoted(b'\'', TokenKind::String, false)
            }
            b'$' => self.dollar(),
            b'0'..=b'9' => self.number(),
            b'.' if self.peek(1).is_some_and(|byte| byte.is_ascii_digit()) => self.number(),
            b'?' => {
                self.position += 1;
                while self.peek(0).is_some_and(|byte| byte.is_ascii_digit()) {
                    self.position += 1;
                }
                TokenKind::Parameter
            }
            b':' if self.peek(1).is_some_and(is_word_start) => {
                self.position += 1;
                self.consume_word();
                TokenKind::Parameter
            }
            b'@' => {
                while self.peek(0) == Some(b'@') {
                    self.position += 1;
                }
                self.consume_word();
                TokenKind::Parameter
            }
            b'(' | b')' | b',' | b';' | b'.' | b'[' | b']' | b'{' | b'}' => {
                self.position += 1;
                TokenKind::Punctuation
            }
            byte if is_operator_byte(byte) => {
                while self.peek(0).is_some_and(is_operator_byte) {
                    if self.peek(0) == Some(b'-') && self.peek(1) == Some(b'-') {
                        break;
                    }
                    if self.peek(0) == Some(b'/') && self.peek(1) == Some(b'*') {
                        break;
                    }
                    self.position += 1;
                }
                TokenKind::Operator
            }
            byte if is_word_start(byte) => self.word(),
            _ => {
                self.position = next_char_boundary(self.source, self.position);
                TokenKind::Unknown
            }
        }
    }

    fn whitespace(&mut self) -> TokenKind {
        while self.peek(0).is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
        TokenKind::Whitespace
    }

    fn line_comment(&mut self) -> TokenKind {
        while self.peek(0).is_some_and(|byte| byte != b'\n') {
            self.position += 1;
        }
        TokenKind::LineComment
    }

    /// Consumes a `/* ... */` comment. PostgreSQL allows nesting, other dialects end the
    /// comment at the first closing marker.
    fn block_comment(&mut self) -> TokenKind {
        let nested = self.dialect == Dialect::Postgres;
        let mut depth = 0usize;
        while let Some(byte) = self.peek(0) {
            if byte == b'/' && self.peek(1) == Some(b'*') {
                if depth == 0 || nested {
                    depth += 1;
                }
                self.position += 2;
            } else if byte == b'*' && self.peek(1) == Some(b'/') {
                self.position += 2;
                depth -= 1;
                if depth == 0 {
                    return TokenKind::BlockComment;
                }
            } else {
                self.position += 1;
            }
        }
        TokenKind::Unterminated
    }

    /// Consumes a quoted literal starting at the current quote character. Doubled quotes
    /// are an escape everywhere; backslash escapes only where `backslash_escapes` is set.
    fn quoted(&mut self, quote: u8, kind: TokenKind, backslash_escapes: bool) -> TokenKind {
        self.position += 1;
        while let Some(byte) = self.peek(0) {
            if backslash_escapes && byte == b'\\' {
                self.position = (self.position + 2).min(self.bytes.len());
                continue;
            }
            self.position += 1;
            if byte == quote {
                if self.peek(0) == Some(quote) {
                    self.position += 1;
                    continue;
                }
                return kind;
            }
        }
        TokenKind::Unterminated
    }

    fn bracket_identifier(&mut self) -> TokenKind {
        while let Some(byte) = self.peek(0) {
            self.position += 1;
            if byte == b']' {
                return TokenKind::QuotedIdentifier;
            }
        }
        TokenKind::Unterminated
    }

    /// Handles `$1` positional parameters and PostgreSQL `$tag$ ... $tag$` dollar quoting.
    fn dollar(&mut self) -> TokenKind {
        if self.peek(1).is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
            while self.peek(0).is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            return TokenKind::Parameter;
        }
        if self.dialect != Dialect::Postgres {
            self.position += 1;
            return TokenKind::Operator;
        }
        let tag_end = self.source[self.position + 1..]
            .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
            .map(|offset| self.position + 1 + offset);
        let Some(tag_end) = tag_end.filter(|end| self.bytes.get(*end) == Some(&b'$')) else {
            self.position += 1;
            return TokenKind::Operator;
        };
        let delimiter = &self.source[self.position..=tag_end];
        let body_start = tag_end + 1;
        match self.source[body_start..].find(delimiter) {
            Some(offset) => {
                self.position = body_start + offset + delimiter.len();
                TokenKind::String
            }
            None => {
                self.position = self.source.len();
                TokenKind::Unterminated
            }
        }
    }

    fn number(&mut self) -> TokenKind {
        if self.peek(0) == Some(b'0') && matches!(self.peek(1), Some(b'x' | b'X')) {
            self.position += 2;
            while self.peek(0).is_some_and(|byte| byte.is_ascii_hexdigit()) {
                self.position += 1;
            }
            return TokenKind::Number;
        }
        while self
            .peek(0)
            .is_some_and(|byte| byte.is_ascii_digit() || byte == b'.' || byte == b'_')
        {
            self.position += 1;
        }
        if matches!(self.peek(0), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(self.peek(1), Some(b'+' | b'-')));
            if self
                .peek(1 + sign)
                .is_some_and(|byte| byte.is_ascii_digit())
            {
                self.position += 1 + sign;
                while self.peek(0).is_some_and(|byte| byte.is_ascii_digit()) {
                    self.position += 1;
                }
            }
        }
        TokenKind::Number
    }

    fn word(&mut self) -> TokenKind {
        let start = self.position;
        self.consume_word();
        if is_keyword(self.dialect, &self.source[start..self.position]) {
            TokenKind::Keyword
        } else {
            TokenKind::Identifier
        }
    }

    fn consume_word(&mut self) {
        let start = self.position;
        while let Some(ch) = self.source[self.position..].chars().next() {
            let allowed = ch.is_alphanumeric() || ch == '_' || (ch == '$' && self.position > start);
            if !allowed {
                break;
            }
            self.position += ch.len_utf8();
        }
    }
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_operator_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'+' | b'-'
            | b'*'
            | b'/'
            | b'<'
            | b'>'
            | b'='
            | b'~'
            | b'!'
            | b'%'
            | b'^'
            | b'&'
            | b'|'
            | b':'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str, dialect: Dialect) -> Vec<(TokenKind, &str)> {
        tokenize(source, dialect)
            .into_iter()
            .filter(|token| token.kind != TokenKind::Whitespace)
            .map(|token| (token.kind, token.text))
            .collect()
    }

    #[test]
    fn tokenizes_basic_select() {
        assert_eq!(
            kinds("SELECT id, 'it''s' FROM t -- c", Dialect::Postgres),
            vec![
                (TokenKind::Keyword, "SELECT"),
                (TokenKind::Identifier, "id"),
                (TokenKind::Punctuation, ","),
                (TokenKind::String, "'it''s'"),
                (TokenKind::Keyword, "FROM"),
                (TokenKind::Identifier, "t"),
                (TokenKind::LineComment, "-- c"),
            ]
        );
    }

    #[test]
    fn postgres_dollar_quotes_and_casts() {
        let tokens = kinds("SELECT $fn$ a; b $fn$, x::int, $1", Dialect::Postgres);
        assert_eq!(tokens[1], (TokenKind::String, "$fn$ a; b $fn$"));
        assert!(tokens.contains(&(TokenKind::Operator, "::")));
        assert_eq!(tokens.last(), Some(&(TokenKind::Parameter, "$1")));
    }

    #[test]
    fn mysql_specific_quoting_and_comments() {
        let tokens = kinds("SELECT `a``b`, \"str\\\"\" # note", Dialect::MySql);
        assert_eq!(tokens[1], (TokenKind::QuotedIdentifier, "`a``b`"));
        assert_eq!(tokens[3], (TokenKind::String, "\"str\\\"\""));
        assert_eq!(tokens[4], (TokenKind::LineComment, "# note"));
    }

    #[test]
    fn nested_comments_only_in_postgres() {
        let source = "/* a /* b */ c */ x";
        assert_eq!(kinds(source, Dialect::Postgres)[0].1, "/* a /* b */ c */");
        assert_eq!(kinds(source, Dialect::Sqlite)[0].1, "/* a /* b */");
    }

    #[test]
    fn unterminated_literals_are_flagged() {
        assert_eq!(
            kinds("'open", Dialect::Generic)[0].0,
            TokenKind::Unterminated
        );
        assert_eq!(
            kinds("/* open", Dialect::Generic)[0].0,
            TokenKind::Unterminated
        );
    }

    #[test]
    fn handles_unicode_and_numbers() {
        let tokens = kinds("größe 1.5e-3 0xff .5", Dialect::Generic);
        assert_eq!(tokens[0], (TokenKind::Identifier, "größe"));
        assert_eq!(tokens[1], (TokenKind::Number, "1.5e-3"));
        assert_eq!(tokens[2], (TokenKind::Number, "0xff"));
        assert_eq!(tokens[3], (TokenKind::Number, ".5"));
    }

    #[test]
    fn spans_cover_the_whole_input() {
        let source = "select 'ä' ; ¤";
        let tokens = tokenize(source, Dialect::Generic);
        let rebuilt: String = tokens.iter().map(|token| token.text).collect();
        assert_eq!(rebuilt, source);
    }
}
