//! UCI, the protocol chess engines speak: the lines an engine sends while it analyses, read into
//! values, and its moves turned into SAN on the position they start from.
//!
//! Nothing here starts a process: the engine crate runs the engine and hands its lines over. An
//! engine is whatever program the user pointed at, so every line is untrusted: lines, moves and
//! numbers are bounded, and anything unreadable is skipped rather than stopping the analysis.
//!
//! Scores come from the engine as the side to move sees them; [`Score::for_white`] turns them
//! round so a board can show them from one side.

use crate::game::Game;
use crate::placement::Color;

/// Longest engine line read, in bytes. A real `info` line with a long principal variation is
/// under 2 KiB.
pub const MAX_LINE_LEN: usize = 8192;

/// Most moves of a principal variation kept.
pub const MAX_PV: usize = 64;

/// Most analysis lines (`MultiPV`) asked for and kept.
pub const MAX_MULTIPV: u32 = 5;

/// Longest engine name kept, in characters.
const MAX_NAME_CHARS: usize = 80;

/// An engine's evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Score {
    /// In hundredths of a pawn.
    Centipawns(i32),
    /// Mate in this many moves; negative when the side the score is for is being mated.
    Mate(i32),
}

impl Score {
    /// The score for White, from one given for `side` (the side to move, in engine output).
    pub fn for_white(self, side: Color) -> Score {
        match (self, side) {
            (s, Color::White) => s,
            (Score::Centipawns(cp), Color::Black) => Score::Centipawns(cp.saturating_neg()),
            (Score::Mate(n), Color::Black) => Score::Mate(n.saturating_neg()),
        }
    }

    /// White's share of the evaluation bar, 0 (Black winning) to 1 (White winning), for a score
    /// given for White. Centipawns go through the logistic curve lichess documents for its bar
    /// (`2 / (1 + e^(-0.00368208 · cp)) - 1`, as a share), so ±4 pawns fill about 80%.
    pub fn white_share(self) -> f32 {
        match self {
            Score::Centipawns(cp) => {
                let x = f64::from(cp.clamp(-10_000, 10_000));
                // The exponent is at most 36.8 in size after the clamp: finite.
                (1.0 / (1.0 + (-0.003_682_08 * x).exp())) as f32
            }
            Score::Mate(n) if n > 0 => 1.0,
            Score::Mate(n) if n < 0 => 0.0,
            Score::Mate(_) => 0.5,
        }
    }

    /// The score as analysis boards print it: `+0.34`, `-1.20`, `0.00`, `#3`, `#-2`.
    pub fn text(self) -> String {
        match self {
            Score::Centipawns(cp) => {
                let sign = if cp > 0 {
                    "+"
                } else if cp < 0 {
                    "-"
                } else {
                    ""
                };
                let abs = cp.unsigned_abs();
                format!("{sign}{}.{:02}", abs / 100, abs % 100)
            }
            Score::Mate(n) => format!("#{n}"),
        }
    }
}

/// Whether a score is exact or only a bound the search had reached when it reported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bound {
    #[default]
    Exact,
    Lower,
    Upper,
}

/// One `info` line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// Which analysis line this is, from 1 (1 when the engine does not say).
    pub multipv: u32,
    pub depth: Option<u32>,
    pub seldepth: Option<u32>,
    /// For the side to move.
    pub score: Option<Score>,
    pub bound: Bound,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time_ms: Option<u64>,
    /// The principal variation, in UCI notation (`e2e4`, `e7e8q`), at most [`MAX_PV`] moves.
    pub pv: Vec<String>,
}

/// What an engine line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// `id name …`: the engine's name.
    Name(String),
    UciOk,
    ReadyOk,
    Info(Info),
    /// The search ended with this move (`None` for `bestmove (none)` or `0000`: no legal move).
    BestMove(Option<String>),
    /// Anything else (`id author`, `option …`, `info string …`, noise).
    Other,
}

/// Read one engine line. Never fails: what cannot be read is [`Message::Other`], and an `info`
/// line keeps whatever fields it could read.
pub fn parse(line: &str) -> Message {
    if line.len() > MAX_LINE_LEN {
        return Message::Other;
    }
    let mut words = line.split_ascii_whitespace();
    match words.next() {
        Some("uciok") => Message::UciOk,
        Some("readyok") => Message::ReadyOk,
        Some("id") => match words.next() {
            Some("name") => {
                let name: String = words.collect::<Vec<_>>().join(" ").chars().take(MAX_NAME_CHARS).collect();
                if name.is_empty() { Message::Other } else { Message::Name(name) }
            }
            _ => Message::Other,
        },
        Some("bestmove") => Message::BestMove(words.next().filter(|m| is_move(m)).map(str::to_owned)),
        Some("info") => parse_info(words).map_or(Message::Other, Message::Info),
        _ => Message::Other,
    }
}

/// The fields of an `info` line after the word `info`. `None` for `info string …` (free text).
fn parse_info<'a>(words: impl Iterator<Item = &'a str>) -> Option<Info> {
    let mut info = Info { multipv: 1, ..Info::default() };
    let mut words = words.peekable();
    while let Some(key) = words.next() {
        match key {
            "string" => return None,
            "depth" => info.depth = words.next().and_then(|w| w.parse().ok()),
            "seldepth" => info.seldepth = words.next().and_then(|w| w.parse().ok()),
            "multipv" => info.multipv = words.next().and_then(|w| w.parse().ok()).filter(|n| (1..=MAX_MULTIPV).contains(n)).unwrap_or(0),
            "nodes" => info.nodes = words.next().and_then(|w| w.parse().ok()),
            "nps" => info.nps = words.next().and_then(|w| w.parse().ok()),
            "time" => info.time_ms = words.next().and_then(|w| w.parse().ok()),
            "score" => {
                info.score = match (words.next(), words.next().and_then(|w| w.parse::<i32>().ok())) {
                    (Some("cp"), Some(n)) => Some(Score::Centipawns(n)),
                    (Some("mate"), Some(n)) => Some(Score::Mate(n)),
                    _ => None,
                };
                match words.peek() {
                    Some(&"lowerbound") => {
                        info.bound = Bound::Lower;
                        words.next();
                    }
                    Some(&"upperbound") => {
                        info.bound = Bound::Upper;
                        words.next();
                    }
                    _ => {}
                }
            }
            "pv" => {
                info.pv.clear();
                while let Some(m) = words.peek().copied().filter(|m| is_move(m)) {
                    if info.pv.len() < MAX_PV {
                        info.pv.push(m.to_owned());
                    }
                    words.next();
                }
            }
            // The rest of the line belongs to these.
            "refutation" | "currline" => break,
            // Every other key the protocol defines takes one value (currmove, hashfull, tbhits…).
            _ => {
                words.next();
            }
        }
    }
    // A line number out of range is not a line this board shows.
    (info.multipv > 0).then_some(info)
}

/// Whether `text` is a move in UCI notation: two squares and maybe the piece a pawn becomes.
/// The null move `0000` is not.
pub fn is_move(text: &str) -> bool {
    let b = text.as_bytes();
    let square = |f: u8, r: u8| (b'a'..=b'h').contains(&f) && (b'1'..=b'8').contains(&r);
    match b {
        [f1, r1, f2, r2] => square(*f1, *r1) && square(*f2, *r2),
        [f1, r1, f2, r2, p] => square(*f1, *r1) && square(*f2, *r2) && matches!(p, b'q' | b'r' | b'b' | b'n'),
        _ => false,
    }
}

/// The moves of `pv` in SAN, played from `fen`; the list stops at the first move that cannot be
/// played there (an engine may report a line for a position it was not given yet).
pub fn pv_san(fen: &str, pv: &[String]) -> Vec<String> {
    let Ok(mut game) = Game::from_fen(fen) else { return Vec::new() };
    let mut out = Vec::new();
    for mv in pv.iter().take(MAX_PV) {
        if game.play_uci(mv).is_err() {
            break;
        }
        out.push(game.san(game.current()).to_owned());
    }
    out
}

/// `sans` numbered as a move list from `fen`'s move number and side to move: `12. Nf3 Nc6 13. d4`,
/// or `12... Nc6 13. d4` when Black moves first.
pub fn numbered(fen: &str, sans: &[String]) -> String {
    let mut fields = fen.split_ascii_whitespace().skip(1);
    let mut black = fields.next() == Some("b");
    let mut number: u32 = fields.nth(3).and_then(|n| n.parse().ok()).filter(|n| *n > 0).unwrap_or(1);
    let mut out = String::new();
    for (i, san) in sans.iter().enumerate() {
        if !out.is_empty() {
            out.push(' ');
        }
        if !black {
            out.push_str(&format!("{number}. "));
        } else if i == 0 {
            out.push_str(&format!("{number}... "));
        }
        out.push_str(san);
        if black {
            number = number.saturating_add(1);
        }
        black = !black;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn reads_a_stockfish_info_line() {
        let line =
            "info depth 22 seldepth 31 multipv 2 score cp -35 upperbound nodes 1234567 nps 987654 hashfull 120 tbhits 0 time 1250 pv e7e5 g1f3 b8c6";
        let Message::Info(info) = parse(line) else { panic!("not an info line") };
        assert_eq!(info.multipv, 2);
        assert_eq!(info.depth, Some(22));
        assert_eq!(info.seldepth, Some(31));
        assert_eq!(info.score, Some(Score::Centipawns(-35)));
        assert_eq!(info.bound, Bound::Upper);
        assert_eq!(info.nodes, Some(1_234_567));
        assert_eq!(info.nps, Some(987_654));
        assert_eq!(info.time_ms, Some(1250));
        assert_eq!(info.pv, ["e7e5", "g1f3", "b8c6"]);
    }

    #[test]
    fn reads_the_other_messages() {
        assert_eq!(parse("uciok"), Message::UciOk);
        assert_eq!(parse("readyok\r"), Message::ReadyOk);
        assert_eq!(parse("id name Stockfish 17"), Message::Name("Stockfish 17".into()));
        assert_eq!(parse("id author the Stockfish developers"), Message::Other);
        assert_eq!(parse("bestmove e2e4 ponder e7e5"), Message::BestMove(Some("e2e4".into())));
        assert_eq!(parse("bestmove (none)"), Message::BestMove(None));
        assert_eq!(parse("bestmove 0000"), Message::BestMove(None));
        assert_eq!(parse("info string NNUE evaluation using nn-1111.nnue"), Message::Other);
        assert_eq!(parse("option name Hash type spin default 16 min 1 max 33554432"), Message::Other);
        assert_eq!(parse(""), Message::Other);
    }

    #[test]
    fn unreadable_fields_are_skipped_not_fatal() {
        let Message::Info(info) = parse("info depth x score mate 3 pv e2e4 zz e7e5") else { panic!() };
        assert_eq!(info.depth, None);
        assert_eq!(info.score, Some(Score::Mate(3)));
        // The PV stops at the first word that is not a move.
        assert_eq!(info.pv, ["e2e4"]);
        assert_eq!(parse("info score cp"), Message::Info(Info { multipv: 1, ..Info::default() }));
        // A line number out of range is not shown.
        assert_eq!(parse("info multipv 9 score cp 1 pv e2e4"), Message::Other);
        assert_eq!(parse("info multipv 0 score cp 1 pv e2e4"), Message::Other);
        // Overlong lines are dropped whole.
        assert_eq!(parse(&format!("info pv {}", "e2e4 ".repeat(MAX_LINE_LEN))), Message::Other);
        let long = format!("info pv {}", "e2e4 e7e5 ".repeat(100));
        let Message::Info(info) = parse(&long) else { panic!() };
        assert_eq!(info.pv.len(), MAX_PV);
    }

    #[test]
    fn moves_are_two_squares_and_a_promotion() {
        assert!(is_move("e2e4"));
        assert!(is_move("e7e8q"));
        assert!(!is_move("e7e8k"));
        assert!(!is_move("0000"));
        assert!(!is_move("e9e4"));
        assert!(!is_move("e2"));
        assert!(!is_move("é2e4"));
    }

    #[test]
    fn pv_becomes_san_and_stops_at_a_move_that_cannot_be_played() {
        let pv: Vec<String> = ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6", "e1g1", "a1a8"].map(String::from).to_vec();
        assert_eq!(pv_san(START, &pv), ["e4", "e5", "Nf3", "Nc6", "Bc4", "Nf6", "O-O"]);
        assert_eq!(pv_san("8/P6k/8/8/8/8/8/K7 w - - 0 1", &["a7a8q".to_owned()]), ["a8=Q"]);
        assert!(pv_san("not a fen", &pv).is_empty());
    }

    #[test]
    fn numbering_follows_the_side_to_move() {
        let sans: Vec<String> = ["Nf3", "Nc6", "d4"].map(String::from).to_vec();
        assert_eq!(numbered("8/8/8/8/8/8/8/K6k w - - 0 12", &sans), "12. Nf3 Nc6 13. d4");
        assert_eq!(numbered("8/8/8/8/8/8/8/K6k b - - 0 12", &sans), "12... Nf3 13. Nc6 d4");
        assert_eq!(numbered("8/8/8/8/8/8/8/K6k w", &sans[..1]), "1. Nf3");
        assert_eq!(numbered(START, &[]), "");
    }

    #[test]
    fn scores_turn_round_and_print_as_boards_do() {
        assert_eq!(Score::Centipawns(34).for_white(Color::Black), Score::Centipawns(-34));
        assert_eq!(Score::Mate(-2).for_white(Color::Black), Score::Mate(2));
        assert_eq!(Score::Centipawns(i32::MIN).for_white(Color::Black), Score::Centipawns(i32::MAX));
        assert_eq!(Score::Centipawns(34).text(), "+0.34");
        assert_eq!(Score::Centipawns(-120).text(), "-1.20");
        assert_eq!(Score::Centipawns(0).text(), "0.00");
        assert_eq!(Score::Centipawns(i32::MIN).text(), "-21474836.48");
        assert_eq!(Score::Mate(3).text(), "#3");
        assert_eq!(Score::Mate(-2).text(), "#-2");
    }

    #[test]
    fn the_bar_share_grows_with_the_score_and_stays_in_range() {
        let shares: Vec<f32> = [i32::MIN, -1000, -100, 0, 100, 1000, i32::MAX].iter().map(|cp| Score::Centipawns(*cp).white_share()).collect();
        assert!(shares.windows(2).all(|w| w[0] <= w[1]), "{shares:?}");
        assert!(shares.iter().all(|s| s.is_finite() && (0.0..=1.0).contains(s)));
        assert!((Score::Centipawns(0).white_share() - 0.5).abs() < 1e-6);
        assert_eq!(Score::Mate(1).white_share(), 1.0);
        assert_eq!(Score::Mate(-1).white_share(), 0.0);
    }
}
