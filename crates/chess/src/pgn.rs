//! PGN files: the one place their bytes are decoded (plan A9-F1) and the one writer of their text
//! (plan A9-F4, [`write`]).
//!
//! The PGN standard (§4.1) names ISO 8859-1, most programs now write UTF-8, some with a byte
//! order mark, and a collection made by joining files (`copy *.pgn`) mixes them. So [`decode`]
//! takes the bytes a line at a time: a line that is valid UTF-8 is kept as it is, any other is read
//! as Windows-1252 (ISO 8859-1 with printable characters where Latin-1 has C1 controls), and a
//! leading byte order mark is dropped. Deciding for the whole file instead would turn every UTF-8
//! game of a joined collection into mojibake for the sake of one Latin-1 game.
//!
//! A comment's `[%…]` commands (`[%clk]`, `[%eval]`) are told apart from its prose here too
//! ([`split_commands`], plan A9-F7).

mod commands;
mod writer;

pub use commands::{
    Command, DIAGRAM_COMMAND, diagram_command_as, eval_command, has_diagram_command, has_prose, join_commands, prose, shown_clock, shown_eval,
    split_commands, without_diagram_command,
};
pub use writer::{ECO_PLIES, LINE_WIDTH, WriteOptions, comment_letters, movetext, write, write_games};

/// Largest PGN file read, in bytes: a collection opened, a book's PGN, the games a tool reads
/// (one cap for all of them, plan A9-F4).
pub const MAX_PGN_FILE_BYTES: usize = 32 << 20;

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

    /// Plan A9-F7: a file shaped as ChessBase 26 writes one (what the D18 session saw, written
    /// here by hand): a byte order mark, CRLF lines and an LF game in one file, the next `[Event`
    /// right after a result with no blank line, a tag twice and the tags it adds, the whole move
    /// text on one line, its evaluation profile before the first move, a diagram as `{[#]}`, a
    /// Black move after a comment with no `5...`, and a training question with the comment's prose
    /// after it. Every game reads whole, and is written back with its commands where they were.
    #[test]
    fn a_pgn_as_chessbase_writes_it_reads_whole() {
        let mut bytes = BOM.to_vec();
        bytes.extend_from_slice(
            b"[Event \"Marks\"]\r\n[Site \"?\"]\r\n[Date \"2026.11.11\"]\r\n[Round \"1\"]\r\n[White \"White\"]\r\n[Black \"Black\"]\r\n\
              [Result \"*\"]\r\n[Annotator \"someone\"]\r\n[ECO \"E59\"]\r\n[WhiteFideId \"-1\"]\r\n[WhiteFideId \"-1\"]\r\n[PlyCount \"20\"]\r\n\
              [GameId \"1234567890\"]\r\n[EventDate \"2026.??.??\"]\r\n\r\n\
              {[%evp 0,19,10,17,14,32767,32767,10]} 1. d4 Nf6 2. c4 e6 3. Nc3 Bb4 4. e3 O-O 5. Bd3 {[#]} d5 6. Nf3 c5 7. O-O Nc6 8. a3 Bxc3 9. bxc3 dxc4 10. Bxc4 Qc7 *\r\n\r\n\
              [Event \"Copied\"]\n[Result \"1/2-1/2\"]\n\n1. e4 c5 2. Nf3 d6 1/2-1/2\n\
              [Event \"Signs\"]\r\n[Result \"*\"]\r\n\r\n\
              1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O $36 {The initiative.} Be7 6. Re1 b5 7. Bb3 d6 *\r\n\r\n\
              [Event \"Exercise\"]\r\n[Result \"*\"]\r\n[SetUp \"1\"]\r\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1\"]\r\n[PlyCount \"1\"]\r\n\r\n\
              {[%tqu \"En\",\"Mate in 1.\",\"\",\"\",\"d1d8\",\"\",10,\"h2h3\",\"\",5] White to play and mate.} 1. Rd8# *\r\n\r\n",
        );
        let (text, encoding) = decode(&bytes);
        assert_eq!(encoding, PgnEncoding::Utf8Bom);
        let games = crate::game::split_pgn(&text, 10);
        assert_eq!(games.len(), 4, "{games:#?}");
        let read: Vec<crate::game::Game> = games
            .iter()
            .map(|g| {
                let (game, stopped) = crate::game::Game::from_pgn(g).unwrap();
                assert_eq!(stopped, None, "{g}");
                game
            })
            .collect();
        let [marks, copied, signs, exercise] = read.as_slice() else { panic!("four games") };
        // The first: twenty plies, the profile before them, the diagram after 5.Bd3.
        assert_eq!(marks.main_line().len(), 21);
        assert_eq!(marks.tag("WhiteFideId"), Some("-1"));
        assert_eq!((marks.tag("GameId"), marks.tag("EventDate")), (Some("1234567890"), Some("2026.??.??")));
        assert_eq!(prose(marks.comment(0)), "");
        let bd3 = marks.main_line()[9];
        assert!(marks.has_diagram(bd3) && prose(marks.comment(bd3)).is_empty());
        let written = marks.pgn();
        assert!(written.contains("{[%evp 0,19,10,17,14,32767,32767,10]} 1. d4") && written.contains("5. Bd3 {[#]} 5... d5"), "{written}");
        // The LF game, and the one after it with no blank line between.
        assert_eq!((copied.tag("Event"), copied.result().as_str(), copied.main_line().len()), (Some("Copied"), "1/2-1/2", 5));
        let line = signs.main_line();
        assert_eq!((signs.san(line[10]), signs.nags(line[9]), signs.comment(line[9])), ("Be7", &[36][..], "The initiative."));
        // The training question is a command, the prose after it the comment's words.
        let (words, commands) = split_commands(exercise.comment(0));
        assert_eq!((words.as_str(), commands.len()), ("White to play and mate.", 1));
        assert!(
            exercise
                .movetext()
                .starts_with("{[%tqu \"En\",\"Mate in 1.\",\"\",\"\",\"d1d8\",\"\",10,\"h2h3\",\"\",5] White to play and mate.} 1. Rd8#")
        );
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
