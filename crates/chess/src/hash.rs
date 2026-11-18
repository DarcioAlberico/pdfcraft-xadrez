//! Small hashes (plan A9-I6, finding A7-19): the one FNV-1a the identifiers made with it are
//! written with — an EPUB's identifier and a part's, a document's name in glyph samples, a long
//! `/ID` folded to the 16 bytes the stores are keyed by — the same from one build to the next. Not
//! a security hash.

/// The 64-bit FNV-1a hash of `bytes`.
pub fn fnv64(bytes: &[u8]) -> u64 {
    fnv64_of(bytes.iter().copied())
}

/// The 64-bit FNV-1a hash of the bytes `bytes` gives, in their order (a document's identifier
/// folded from both ends, a list of pages as bytes).
pub fn fnv64_of(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published FNV-1a test vectors: the identifiers made with it before stay as they were.
    #[test]
    fn fnv64_is_fnv_1a() {
        assert_eq!(fnv64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv64(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
