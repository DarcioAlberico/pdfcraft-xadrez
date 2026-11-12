//! Chess ▸ Game collections: what names a document file across sessions, so the chess games
//! played on its diagrams are found again when it is reopened (PGN toolbar spec §4.7, T4b).
//!
//! A document is named three ways ([`DocKeys::matches`]): its first file identifier (`/ID[0]`),
//! the path it was opened from or saved to, and its file name together with its size. The
//! identifier survives renaming and moving; the path survives a file with no identifier; the name
//! and size survive a copy opened from somewhere else (the web build has no paths). When both
//! sides have an identifier it decides alone: two different identifiers are two documents, whatever
//! their paths or names say (plan A9, G1). Saving can create the identifier (the writer adds one
//! to a file without it) and changes the size, so whoever keeps games under these keys takes them
//! again after every save.
//!
//! **The identifier every new file used to get.** Before plan A9 (G1), every document PdfCraft
//! made from nothing (extract, split, combine) was saved with one identifier,
//! [`DocKeys::KNOWN_CONSTANT_ID`]. It names no document: it counts as no identifier at all, so
//! those files are named by their path, or their name and size, like a file without one.

use pdfcraft_cos::Object;

use crate::Document;

/// Longest file identifier read; a longer one is not a valid identifier and is ignored.
const MAX_ID_LEN: usize = 256;

/// What names a document file across sessions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocKeys {
    /// The first file identifier (`/ID[0]`). The writer makes 16-byte identifiers; one of another
    /// length is folded to 16 bytes ([`fold_id`]).
    pub id0: Option<[u8; 16]>,
    /// Where the document was opened from or last saved to.
    pub path: Option<String>,
    /// The file name shown for the document.
    pub name: Option<String>,
    /// The size in bytes of the file as last opened or saved.
    pub size: Option<u64>,
}

impl DocKeys {
    /// The identifier PdfCraft gave every document it made from nothing (extract, split, combine)
    /// before plan A9 (G1): `9BA0D5AAD0617164A1D073B0D8C7DCD9`, a hash of no bytes. Files that
    /// have it are many different documents, so it counts as no identifier ([`DocKeys::id`]).
    pub const KNOWN_CONSTANT_ID: [u8; 16] = [0x9B, 0xA0, 0xD5, 0xAA, 0xD0, 0x61, 0x71, 0x64, 0xA1, 0xD0, 0x73, 0xB0, 0xD8, 0xC7, 0xDC, 0xD9];

    /// The identifier when it names this document: `id0`, unless it is
    /// [`DocKeys::KNOWN_CONSTANT_ID`].
    pub fn id(&self) -> Option<[u8; 16]> {
        self.id0.filter(|id| *id != DocKeys::KNOWN_CONSTANT_ID)
    }

    /// Whether `self` and `other` name the same document. When both have an identifier
    /// ([`DocKeys::id`]), that alone decides: two different identifiers are two documents even
    /// with the same path, name and size (another book saved over the file, a part split from
    /// it). Otherwise the same path, or the same file name and size. Keys that are missing on
    /// either side never match. The one rule for every chess store (games, corrections, the
    /// book's PGN).
    pub fn matches(&self, other: &DocKeys) -> bool {
        fn same<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> bool {
            a.is_some() && a == b
        }
        if let (Some(a), Some(b)) = (self.id(), other.id()) {
            return a == b;
        }
        same(&self.path, &other.path) || (same(&self.name, &other.name) && same(&self.size, &other.size))
    }

    /// Whether no key is known (nothing can match).
    pub fn is_empty(&self) -> bool {
        self.id().is_none() && self.path.is_none() && (self.name.is_none() || self.size.is_none())
    }
}

/// A file identifier as 16 bytes: as it is when it has 16, else folded with two FNV-1a passes
/// (one from each end). `None` for an empty or oversized identifier.
pub fn fold_id(bytes: &[u8]) -> Option<[u8; 16]> {
    if bytes.is_empty() || bytes.len() > MAX_ID_LEN {
        return None;
    }
    if let Ok(id) = <[u8; 16]>::try_from(bytes) {
        return Some(id);
    }
    let fnv = |it: &mut dyn Iterator<Item = &u8>| it.fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    let mut out = [0u8; 16];
    let (front, back) = out.split_at_mut(8);
    front.copy_from_slice(&fnv(&mut bytes.iter()).to_be_bytes());
    back.copy_from_slice(&fnv(&mut bytes.iter().rev()).to_be_bytes());
    Some(out)
}

/// An identifier written as 32 hexadecimal digits (as the stores record it), read back. `None`
/// for anything else.
pub fn id_from_hex(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut id = [0u8; 16];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(id)
}

impl Document {
    /// The keys that name this document's file now ([`DocKeys`]). The identifier is read from the
    /// document's trailer; a document that cannot be edited yet (encrypted and not unlocked) has
    /// no trailer at hand, and is named by its path, name and size alone.
    pub fn doc_keys(&self) -> DocKeys {
        let id0 = self.editor.as_ref().and_then(|e| match e.cos.trailer().get(b"ID") {
            Some(Object::Array(ids)) => ids.first().and_then(Object::as_string).and_then(|s| fold_id(&s.bytes)),
            _ => None,
        });
        let size = u64::try_from(self.saved_bytes().len()).ok();
        DocKeys { id0, path: self.path.clone(), name: Some(self.name.clone()).filter(|n| !n.is_empty()), size }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::Session;

    #[test]
    fn any_key_matches_and_missing_keys_never_do() {
        let a = DocKeys { id0: Some([1; 16]), path: Some("/a.pdf".into()), name: Some("a.pdf".into()), size: Some(10) };
        assert!(a.matches(&DocKeys { id0: Some([1; 16]), ..DocKeys::default() }));
        assert!(a.matches(&DocKeys { path: Some("/a.pdf".into()), ..DocKeys::default() }));
        assert!(a.matches(&DocKeys { name: Some("a.pdf".into()), size: Some(10), ..DocKeys::default() }));
        // A name or a size alone is not enough: many files share one.
        assert!(!a.matches(&DocKeys { name: Some("a.pdf".into()), size: Some(11), ..DocKeys::default() }));
        assert!(!a.matches(&DocKeys { size: Some(10), ..DocKeys::default() }));
        assert!(!DocKeys::default().matches(&DocKeys::default()));
        assert!(DocKeys::default().is_empty());
        assert!(!a.is_empty());
    }

    /// Two different identifiers are two documents whatever else they share (plan A9, G1): a part
    /// split from a book, or another book saved over its file, is not the book. The identifier
    /// every new file used to get names nothing.
    #[test]
    fn an_identifier_on_both_sides_decides_alone_and_the_known_constant_is_none() {
        let a = DocKeys { id0: Some([1; 16]), path: Some("/a.pdf".into()), name: Some("a.pdf".into()), size: Some(10) };
        assert!(!a.matches(&DocKeys { id0: Some([2; 16]), ..a.clone() }), "same path, name and size, another identifier");
        assert!(a.matches(&DocKeys { id0: None, ..a.clone() }), "no identifier on one side: the path");
        assert!(DocKeys { id0: None, path: None, ..a.clone() }.matches(&DocKeys { id0: None, path: None, ..a.clone() }));
        let constant = DocKeys { id0: Some(DocKeys::KNOWN_CONSTANT_ID), ..DocKeys::default() };
        assert_eq!(constant.id(), None);
        assert!(constant.is_empty());
        assert!(!constant.matches(&constant), "the constant names no document");
        // Files that have it are named by their path, or their name and size.
        let at = |path: &str| DocKeys { path: Some(path.into()), ..constant.clone() };
        assert!(at("/a.pdf").matches(&a), "by the path");
        assert!(!at("/cap1.pdf").matches(&at("/cap2.pdf")));
        assert!(at("/cap1.pdf").matches(&at("/cap1.pdf")));
        assert_eq!(id_from_hex(&"9ba0d5aad0617164a1d073b0d8c7dcd9".to_uppercase()), Some(DocKeys::KNOWN_CONSTANT_ID));
        assert_eq!(id_from_hex("9ba0"), None);
        assert_eq!(id_from_hex(&"é".repeat(16)), None);
    }

    /// Every part extracted used to be saved with one identifier ([`DocKeys::KNOWN_CONSTANT_ID`]),
    /// so the chess stores took every part for one book (plan A9, A4-03). Different pages give
    /// different identifiers; the same pages at the same moment, the same one.
    #[test]
    fn extracted_parts_are_different_documents() {
        let mut s = Session::new().with_clock(|| 1_700_000_000);
        let id = s.open("book.pdf", Some("/books/book.pdf".into()), Arc::new(crate::tests::fixture(3)), None).unwrap();
        let mut keys = |pages: &[usize], name: &str| {
            let bytes = s.extract(id, pages).unwrap();
            let part = s.open(name, None, bytes, None).unwrap();
            s.get(part).unwrap().doc_keys()
        };
        let (one, two, again) = (keys(&[0], "one.pdf"), keys(&[1, 2], "two.pdf"), keys(&[0], "again.pdf"));
        assert!(one.id().is_some() && two.id().is_some(), "{one:?} {two:?}");
        assert_ne!(one.id0, two.id0);
        assert_eq!(one.id0, again.id0, "the same pages, the same moment");
        assert!(!one.matches(&two));
    }

    #[test]
    fn identifiers_fold_to_sixteen_bytes() {
        assert_eq!(fold_id(&[7; 16]), Some([7; 16]));
        assert_eq!(fold_id(&[]), None);
        assert_eq!(fold_id(&[0; 300]), None);
        let a = fold_id(b"short id").unwrap();
        assert_ne!(a, fold_id(b"short ie").unwrap());
        assert_eq!(a, fold_id(b"short id").unwrap(), "stable");
    }

    /// A file without `/ID` gets one when it is saved, and its size changes: the keys must be
    /// read again after a save (the app re-keys the document's games then).
    #[test]
    fn saving_a_file_without_an_identifier_gives_it_one() {
        let mut s = Session::new().with_clock(|| 1_700_000_000);
        let bytes = crate::tests::fixture(1);
        let id = s.open("book.pdf", Some("/books/book.pdf".into()), Arc::new(bytes.clone()), None).unwrap();
        let before = s.get(id).unwrap().doc_keys();
        assert_eq!(before.id0, None);
        assert_eq!(before.path.as_deref(), Some("/books/book.pdf"));
        assert_eq!(before.name.as_deref(), Some("book.pdf"));
        assert_eq!(before.size, Some(bytes.len() as u64));
        s.apply(id, crate::Edit::RotatePages { pages: vec![0], degrees: 90 }).unwrap();
        let saved = s.save_bytes(id).unwrap();
        s.mark_saved(id, saved.clone(), Some("/books/book.pdf".into())).unwrap();
        let after = s.get(id).unwrap().doc_keys();
        assert!(after.id0.is_some(), "the save created an identifier");
        assert_eq!(after.size, Some(saved.len() as u64));
        assert!(after.matches(&before), "the path still matches");
        // A copy opened from elsewhere is found by its identifier alone.
        let copy = s.open("copy.pdf", Some("/elsewhere/copy.pdf".into()), saved, None).unwrap();
        let keys = s.get(copy).unwrap().doc_keys();
        assert_eq!(keys.id0, after.id0);
        assert!(!keys.matches(&before), "the keys from before the save don't name the copy");
        assert!(keys.matches(&after));
    }
}
