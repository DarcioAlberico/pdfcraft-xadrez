//! PGN files as bytes: the one place their text is decoded (plan A9-F1).
//!
//! The PGN standard (§4.1) names ISO 8859-1, most programs now write UTF-8, some with a byte
//! order mark, and a collection made by joining files (`copy *.pgn`) mixes them. So [`decode`]
//! takes the bytes a line at a time: a line that is valid UTF-8 is kept as it is, any other is read
//! as Windows-1252 (ISO 8859-1 with printable characters where Latin-1 has C1 controls), and a
//! leading byte order mark is dropped. Deciding for the whole file instead would turn every UTF-8
//! game of a joined collection into mojibake for the sake of one Latin-1 game.

/// How a PGN's bytes were read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PgnEncoding {
    /// UTF-8 (or plain ASCII).
    Utf8,
    /// UTF-8 after a byte order mark, which was dropped.
    Utf8Bom,
    /// Every line that is not ASCII is Windows-1252 (or ISO 8859-1).
    Windows1252,
    /// Some lines UTF-8 and `lines_1252` lines Windows-1252, as a joined collection is.
    Mixed { lines_1252: usize },
}

impl PgnEncoding {
    /// Whether the text was read as UTF-8 throughout (a byte order mark or not).
    pub fn is_utf8(self) -> bool {
        matches!(self, PgnEncoding::Utf8 | PgnEncoding::Utf8Bom)
    }

    /// A short name for messages and tool replies: `utf-8`, `utf-8 with BOM`, `windows-1252` or
    /// `mixed`.
    pub fn name(self) -> &'static str {
        match self {
            PgnEncoding::Utf8 => "utf-8",
            PgnEncoding::Utf8Bom => "utf-8 with BOM",
            PgnEncoding::Windows1252 => "windows-1252",
            PgnEncoding::Mixed { .. } => "mixed",
        }
    }
}

/// The byte order mark of UTF-8.
pub const BOM: &[u8] = b"\xef\xbb\xbf";

/// The characters Windows-1252 puts at 0x80–0x9F; the five it leaves undefined (0x81, 0x8D,
/// 0x8F, 0x90, 0x9D) stand for themselves, as the WHATWG Encoding Standard decodes them.
const HIGH: [char; 32] = [
    '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}',
    '\u{152}', '\u{8d}', '\u{17d}', '\u{8f}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}', '\u{178}',
];

/// One byte as Windows-1252.
fn windows_1252(b: u8) -> char {
    match b {
        0x80..=0x9f => HIGH.get(usize::from(b - 0x80)).copied().unwrap_or(char::REPLACEMENT_CHARACTER),
        _ => char::from(b),
    }
}

/// The text of a PGN file's bytes and how they were read: line by line, UTF-8 where a line is
/// valid UTF-8 and Windows-1252 where it is not, a leading byte order mark dropped.
pub fn decode(bytes: &[u8]) -> (String, PgnEncoding) {
    let (bom, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let mut text = String::with_capacity(body.len());
    let (mut lines_1252, mut lines_utf8) = (0_usize, 0_usize);
    for line in body.split_inclusive(|b| *b == b'\n') {
        match std::str::from_utf8(line) {
            Ok(s) => {
                if !s.is_ascii() {
                    lines_utf8 = lines_utf8.saturating_add(1);
                }
                text.push_str(s);
            }
            Err(_) => {
                lines_1252 = lines_1252.saturating_add(1);
                text.extend(line.iter().map(|b| windows_1252(*b)));
            }
        }
    }
    let encoding = match (lines_1252, bom || lines_utf8 > 0) {
        (0, _) if bom => PgnEncoding::Utf8Bom,
        (0, _) => PgnEncoding::Utf8,
        (_, false) => PgnEncoding::Windows1252,
        (n, true) => PgnEncoding::Mixed { lines_1252: n },
    };
    (text, encoding)
}

/// `text` without the byte order mark it may start with (text pasted, or passed through JSON,
/// keeps the U+FEFF a file's bytes had).
pub fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// The text the readers before plan A9-F1 made of these bytes (every byte that is not UTF-8 a
/// U+FFFD), for comparing with what was kept then.
pub fn legacy_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The bytes of a PGN file holding `text`: UTF-8, after a byte order mark when `bom` asks for one
/// (PGN is written without one unless the user asks: decision D18).
pub fn encode(text: &str, bom: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len().saturating_add(BOM.len()));
    if bom {
        bytes.extend_from_slice(BOM);
    }
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A1-10: a Latin-1 (Windows-1252) PGN keeps its accents and says how it was read.
    #[test]
    fn a_windows_1252_pgn_keeps_its_accents() {
        let (text, encoding) = decode(b"[White \"R\xe9ti\"]\n[Black \"Capablanca\"]\n\n1. Nf3 {S\xe3o Paulo \x96 \x93aspas\x94} *\n");
        assert!(text.contains("Réti") && text.contains("São Paulo – “aspas”"), "{text}");
        assert_eq!(encoding, PgnEncoding::Windows1252);
        assert!(!encoding.is_utf8());
        // The five bytes Windows-1252 leaves undefined stand for themselves, as WHATWG reads them.
        let (text, _) = decode(b"\x81\x8d\x8f\x90\x9d\x80\x9f");
        assert_eq!(text, "\u{81}\u{8d}\u{8f}\u{90}\u{9d}€Ÿ");
    }

    /// A1-01: a byte order mark is dropped, and the file is read as UTF-8.
    #[test]
    fn a_byte_order_mark_is_dropped() {
        let (text, encoding) = decode("\u{feff}[White \"Réti\"]\n1. e4 *\n".as_bytes());
        assert_eq!(text, "[White \"Réti\"]\n1. e4 *\n");
        assert_eq!(encoding, PgnEncoding::Utf8Bom);
        assert!(encoding.is_utf8());
        assert_eq!(decode("[White \"Réti\"]".as_bytes()), ("[White \"Réti\"]".to_owned(), PgnEncoding::Utf8));
        assert_eq!(decode(b""), (String::new(), PgnEncoding::Utf8));
        assert_eq!(without_bom("\u{feff}1. e4"), "1. e4");
        assert_eq!(without_bom("1. e4"), "1. e4");
    }

    /// Files joined into one collection: each line read as what it is, so a Latin-1 game does
    /// not turn the UTF-8 one into mojibake.
    #[test]
    fn a_joined_collection_is_read_line_by_line() {
        let mut bytes = "[White \"Réti\"]\n1. e4 *\n\n".as_bytes().to_vec();
        bytes.extend_from_slice(b"[White \"R\xe9ti\"]\r\n1. d4 *\r\n");
        let (text, encoding) = decode(&bytes);
        assert_eq!(text.matches("Réti").count(), 2, "{text}");
        assert_eq!(encoding, PgnEncoding::Mixed { lines_1252: 1 });
        // After a byte order mark too.
        let mut bom = BOM.to_vec();
        bom.extend_from_slice(&bytes);
        assert_eq!(decode(&bom).1, PgnEncoding::Mixed { lines_1252: 1 });
        // The text the readers used to make put U+FFFD in the second game.
        assert!(legacy_text(&bytes).contains("R\u{fffd}ti"));
    }

    #[test]
    fn a_file_is_written_with_a_byte_order_mark_only_when_asked() {
        assert_eq!(encode("1. e4 *\n", false), b"1. e4 *\n");
        assert_eq!(encode("Réti", true), "\u{feff}Réti".as_bytes());
        assert_eq!(decode(&encode("[White \"Réti\"]\n", true)), ("[White \"Réti\"]\n".to_owned(), PgnEncoding::Utf8Bom));
    }

    /// The D18 kit (`docs/chess/d18-kit/`, synthetic files written for the ChessBase session):
    /// every file decodes as it was written and every game in it reads to its end, accents kept.
    #[test]
    fn the_d18_kit_reads_whole() {
        let kit: [(&str, &[u8], PgnEncoding, usize); 6] = [
            ("1-utf8.pgn", include_bytes!("../../../docs/chess/d18-kit/1-utf8.pgn"), PgnEncoding::Utf8, 2),
            ("2-utf8-com-bom.pgn", include_bytes!("../../../docs/chess/d18-kit/2-utf8-com-bom.pgn"), PgnEncoding::Utf8Bom, 2),
            ("3-windows-1252.pgn", include_bytes!("../../../docs/chess/d18-kit/3-windows-1252.pgn"), PgnEncoding::Windows1252, 2),
            ("4-nags-das-pretas.pgn", include_bytes!("../../../docs/chess/d18-kit/4-nags-das-pretas.pgn"), PgnEncoding::Utf8, 1),
            ("5-marcas-de-diagrama.pgn", include_bytes!("../../../docs/chess/d18-kit/5-marcas-de-diagrama.pgn"), PgnEncoding::Utf8, 1),
            ("6-exercicio.pgn", include_bytes!("../../../docs/chess/d18-kit/6-exercicio.pgn"), PgnEncoding::Utf8, 2),
        ];
        for (name, bytes, encoding, count) in kit {
            let (text, read_as) = decode(bytes);
            assert_eq!(read_as, encoding, "{name}");
            let games = crate::game::split_pgn(&text, 10);
            assert_eq!(games.len(), count, "{name}");
            for game in games {
                let (_, stopped) = crate::game::Game::from_pgn(game).unwrap();
                assert_eq!(stopped, None, "{name}: {game}");
            }
            if name.starts_with(['1', '2', '3']) {
                assert!(text.contains("Réti, Richard") && text.contains("São Paulo") && text.contains("Peña, Ñuño"), "{name}");
            }
        }
    }

    proptest::proptest! {
        /// Any bytes give text; UTF-8 text gives itself.
        #[test]
        fn any_bytes_decode(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..512)) {
            let (text, encoding) = decode(&bytes);
            if let Ok(s) = std::str::from_utf8(&bytes) {
                proptest::prop_assert_eq!(text.as_str(), s.strip_prefix('\u{feff}').unwrap_or(s));
                proptest::prop_assert!(encoding.is_utf8());
            }
        }
    }
}
