//! A book's figurine cipher kept per document (plan A9-G3c, V4b; §2.3 and §2.7; finding R3-10): the
//! symbol → piece table its pages' readings taught ([`BookCipher`]), a kind of the engine's
//! document store, so the next session, the command line and the app start from it instead of
//! from nothing. The session holds the one copy in memory ([`crate::Session::chess_cipher`]);
//! this is where it is kept.
//!
//! - **File:** `<hash>.cipher.json` (`format` [`FORMAT`], [`VERSION`]), the document's record and
//!   the table as the command line's `cipher_out` writes it (`entries` with every page's proofs
//!   counted in, then `pages`). Fields a later version adds are kept as they are.
//! - **Merge** ([`BookCipher::merge`]): by page, a page both copies read keeping the file's being
//!   written; the evidence that is no page's by the larger count, never the sum.
//! - **Versions:** no build before this one reads `<hash>.cipher.json` (a new name), so it has
//!   no old lock to take; a file of a newer version is read for what this build can make of it,
//!   and never written.
//! - **What may be kept:** [`super::store::persist_policy`], asked by every writer.

use pdfcraft_chessvision::cipher::BookCipher;
use serde::Serialize;
use serde_json::{Map, Value};

use super::corrections::DocRecord;
use super::store::{DocStore, Parsed, StoreFailure, StoreKind};

/// The `format` every cipher file names.
pub const FORMAT: &str = "pdfcraft-chess-cipher";

/// The version this build writes and reads.
pub const VERSION: u32 = 1;

/// Largest file read or written: the table's own cap, and its record around it.
pub const MAX_FILE_BYTES: usize = pdfcraft_chessvision::cipher::MAX_JSON_BYTES + (64 << 10);

/// One document's kept cipher.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CipherFile {
    pub doc: DocRecord,
    pub cipher: BookCipher,
    /// Fields of a later version of this one, kept as they were.
    pub other: Map<String, Value>,
}

/// The file as it is written.
#[derive(Serialize)]
struct Written<'a> {
    format: &'static str,
    version: u32,
    doc: &'a DocRecord,
    cipher: &'a BookCipher,
    #[serde(flatten)]
    other: &'a Map<String, Value>,
}

impl CipherFile {
    /// An empty file for the document `doc`.
    pub fn empty(doc: DocRecord) -> CipherFile {
        CipherFile { doc, ..CipherFile::default() }
    }

    /// A file's bytes, read: this version's (or a newer one's, only read), else why not.
    pub fn parse(bytes: &[u8]) -> Parsed<CipherFile> {
        if bytes.len() > MAX_FILE_BYTES {
            return Parsed::Bad(format!("larger than {} MB", MAX_FILE_BYTES >> 20));
        }
        let Ok(Value::Object(mut fields)) = serde_json::from_slice::<Value>(bytes) else {
            return Parsed::Bad("not a JSON object".into());
        };
        if fields.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Parsed::Bad("not a book cipher".into());
        }
        let version = fields.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
        fields.remove("format");
        fields.remove("version");
        let file = (|| -> Result<CipherFile, String> {
            let doc: DocRecord = serde_json::from_value(fields.remove("doc").ok_or("no document record")?).map_err(|e| e.to_string())?;
            let table = fields.remove("cipher").unwrap_or(Value::Null);
            // The table's own reading, with its caps: what a table can't hold is left out.
            let cipher =
                if table.is_null() { BookCipher::default() } else { BookCipher::from_json(&table.to_string()).map_err(|e| e.to_string())? };
            Ok(CipherFile { doc, cipher, other: fields })
        })();
        match (version, file) {
            (Some(VERSION), Ok(file)) => Parsed::File { file, skipped: 0 },
            (Some(v), file) if v > VERSION => Parsed::Newer { version: v, file: file.ok() },
            (_, Err(why)) => Parsed::Bad(why),
            (_, Ok(_)) => Parsed::Bad("no version this build reads".into()),
        }
    }

    /// The file as bytes, refused when it would be too big to read back.
    pub fn to_bytes(&self) -> Result<Vec<u8>, CipherStoreError> {
        let written = Written { format: FORMAT, version: VERSION, doc: &self.doc, cipher: &self.cipher, other: &self.other };
        let bytes = serde_json::to_vec(&written).map_err(|_| CipherStoreError::TooBig)?;
        if bytes.len() > MAX_FILE_BYTES { Err(CipherStoreError::TooBig) } else { Ok(bytes) }
    }
}

/// Why a store failed.
#[derive(Debug, thiserror::Error)]
pub enum CipherStoreError {
    #[error("the book ciphers' folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("the document has no name, path or identifier to keep a cipher under")]
    NoKeys,
    #[error("another PdfCraft is writing the book's cipher; try again")]
    Busy,
    #[error("the book's cipher is larger than {} MB", MAX_FILE_BYTES >> 20)]
    TooBig,
    #[error("the book's cipher file was written by a newer PdfCraft (version {0}); this one only reads it")]
    Newer(u32),
    #[error("the book ciphers kept here would take more than their {0} bytes")]
    OverBudget(usize),
}

impl StoreFailure for CipherStoreError {
    fn no_keys() -> Self {
        CipherStoreError::NoKeys
    }

    fn busy() -> Self {
        CipherStoreError::Busy
    }

    fn newer(version: u32) -> Self {
        CipherStoreError::Newer(version)
    }

    fn over_budget(budget: usize) -> Self {
        CipherStoreError::OverBudget(budget)
    }
}

/// The book's cipher as a kind of document store (plan A9 §2.3): `<hash>.cipher.json`, merged by
/// [`BookCipher::merge`].
pub struct Cipher;

impl StoreKind for Cipher {
    const KIND: &'static str = "cipher";
    const FORMAT: &'static str = FORMAT;
    const MAX_FILE_BYTES: usize = MAX_FILE_BYTES;
    /// No build before this one writes a cipher file.
    const OLD_LOCK: bool = false;
    type File = CipherFile;
    type Error = CipherStoreError;

    fn empty(doc: DocRecord) -> CipherFile {
        CipherFile::empty(doc)
    }

    fn doc(file: &CipherFile) -> &DocRecord {
        &file.doc
    }

    fn set_doc(file: &mut CipherFile, doc: DocRecord) {
        file.doc = doc;
    }

    fn read(bytes: &[u8]) -> Parsed<CipherFile> {
        CipherFile::parse(bytes)
    }

    fn to_bytes(file: &CipherFile) -> Result<Vec<u8>, CipherStoreError> {
        file.to_bytes()
    }

    fn holds_anything(file: &CipherFile) -> bool {
        !file.cipher.is_empty()
    }

    fn merge(mine: &mut CipherFile, theirs: &CipherFile) {
        mine.cipher.merge(&theirs.cipher);
    }
}

/// The book ciphers in a folder, one file per document (desktop and command line).
#[cfg(not(target_arch = "wasm32"))]
pub type DirStore = DocStore<Cipher, super::store::DirBackend>;

/// The book ciphers in memory, within a byte budget of their own (the web build's).
pub type MemStore = DocStore<Cipher, super::store::MemBackend>;

impl<B: super::store::StoreBackend> DocStore<Cipher, B> {
    /// Keep `cipher` as document `keys`' (of `pages` pages), merged under the store's lock with
    /// what the store holds ([`BookCipher::merge`]: a page both read keeps `cipher`'s), and give
    /// back the table now kept, which the session then holds.
    pub fn keep(&self, keys: &super::DocKeys, pages: usize, cipher: &BookCipher) -> Result<BookCipher, CipherStoreError> {
        let (kept, _) = self.update(keys, pages, |file| {
            let mut mine = cipher.clone();
            mine.merge(&file.cipher);
            file.cipher = mine;
            Ok::<BookCipher, CipherStoreError>(file.cipher.clone())
        })?;
        Ok(kept)
    }
}

#[cfg(test)]
mod tests;
