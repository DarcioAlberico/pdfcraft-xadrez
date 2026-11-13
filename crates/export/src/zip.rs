//! A minimal zip writer (deflate or store, no zip64) for Office files and EPUBs.
//!
//! Without zip64 an archive holds at most 65,535 files, each and all of them under 4 GB, and a
//! name under 64 KB: past any of these, [`Zip::add`] and [`Zip::finish`] refuse with a
//! [`ZipError`] (plan A9-X1a, A4-18: the sizes were cut with `as u32`, the count overflowed, and an
//! error of the compressor became an empty file).

use std::io::Write;

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        // Masked to a byte: always within the 256 entries.
        c = table[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// Why an archive could not be made: the zip format without zip64 holds no more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZipError {
    /// A file, as stored, is 4 GB or larger.
    FileTooLarge(String),
    /// A file's name is 64 KB or longer.
    NameTooLong,
    /// The archive holds 65,535 files already.
    TooManyFiles,
    /// The archive is 4 GB or larger.
    ArchiveTooLarge,
    /// The compressor failed on a file.
    Deflate { name: String, error: String },
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipError::FileTooLarge(name) => write!(f, "{name} is 4 GB or larger, more than a zip file holds"),
            ZipError::NameTooLong => f.write_str("a file name is longer than a zip file holds (65,535 bytes)"),
            ZipError::TooManyFiles => write!(f, "more than {} files, the most a zip file holds", u16::MAX),
            ZipError::ArchiveTooLarge => f.write_str("the file would be 4 GB or larger, more than a zip file holds"),
            ZipError::Deflate { name, error } => write!(f, "{name} could not be compressed: {error}"),
        }
    }
}

impl std::error::Error for ZipError {}

/// Builds a zip archive in memory.
#[derive(Default)]
pub struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Zip {
    /// Add a file. Already-compressed data (images) is stored; the rest is deflated. Refused, the
    /// archive left as it was, when the file, its name or the archive would pass what a zip file
    /// without zip64 holds, or the compressor fails.
    pub fn add(&mut self, name: &str, data: &[u8], compress: bool) -> Result<(), ZipError> {
        if self.count == u16::MAX {
            return Err(ZipError::TooManyFiles);
        }
        let name_len = u16::try_from(name.len()).map_err(|_| ZipError::NameTooLong)?;
        let too_large = || ZipError::FileTooLarge(name.to_owned());
        let size = u32::try_from(data.len()).map_err(|_| too_large())?;
        let crc = crc32(data);
        let (method, body) = if compress {
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            let deflated = e.write_all(data).and_then(|()| e.finish());
            (8u16, deflated.map_err(|error| ZipError::Deflate { name: name.to_owned(), error: error.to_string() })?)
        } else {
            (0u16, data.to_vec())
        };
        let body_len = u32::try_from(body.len()).map_err(|_| too_large())?;
        let offset = u32::try_from(self.out.len()).map_err(|_| ZipError::ArchiveTooLarge)?;
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // version needed
            h.extend(0x0800u16.to_le_bytes()); // UTF-8 names
            h.extend(method.to_le_bytes());
            h.extend([0, 0, 0x21, 0]); // 00:00, 1980-01-01 (a zero date is not a valid DOS date)
            h.extend(crc.to_le_bytes());
            h.extend(body_len.to_le_bytes());
            h.extend(size.to_le_bytes());
            h.extend(name_len.to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra
            if central {
                h.extend(0u16.to_le_bytes()); // comment
                h.extend(0u16.to_le_bytes()); // disk
                h.extend(0u16.to_le_bytes()); // internal attributes
                h.extend(0u32.to_le_bytes()); // external attributes
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        self.out.extend(header(0x0403_4b50, false));
        self.out.extend(&body);
        self.central.extend(header(0x0201_4b50, true));
        // Below u16::MAX: checked above.
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// The archive's bytes; refused when it is 4 GB or larger.
    pub fn finish(mut self) -> Result<Vec<u8>, ZipError> {
        let start = u32::try_from(self.out.len()).map_err(|_| ZipError::ArchiveTooLarge)?;
        let size = u32::try_from(self.central.len()).map_err(|_| ZipError::ArchiveTooLarge)?;
        self.out.append(&mut self.central);
        self.out.extend(0x0605_4b50u32.to_le_bytes());
        self.out.extend([0u8; 4]);
        self.out.extend(self.count.to_le_bytes());
        self.out.extend(self.count.to_le_bytes());
        self.out.extend(size.to_le_bytes());
        self.out.extend(start.to_le_bytes());
        self.out.extend(0u16.to_le_bytes());
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_reference() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn archive_has_both_directories() {
        let mut z = Zip::default();
        z.add("a.txt", b"hello hello hello", true).unwrap();
        z.add("b.png", b"\x89PNG", false).unwrap();
        let b = z.finish().unwrap();
        assert!(b.starts_with(b"PK\x03\x04"));
        assert_eq!(b.windows(4).filter(|w| *w == b"PK\x01\x02").count(), 2);
        assert_eq!(&b[b.len() - 22..b.len() - 18], b"PK\x05\x06");
    }

    /// Plan A9-X1a (A4-18): past what a zip file without zip64 holds, the archive refuses the
    /// file, unchanged, instead of cutting its sizes or its count (a count past 65,535 overflowed).
    #[test]
    fn what_a_zip_file_cannot_hold_is_refused() {
        let mut z = Zip::default();
        z.add("a.txt", b"a", true).unwrap();
        let before = (z.out.clone(), z.central.clone());
        assert_eq!(z.add(&"n".repeat(usize::from(u16::MAX) + 1), b"x", true), Err(ZipError::NameTooLong));
        assert!(z.add(&"n".repeat(usize::from(u16::MAX)), b"x", false).is_ok(), "the longest name a zip file holds");
        z.count = u16::MAX;
        let held = (z.out.clone(), z.central.clone());
        assert_eq!(z.add("one-too-many.txt", b"x", true), Err(ZipError::TooManyFiles));
        assert_eq!((z.out.clone(), z.central.clone()), held, "left as it was");
        assert_ne!(held, before);
        let bytes = z.finish().unwrap();
        assert_eq!(bytes.get(bytes.len() - 12..bytes.len() - 10), Some(&u16::MAX.to_le_bytes()[..]));
    }
}
