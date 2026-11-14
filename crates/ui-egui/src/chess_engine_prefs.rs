//! Preferences ▸ Chess engine (plan A9-N8, finding A8-11): the search threads, the hash table and
//! the folder of the user's Syzygy endgame tables, for the board's engine
//! ([`pdfcraft_engine::chess::analysis::EngineSettings`]).
//!
//! - **Only what the engine takes:** once an engine has answered its handshake in this session,
//!   only the settings it announced are offered, within the bounds it announced
//!   ([`pdfcraft_engine::chess::analysis::Settable`]); the others are named as not taken. Before
//!   that, all three are offered and the note says each goes only to an engine that takes it.
//!   Either way the engine is told only what it announced (the engine's own rule).
//! - **The engine's default** is a choice of its own (each `None`): nothing is sent then.
//! - **The tables are the user's:** a folder picked here, never fetched.
//! - Kept in the settings with the engine program and the lines (`chess_engine`, read as
//!   untrusted: [`settings_from`]). Desktop only: the web build has no engine.

use egui::RichText;
use pdfcraft_engine::chess::analysis::{EngineSettings, MAX_OPTION_TEXT, Settable};

use crate::PdfCraftApp;
use crate::theme::{self, Tokens};

/// Most search threads a setting holds.
const MAX_THREADS: u32 = 1024;

/// Largest hash table a setting holds, in MB (Stockfish's own largest).
const MAX_HASH_MB: u32 = 33_554_432;

/// The hash tables offered, in MB, within what the engine announced.
const HASH_SIZES: [u32; 10] = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192];

/// The engine settings from the settings file's `chess_engine`. It is untrusted: a number out of
/// bounds, or a folder that is empty, too long or has a control character in it (a line break
/// would reach the engine as a second command), is the engine's default.
pub(crate) fn settings_from(v: &serde_json::Value) -> EngineSettings {
    let number = |key: &str, max: u32| v[key].as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| (1..=max).contains(n));
    let syzygy_path =
        v["syzygy_path"].as_str().filter(|p| !p.is_empty() && p.len() <= MAX_OPTION_TEXT && !p.chars().any(char::is_control)).map(str::to_owned);
    EngineSettings { threads: number("threads", MAX_THREADS), hash_mb: number("hash_mb", MAX_HASH_MB), syzygy_path }
}

/// The threads offered: from the engine's least (at least 1) to the processors this machine has,
/// within the engine's most.
fn thread_counts(settable: Option<&Settable>) -> Vec<u32> {
    let cores = std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(MAX_THREADS));
    let (min, max) = settable.and_then(|s| s.threads).unwrap_or((1, i64::from(MAX_THREADS)));
    let min = u32::try_from(min.max(1)).unwrap_or(1);
    let max = u32::try_from(max.min(i64::from(cores.min(MAX_THREADS)))).unwrap_or(1);
    (min..=max).collect()
}

/// The hash tables offered, within the engine's bounds.
fn hash_sizes(settable: Option<&Settable>) -> Vec<u32> {
    let (min, max) = settable.and_then(|s| s.hash_mb).unwrap_or((1, i64::from(MAX_HASH_MB)));
    HASH_SIZES.into_iter().filter(|mb| (min..=max).contains(&i64::from(*mb))).collect()
}

/// A number setting's row: its name, and a list of the engine's default and `values`.
fn number_row(ui: &mut egui::Ui, name: &str, id: &str, value: &mut Option<u32>, values: &[u32], text: impl Fn(u32) -> String) {
    let label = ui.label(name);
    let shown = value.map_or_else(|| tl!("Engine's default").to_owned(), &text);
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, tl!("Engine's default"));
            for v in values {
                ui.selectable_value(value, Some(*v), text(*v));
            }
        })
        .response
        .labelled_by(label.id);
    ui.end_row();
}

/// Preferences ▸ Chess engine.
pub(crate) fn section(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) {
    ui.label(RichText::new(tl!("Chess engine")).font(theme::semibold(13.0)));
    let settable = app.chess.study.settable;
    let mut settings = app.chess.study.settings.clone();
    let takes = |of: fn(&Settable) -> bool| settable.as_ref().is_none_or(of);
    let mut choose = false;
    egui::Grid::new("chess-engine-settings").num_columns(2).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
        if takes(|s| s.threads.is_some()) {
            number_row(ui, tl!("Search threads"), "chess-engine-threads", &mut settings.threads, &thread_counts(settable.as_ref()), |n| {
                n.to_string()
            });
        }
        if takes(|s| s.hash_mb.is_some()) {
            // The unit as the engines and the operating systems write it.
            let mb = |n: u32| format!("{n} MB");
            number_row(ui, tl!("Hash table"), "chess-engine-hash", &mut settings.hash_mb, &hash_sizes(settable.as_ref()), mb);
        }
        if takes(|s| s.syzygy_path) {
            ui.label(tl!("Endgame tables (Syzygy)"));
            ui.horizontal_wrapped(|ui| {
                let shown = settings.syzygy_path.clone().unwrap_or_else(|| tl!("No folder chosen").to_owned());
                ui.label(RichText::new(shown).color(t.text_muted));
                choose = ui.button(tl!("Choose folder…")).clicked();
                if settings.syzygy_path.is_some() && ui.button(tl!("Forget")).clicked() {
                    settings.syzygy_path = None;
                }
            });
            ui.end_row();
        }
    });
    let muted = |ui: &mut egui::Ui, text: String| {
        ui.label(RichText::new(text).small().color(t.text_muted));
    };
    match settable {
        None => muted(ui, tl!("Each one goes to the engine only if it takes it. Start the analysis to see which ones this engine takes.").to_owned()),
        Some(s) => {
            let not: Vec<&str> = [
                (s.threads.is_none(), tl!("Search threads")),
                (s.hash_mb.is_none(), tl!("Hash table")),
                (!s.syzygy_path, tl!("Endgame tables (Syzygy)")),
            ]
            .into_iter()
            .filter_map(|(not, name)| not.then_some(name))
            .collect();
            if !not.is_empty() {
                muted(ui, crate::i18n::fmt(tl!("This engine does not take: {names}"), &[("names", &not.join(", "))]));
            }
        }
    }
    app.chess.study.settings = settings;
    if choose {
        app.choose_chess_tables();
    }
}

impl PdfCraftApp {
    /// Preferences ▸ Chess engine ▸ Choose folder…: the folder of the user's endgame tables (the
    /// picker answers on a later frame; `pick_override` stands in for it in tests).
    pub(crate) fn choose_chess_tables(&mut self) {
        let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Endgame tables (Syzygy)"));
        self.ask_one(crate::pickers::Ask::Folder(dialog), None, |app, path| {
            let path = path.to_string_lossy().into_owned();
            // A folder whose name the engine could not be given as one command is refused there;
            // here it is not kept.
            if path.len() <= MAX_OPTION_TEXT && !path.chars().any(char::is_control) {
                app.chess.study.settings.syzygy_path = Some(path);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_file_is_read_as_untrusted() {
        let read = |v: serde_json::Value| settings_from(&v);
        assert_eq!(
            read(serde_json::json!({ "threads": 4, "hash_mb": 256, "syzygy_path": "C:/tb" })),
            EngineSettings { threads: Some(4), hash_mb: Some(256), syzygy_path: Some("C:/tb".into()) }
        );
        for bad in [
            serde_json::json!({ "threads": 0, "hash_mb": -1, "syzygy_path": "" }),
            serde_json::json!({ "threads": 5000, "hash_mb": 99_999_999_999u64, "syzygy_path": "C:/tb\nquit" }),
            serde_json::json!({ "threads": "4", "hash_mb": 1.5, "syzygy_path": 7 }),
            serde_json::json!(null),
        ] {
            assert_eq!(read(bad.clone()), EngineSettings::default(), "{bad}");
        }
        let long = "x".repeat(MAX_OPTION_TEXT + 1);
        assert_eq!(read(serde_json::json!({ "syzygy_path": long })).syzygy_path, None);
    }

    #[test]
    fn only_what_the_engine_announced_is_offered() {
        let s = Settable { threads: Some((2, 3)), hash_mb: Some((32, 100)), syzygy_path: false };
        let threads = thread_counts(Some(&s));
        assert!(threads.first() == Some(&2) || threads.is_empty(), "{threads:?}");
        assert!(threads.iter().all(|n| (2..=3).contains(n)), "{threads:?}");
        assert_eq!(hash_sizes(Some(&s)), [32, 64]);
        // Bounds the wrong way round, or past the integers: nothing, never a panic.
        let odd = Settable { threads: Some((9, 1)), hash_mb: Some((i64::MAX, i64::MIN)), syzygy_path: true };
        assert!(thread_counts(Some(&odd)).is_empty());
        assert!(hash_sizes(Some(&odd)).is_empty());
        let wide = Settable { threads: Some((i64::MIN, i64::MAX)), hash_mb: None, syzygy_path: true };
        assert_eq!(thread_counts(Some(&wide)).first(), Some(&1));
        assert_eq!(hash_sizes(None).len(), HASH_SIZES.len(), "unknown: all offered");
    }
}
