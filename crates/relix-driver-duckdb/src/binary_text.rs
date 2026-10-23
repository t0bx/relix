/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::fmt::Write;

const BIGNUM_HEADER_BYTES: usize = 3;
const DECIMAL_CHUNK: u32 = 1_000_000_000;

/// Writes a blob like DuckDB's `BLOB -> VARCHAR` cast: printable ASCII stays, quotes,
/// backslashes and every other byte become `\xHH`.
pub(crate) fn blob_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for &byte in bytes {
        let regular = (32..=126).contains(&byte) && !matches!(byte, b'\\' | b'\'' | b'"');
        if regular {
            text.push(char::from(byte));
        } else {
            let _ = write!(text, "\\x{byte:02X}");
        }
    }
    text
}

/// Decodes DuckDB's `BIT` storage: the first byte counts the padding bits that precede
/// the value in the second byte.
pub(crate) fn bit_string(bytes: &[u8]) -> String {
    let Some((&padding, data)) = bytes.split_first() else {
        return String::new();
    };
    data.iter()
        .flat_map(|byte| (0..8).rev().map(move |bit| (byte >> bit) & 1))
        .skip(usize::from(padding))
        .map(|bit| if bit == 1 { '1' } else { '0' })
        .collect()
}

/// Decodes DuckDB's `BIGNUM` storage: a three byte header whose top bit marks positive
/// numbers, followed by the big-endian magnitude. Negative numbers store every byte
/// (header and magnitude) bitwise inverted.
pub(crate) fn bignum_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() < BIGNUM_HEADER_BYTES {
        return None;
    }
    let negative = bytes[0] & 0x80 == 0;
    let magnitude: Vec<u8> = bytes[BIGNUM_HEADER_BYTES..]
        .iter()
        .map(|&byte| if negative { !byte } else { byte })
        .collect();
    let digits = decimal_digits(magnitude);
    if negative && digits != "0" {
        Some(format!("-{digits}"))
    } else {
        Some(digits)
    }
}

fn decimal_digits(mut magnitude: Vec<u8>) -> String {
    let mut chunks = Vec::new();
    while magnitude.iter().any(|&byte| byte != 0) {
        let mut remainder: u64 = 0;
        for byte in &mut magnitude {
            let current = (remainder << 8) | u64::from(*byte);
            *byte = u8::try_from(current / u64::from(DECIMAL_CHUNK)).unwrap_or(u8::MAX);
            remainder = current % u64::from(DECIMAL_CHUNK);
        }
        chunks.push(remainder);
    }
    let Some((most_significant, rest)) = chunks.split_last() else {
        return "0".to_owned();
    };
    let mut text = most_significant.to_string();
    for chunk in rest.iter().rev() {
        let _ = write!(text, "{chunk:09}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_escape_non_printable_bytes() {
        assert_eq!(blob_text(&[0xaa, 0xbb, b' ', b'a', 0]), "\\xAA\\xBB a\\x00");
        assert_eq!(blob_text(b"it's"), "it\\x27s");
    }

    #[test]
    fn bits_skip_their_padding() {
        assert_eq!(bit_string(&[5, 253]), "101");
        assert_eq!(bit_string(&[3, 246]), "10110");
        assert_eq!(bit_string(&[7, 255]), "1");
    }

    #[test]
    fn bignums_decode_sign_and_magnitude() {
        assert_eq!(bignum_text(&[128, 0, 1, 123]).as_deref(), Some("123"));
        assert_eq!(bignum_text(&[127, 255, 254, 250]).as_deref(), Some("-5"));
        assert_eq!(bignum_text(&[128, 0, 1, 0]).as_deref(), Some("0"));
        let large = [127, 255, 245, 253, 98, 189, 73, 177, 137, 142, 189, 187, 52];
        assert_eq!(
            bignum_text(&large).as_deref(),
            Some("-12345678901234567890123")
        );
    }
}
