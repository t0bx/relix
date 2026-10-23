/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::fmt::Write;

/// Encodes like Java's `URLEncoder` (form encoding), which Trino uses for session
/// property values and prepared statements in headers.
pub(crate) fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            other => {
                let _ = write!(encoded, "%{other:02X}");
            }
        }
    }
    encoded
}

pub(crate) fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = match bytes[index..] {
            [b'%', high, low, ..] => hex_value(high).zip(hex_value(low)),
            _ => None,
        };
        match (bytes[index], escaped) {
            (_, Some((high, low))) => {
                decoded.push(high << 4 | low);
                index += 3;
            }
            (b'+', None) => {
                decoded.push(b' ');
                index += 1;
            }
            (other, None) => {
                decoded.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_through_form_encoding() {
        let original = "SELECT * FROM t WHERE a = 'ä,b=c'";
        let encoded = encode(original);
        assert!(!encoded.contains([',', '=', ' ', '\'']));
        assert_eq!(decode(&encoded), original);
        assert_eq!(decode("a+b%2Cc"), "a b,c");
        assert_eq!(decode("100%"), "100%");
    }
}
