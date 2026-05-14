//! Crockford Base32, lowercase variant.
//!
//! Used to encode the random per-day suffix inside the PreXiv
//! identifier `prexiv:YYMMDD.SUFFIX`. The alphabet is
//! `0-9` then `a-z` minus `i`, `l`, `o`, `u` — 32 characters, chosen
//! to be unambiguous at glance (no confusion between `1` and `l`,
//! `0` and `O`, etc.) and to lexsort the same as the integer values
//! they encode. The latter is what gives PreXiv ids their "compare
//! chronologically by string alone" property.
//!
//! 6 characters of Crockford-32 cover `32^6 = 1 073 741 824` values
//! — about a billion per day. Even an agent-swarm submission rate of
//! ~10 000 per second couldn't exhaust it in 24 hours.

pub const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Encode `n` as a left-padded, lowercase Crockford base-32 string of
/// exactly `width` characters. Wraps modulo 2^(5*width) when `n` would
/// overflow — width=6 supports up to 2^30, which is more than the per-
/// day capacity we ever expect to hit.
pub fn encode(mut n: u64, width: usize) -> String {
    let mut buf = vec![b'0'; width];
    for i in (0..width).rev() {
        buf[i] = ALPHABET[(n & 31) as usize];
        n >>= 5;
    }
    // SAFETY: every byte is from ALPHABET (ASCII).
    String::from_utf8(buf).expect("ALPHABET is ASCII")
}

/// Decode an ASCII Crockford-32 string (lowercase) back to its integer
/// value. Returns `None` on any non-alphabet character. Lengths up to
/// 12 chars (=60 bits) fit in a u64 without truncation.
#[allow(dead_code)]
pub fn decode(s: &str) -> Option<u64> {
    let mut acc: u64 = 0;
    for c in s.bytes() {
        let v: u64 = match c {
            b'0'..=b'9' => (c - b'0') as u64,
            b'a'..=b'h' => (c - b'a' + 10) as u64,
            b'j' => 18,
            b'k' => 19,
            b'm' => 20,
            b'n' => 21,
            b'p' => 22,
            b'q' => 23,
            b'r' => 24,
            b's' => 25,
            b't' => 26,
            b'v' => 27,
            b'w' => 28,
            b'x' => 29,
            b'y' => 30,
            b'z' => 31,
            _ => return None,
        };
        // Guard against overflow — we never expect a value > 2^30.
        acc = acc.checked_mul(32)?.checked_add(v)?;
    }
    Some(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_pads_left() {
        assert_eq!(encode(0, 6), "000000");
        assert_eq!(encode(1, 6), "000001");
        assert_eq!(encode(31, 6), "00000z");
        assert_eq!(encode(32, 6), "000010");
    }

    #[test]
    fn roundtrip_random_values() {
        for n in [0u64, 1, 31, 32, 1023, 65535, 1_000_000, 1_073_741_823] {
            let s = encode(n, 6);
            assert_eq!(decode(&s), Some(n), "roundtrip failed for {n}");
        }
    }

    #[test]
    fn decode_rejects_forbidden_letters() {
        // Crockford excludes i, l, o, u.
        for c in ['i', 'l', 'o', 'u', 'I', 'L', 'O', 'U', '?', '-'] {
            let s: String = std::iter::once(c).collect();
            assert!(decode(&s).is_none(), "should reject {c}");
        }
    }

    #[test]
    fn ordering_preserved() {
        // Lex order on encoded strings == integer order. This is the
        // load-bearing property for chronological-by-id PreXiv ids.
        let mut pairs: Vec<(u64, String)> = (0..2000).map(|n| (n, encode(n, 6))).collect();
        pairs.sort_by(|a, b| a.1.cmp(&b.1));
        for (i, (n, _)) in pairs.iter().enumerate() {
            assert_eq!(*n as usize, i);
        }
    }
}
