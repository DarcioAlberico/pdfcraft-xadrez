use proptest::prelude::*;

use super::*;
use crate::game::Game;

/// A game as lichess exports it (synthetic): an evaluation and a clock in the comment of every
/// move, and nothing else.
pub(crate) const LICHESS_GAME: &str = "[Event \"Rated blitz game\"]\n[Site \"?\"]\n[Date \"2026.11.19\"]\n[Round \"-\"]\n\
[White \"White\"]\n[Black \"Black\"]\n[Result \"*\"]\n\n\
1. e4 { [%eval 0.17] [%clk 0:03:00] } 1... e5 { [%eval 0.19] [%clk 0:03:00] } \
2. Nf3 { [%eval 0.15] [%clk 0:02:58] } 2... Nc6 { [%eval 0.2] [%clk 0:02:57] } \
3. Bb5 { [%eval #4] [%clk 0:02:50] } *\n";

fn names(commands: &[Command]) -> Vec<&str> {
    commands.iter().map(Command::name).collect()
}

/// R6-01: a lichess game's comments are commands only, so their prose is empty; the commands are
/// kept as read, with their names and what they say.
#[test]
fn a_lichess_comment_is_commands_and_no_prose() {
    let (game, stopped) = Game::from_pgn(LICHESS_GAME).unwrap();
    assert_eq!(stopped, None);
    let line = game.main_line();
    assert_eq!(line.len(), 6);
    for &node in line.iter().skip(1) {
        let comment = game.comment(node);
        let (prose, commands) = split_commands(comment);
        assert_eq!(prose, "", "{comment}");
        assert!(!has_prose(comment), "{comment}");
        assert_eq!(names(&commands), ["eval", "clk"], "{comment}");
    }
    let (_, commands) = split_commands(game.comment(line[1]));
    assert_eq!(commands[0].text, "[%eval 0.17]");
    assert_eq!((commands[0].args(), commands[1].args()), ("0.17", "0:03:00"));
    assert_eq!((shown_eval(&commands).as_deref(), shown_clock(&commands).as_deref()), (Some("+0.17"), Some("3:00")));
    let (_, mate) = split_commands(game.comment(line[5]));
    assert_eq!(shown_eval(&mate).as_deref(), Some("#4"));
}

/// The prose keeps its words in order, a command between two words leaving one space, and each
/// command says where in the prose it stood.
#[test]
fn prose_and_commands_come_apart_and_back() {
    let text = "Good [%clk 0:01:00] idea [%eval 0.31]";
    let (prose, commands) = split_commands(text);
    assert_eq!(prose, "Good idea");
    assert_eq!(commands.iter().map(|c| (c.text.as_str(), c.at)).collect::<Vec<_>>(), [("[%clk 0:01:00]", 4), ("[%eval 0.31]", 9)]);
    assert_eq!(join_commands(&prose, &commands), text);
    // Glued to words, a command still comes out, and the words stay apart.
    let (prose, commands) = split_commands("a[%emt 0:00:05]b");
    assert_eq!((prose.as_str(), commands[0].at), ("a b", 1));
    assert_eq!(join_commands(&prose, &commands), "a [%emt 0:00:05] b");
    // Any `[%word …]`, the training command and the board's drawings too, kept as read.
    let (prose, commands) = split_commands("[%tqu \"En\",\"Find the win\",\"\",\"\",\"Nf3\",\"\",10] Study this. [%csl Xz9] [%evp 17,20,31]");
    assert_eq!(prose, "Study this.");
    assert_eq!(names(&commands), ["tqu", "csl", "evp"]);
    assert_eq!(commands[0].args(), "\"En\",\"Find the win\",\"\",\"\",\"Nf3\",\"\",10");
    // A comment with no command costs nothing.
    assert!(matches!(prose_of("Just words."), Cow::Borrowed("Just words.")));
    assert_eq!(prose_of("[%clk 0:01:00]"), "");
    assert!(has_prose("Fine [%clk 1:00]") && !has_prose("  [%clk 1:00]  "));
}

fn prose_of(text: &str) -> Cow<'_, str> {
    prose(text)
}

/// What ChessBase writes (the D18 session, 2026-11-19): its diagram mark `[#]`, its evaluation
/// profile before the first move, and a training question with the comment's prose after it in
/// the same braces. Each is a command; the prose stays.
#[test]
fn chessbase_s_commands_are_commands() {
    let (prose, commands) = split_commands("[#]");
    assert_eq!((prose.as_str(), names(&commands)), ("", vec!["#"]));
    assert_eq!((commands[0].args(), has_diagram_command("[#]")), ("", true));
    let (prose, commands) = split_commands("Quiet[#]development");
    assert_eq!((prose.as_str(), commands[0].at), ("Quiet development", 5));
    assert_eq!(without_diagram_command("Quiet [#] development [%clk 0:01:00]"), "Quiet development [%clk 0:01:00]");
    assert_eq!(without_diagram_command("[#]"), "");
    assert_eq!(diagram_command_as("Quiet[#]move", "Diagram"), "Quiet Diagram move");
    assert!(!has_diagram_command("[ # ]") && split_commands("[#x]").1.is_empty());
    let (prose, commands) = split_commands("[%evp 0,19,10,17,14,32767,32767,10,22]");
    assert_eq!((prose.as_str(), names(&commands)), ("", vec!["evp"]));
    let tqu = "[%tqu \"En\",\"Mate in 1.\",\"\",\"\",\"d1d8\",\"\",10,\"h2h3\",\"\",5]";
    let (prose, commands) = split_commands(&format!("{tqu} White to play and mate."));
    assert_eq!(prose, "White to play and mate.");
    assert_eq!((commands[0].text.as_str(), commands[0].at), (tqu, 0));
    assert_eq!(commands[0].args(), "\"En\",\"Mate in 1.\",\"\",\"\",\"d1d8\",\"\",10,\"h2h3\",\"\",5");
}

/// What is not a command stays prose: an unclosed one, one holding a `[` before its `]` (the
/// inner one is a command), a name that is not one.
#[test]
fn what_is_not_a_command_stays_prose() {
    for text in ["[%clk 0:03", "[% clk 1]", "[%1x 2]", "[%clk:1]", "[%]", "[clk 1]", "% clk", "[%", "[%é 1]", "[%_x 1]"] {
        let (prose, commands) = split_commands(text);
        assert!(commands.is_empty(), "{text}");
        assert_eq!(prose, text, "{text}");
    }
    // A name with nothing after it is a command.
    assert_eq!(names(&split_commands("[%clk]").1), ["clk"]);
    let (prose, commands) = split_commands("[%clk [%eval 1] x]");
    assert_eq!((prose.as_str(), names(&commands)), ("[%clk x]", vec!["eval"]));
    // A name past 32 letters is not one.
    let long = format!("[%{} 1]", "a".repeat(33));
    assert!(split_commands(&long).1.is_empty());
    assert_eq!(names(&split_commands(&format!("[%{} 1]", "a".repeat(32))).1).len(), 1);
}

/// Hostile text: a huge comment, thousands of unclosed `[%`, brackets nested deep. Each is split
/// at once, in time linear in its length.
#[test]
fn hostile_comments_split_quickly() {
    let started = std::time::Instant::now();
    let unclosed = "[%a ".repeat(250_000);
    let (prose, commands) = split_commands(&unclosed);
    assert!(commands.is_empty());
    assert_eq!(prose.len(), unclosed.trim_end().len());
    let nested = format!("{}{}", "[%clk [%eval ".repeat(50_000), "]".repeat(50_000));
    let (_, commands) = split_commands(&nested);
    assert_eq!(commands.len(), 1, "only the innermost is closed before a `[`");
    let many = "[%clk 0:00:01]".repeat(100_000);
    let (prose, commands) = split_commands(&many);
    assert_eq!((prose.as_str(), commands.len()), ("", 100_000));
    let no_close = format!("[%clk {}", "x".repeat(1 << 20));
    assert!(split_commands(&no_close).1.is_empty());
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
}

/// The clock and the evaluation as a badge shows them, and nothing for what is not one.
#[test]
fn the_badges_show_a_clock_and_an_evaluation() {
    let clock = |text: &str| shown_clock(&split_commands(text).1);
    assert_eq!(clock("[%clk 0:02:58]").as_deref(), Some("2:58"));
    assert_eq!(clock("[%clk 0:00:05]").as_deref(), Some("0:05"));
    assert_eq!(clock("[%clk 1:30:00]").as_deref(), Some("1:30:00"));
    assert_eq!(clock("[%clk 0:00:09.7]").as_deref(), Some("0:09.7"));
    assert_eq!(clock("[%emt 0:00:12]").as_deref(), Some("0:12"), "the time a move took, without a clock");
    assert_eq!(clock("[%emt 0:00:12] [%clk 0:10:00]").as_deref(), Some("10:00"), "the clock first");
    for bad in ["[%clk]", "[%clk soon]", "[%clk -1:00]", "[%clk 00000000000000000:00]", "[%eval 0.3]", "words"] {
        assert_eq!(clock(bad), None, "{bad}");
    }
    let eval = |text: &str| shown_eval(&split_commands(text).1);
    assert_eq!(eval("[%eval 0.17]").as_deref(), Some("+0.17"));
    assert_eq!(eval("[%eval -1.5]").as_deref(), Some("\u{2212}1.5"));
    assert_eq!(eval("[%eval 0.00]").as_deref(), Some("0.00"));
    assert_eq!(eval("[%eval 2.35,22]").as_deref(), Some("+2.35"), "the depth after the value");
    assert_eq!(eval("[%eval #4]").as_deref(), Some("#4"));
    assert_eq!(eval("[%eval #-2]").as_deref(), Some("#\u{2212}2"));
    for bad in ["[%eval]", "[%eval x]", "[%eval 1.]", "[%eval .5]", "[%eval #1.5]", "[%eval ##3]", "[%eval 1-2]", "[%clk 1:00]"] {
        assert_eq!(eval(bad), None, "{bad}");
    }
}

/// An engine's value for White as lichess's evaluation command.
#[test]
fn an_engine_value_is_written_as_lichess_reads_it() {
    assert_eq!(eval_command(Value::Cp(17)).as_deref(), Some("[%eval 0.17]"));
    assert_eq!(eval_command(Value::Cp(-150)).as_deref(), Some("[%eval -1.50]"));
    assert_eq!(eval_command(Value::Cp(-5)).as_deref(), Some("[%eval -0.05]"));
    assert_eq!(eval_command(Value::Cp(0)).as_deref(), Some("[%eval 0.00]"));
    assert_eq!(eval_command(Value::Cp(i32::MIN)).as_deref(), Some("[%eval -21474836.48]"));
    assert_eq!(eval_command(Value::Mates(3)).as_deref(), Some("[%eval #3]"));
    assert_eq!(eval_command(Value::Mated(2)).as_deref(), Some("[%eval #-2]"));
    assert_eq!((eval_command(Value::Mates(0)), eval_command(Value::Mated(0))), (None, None));
    // Read back, the badge shows what was written.
    let written = eval_command(Value::Cp(-150)).unwrap();
    assert_eq!(shown_eval(&split_commands(&written).1).as_deref(), Some("\u{2212}1.50"));
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

proptest! {
    /// Any text splits without a panic, and the prose and the commands give it back but for
    /// whitespace; each command is one as read, in order, at a place inside the prose.
    #[test]
    fn any_text_splits_and_joins_back(text in "([\\[\\]% a-z0-9:.,#\"é♘\t\n-]{0,12}|\\[%clk 0:0[0-9]:00\\]|\\[%eval -?[0-9]\\.[0-9]{2}\\]|\\[#\\]){0,24}") {
        let (prose, commands) = split_commands(&text);
        prop_assert_eq!(without_whitespace(&join_commands(&prose, &commands)), without_whitespace(&text));
        let mut last = 0usize;
        for c in &commands {
            prop_assert!((c.text.starts_with("[%") && c.text.ends_with(']')) || c.text == DIAGRAM_COMMAND, "{:?}", c.text);
            prop_assert!(c.at >= last && c.at <= prose.len() && prose.is_char_boundary(c.at), "{:?} in {:?}", c, prose);
            prop_assert!(!c.name().is_empty());
            last = c.at;
        }
        prop_assert_eq!(prose.trim(), prose.as_str());
        prop_assert!(!prose.contains("  "));
        let _ = (shown_clock(&commands), shown_eval(&commands), has_prose(&text));
    }

    /// Any string at all, bytes the pattern above would not make included.
    #[test]
    fn any_string_splits_without_a_panic(text in any::<String>()) {
        let (prose, commands) = split_commands(&text);
        prop_assert_eq!(without_whitespace(&join_commands(&prose, &commands)), without_whitespace(&text));
    }
}
