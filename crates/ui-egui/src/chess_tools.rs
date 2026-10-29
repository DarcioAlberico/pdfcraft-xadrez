//! Chess ▸ PGN toolbar (T5a): the vertical tool rail beside the chess board, and the keys the
//! chess panel takes for itself while its scope is on.
//!
//! **The rail** is drawn from [`RAIL`], a static list of sections with a separator between them.
//! A section holds [`Entry`]s: a [`Tool`] or a [`Group`] of tools. A tool is either a mode
//! ([`Kind::Mode`]: armed until Esc or another mode, see [`PgnTool`]) or an action
//! ([`Kind::Action`]: it acts once on the board), or a switch ([`Kind::Switch`]: Follow, on until
//! pressed again, `chess_source`). A group shows one of its members, the last
//! used, with a small triangle in its corner; a long press (at least [`LONG_PRESS`]), a right
//! click, or Enter or Down arrow while it has the keyboard focus opens the others, and Shift with
//! the group's letter switches the member shown. Later steps add tools by adding entries: the
//! rail, the keys the scope takes, the key hints in the names and the move list's context menu
//! all follow from the list. The annotation tools (T6) are there too: the three glyph groups,
//! whose buttons show their glyph as text and are named for it (while a text field the palette
//! types into had the keyboard, a glyph with a sign goes there instead, as from the palette), the
//! comment editor and the glyph palette (`chess_annotate`). T15 adds the glyph brush (a mode: a click on a move in the
//! notation puts the brush's glyph on that move or takes it off, instead of going to it), moving a
//! variation up or down among the others at its fork (Alt+↑, Alt+↓) and the threat (a null move)
//! to the Structure group.
//!
//! **The key scope** (`chess_scope`, spec §4.2). While Undo goes to the game
//! ([`ChessState::in_scope`]) and a rail was on screen in the last frame, the chess keys (Esc,
//! the arrows, Home, End and every key a rail tool or group has) are taken out of the frame's raw
//! input in `raw_input_hook`, before egui (which moves the focus on the arrows) and before every
//! shortcut of the app (Delete on the page grid's pages and on the selected annotation, the arrows
//! on the page) can see them, and handed to the panel through [`ToolState`]. They are left alone
//! while a text field has the keyboard, so a letter or Delete typed in a comment is the comment's;
//! the hook runs before the frame, so that is the focus as the last frame left it. They are also
//! left alone while a popup or menu is open (Esc and the arrows are its own) and under a modal
//! dialog. Enter is taken only while nothing has the keyboard focus (a focused button is pressed
//! with it). Text made only of the characters of typed glyphs (`!?=+-/~0`) is taken too, for
//! `chess_annotate::Typed`.

use std::time::Duration;

use egui::{Align, Event, Key, Layout, Modifiers, Rect, Sense, Stroke, UiBuilder, pos2, vec2};
use pdfcraft_engine::chess::capture::CaptureMode;
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodeId, NodePath, Shift, StructureError};
use pdfcraft_engine::commands::Shortcut;

use crate::chess_annotate::{self, TYPED_WAIT, Typed};
use crate::chess_board_ui::BoardState;
use crate::chess_history::ChessEdit;
use crate::chess_ui::ChessState;
use crate::icons;
use crate::theme::Tokens;

/// The rail's width, in points.
pub(crate) const RAIL_WIDTH: f32 = 32.0;

/// A rail button's side, in points (WCAG 2.5.8 asks for at least 24).
const BUTTON: f32 = 28.0;

/// How long a group's button is held before its other tools show, in seconds.
pub(crate) const LONG_PRESS: f64 = 0.4;

/// Most keys kept for the panel from one frame's input (a held key repeats).
const MAX_KEYS: usize = 64;

/// Most app commands queued between two frames.
const MAX_COMMANDS: usize = 8;

/// The mode the rail has armed. Navigate is the default; the capture modes are `chess_capture`'s
/// (T7a); the OCR area (T9) is a switch on their source, `chess_ocr_area`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PgnTool {
    /// A click on a move goes to it; nothing is armed.
    #[default]
    Navigate,
    /// Text selected on the page goes into the game, as the mode says.
    Capture(CaptureMode),
    /// Text selected on the page becomes the move's comment: `before` it (else after), and in
    /// place of what it says (`replace`, else added to it).
    Comment { before: bool, replace: bool },
    /// The set-up view in the board's place (T5c, `chess_study_ui::setup_in_place`).
    Setup,
    /// The glyph brush (T15): a click on a move in the notation puts glyph `code` (White's code;
    /// the side's is used) on that move, or takes it off, as one undo step, instead of going to
    /// it. A glyph chosen in a glyph group while it is armed becomes the brush's.
    GlyphBrush(u16),
}

impl PgnTool {
    /// Whether `self` and `other` are the same tool of the rail: Comment is one tool whatever its
    /// switches.
    pub fn same(self, other: PgnTool) -> bool {
        match (self, other) {
            (PgnTool::Comment { .. }, PgnTool::Comment { .. }) | (PgnTool::GlyphBrush(_), PgnTool::GlyphBrush(_)) => true,
            (a, b) => a == b,
        }
    }
}

/// The rail's state, kept in [`ChessState`].
#[derive(Default)]
pub struct ToolState {
    /// The mode armed.
    pub tool: PgnTool,
    /// The keys taken from this frame's input for the panel, in order.
    keys: Vec<(Key, Modifiers)>,
    /// A rail was drawn since the last frame's input was read: only then are keys taken.
    rail_shown: bool,
    /// The member each group shows (the last used), by group id.
    shown: Vec<(&'static str, usize)>,
    /// The group button held long enough to open its tools: its click when let go is not an action.
    long_press: Option<egui::Id>,
    /// The popup was opened from the keyboard: its first tool takes the focus.
    focus_popup: bool,
    /// The app commands rail tools asked for ([`Kind::Command`]), in order, until the app runs them.
    commands: Vec<&'static str>,
    /// Glyph signs typed (T6).
    typed: Typed,
    /// The glyph last chosen in a glyph group (White's code; 0: none yet), which the glyph brush
    /// takes when it is armed (T15).
    last_glyph: u16,
    /// The glyph palette's window is open (U1): for the session, whatever game is shown.
    pub(crate) palette_open: bool,
    /// The window and pass the palette was last drawn in: once a pass.
    pub(crate) palette_drawn: Option<(egui::ViewportId, u64)>,
    /// Where the palette first opens: beside the rail's button, as last drawn.
    palette_anchor: Option<egui::Pos2>,
    /// Moves that left the game, told with Undo (plan A8 §7, V2a).
    pub deleted: crate::chess_deleted::DeleteWatch,
}

impl ToolState {
    /// The app commands the rail asked for since the last call, in order.
    pub(crate) fn take_commands(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.commands)
    }

    /// The member group `id` shows.
    fn shown(&self, id: &str) -> usize {
        self.shown.iter().find(|(g, _)| *g == id).map_or(0, |(_, i)| *i)
    }

    /// The glyph the brush takes when it is armed: the one last chosen in a glyph group, `!` until
    /// one is.
    fn brush_glyph(&self) -> u16 {
        if self.last_glyph == 0 { 1 } else { self.last_glyph }
    }

    /// The brush's glyph while the glyph brush is armed (White's code).
    pub(crate) fn brush(&self) -> Option<u16> {
        match self.tool {
            PgnTool::GlyphBrush(code) => Some(code),
            _ => None,
        }
    }

    fn show(&mut self, id: &'static str, member: usize) {
        match self.shown.iter_mut().find(|(g, _)| *g == id) {
            Some(slot) => slot.1 = member,
            None => self.shown.push((id, member)),
        }
    }
}

/// A key of the chess keymap: the key and the modifiers it is pressed with. `command` is Ctrl on
/// Windows and Linux and ⌘ on macOS (where Ctrl with the arrows belongs to the system).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChessKey {
    key: Key,
    command: bool,
    shift: bool,
    /// Alt (⌥ on macOS), alone (T15: Alt+↑ and Alt+↓ move a variation).
    alt: bool,
}

impl ChessKey {
    pub(crate) const fn plain(key: Key) -> ChessKey {
        ChessKey { key, command: false, shift: false, alt: false }
    }

    const fn shift(key: Key) -> ChessKey {
        ChessKey { key, command: false, shift: true, alt: false }
    }

    const fn command(key: Key) -> ChessKey {
        ChessKey { key, command: true, shift: false, alt: false }
    }

    const fn command_shift(key: Key) -> ChessKey {
        ChessKey { key, command: true, shift: true, alt: false }
    }

    const fn alt(key: Key) -> ChessKey {
        ChessKey { key, command: false, shift: false, alt: true }
    }

    /// Whether `key` pressed with `m` is this key: the same modifiers.
    fn matches(self, key: Key, m: Modifiers) -> bool {
        key == self.key && m.command == self.command && m.shift == self.shift && m.alt == self.alt && (self.command || !(m.ctrl || m.mac_cmd))
    }

    /// How the key is written on this platform ("Ctrl+Shift+↑", "⇧⌘↑").
    fn label(self, ctx: &egui::Context) -> String {
        let key = match self.key {
            Key::Escape => "Esc",
            Key::Delete => "Delete",
            Key::Backspace => "Backspace",
            Key::ArrowUp => "↑",
            Key::ArrowDown => "↓",
            Key::ArrowLeft => "←",
            Key::ArrowRight => "→",
            other => other.name(),
        };
        let label = crate::commands::shortcut_label(ctx, Shortcut { command: self.command, shift: self.shift, mac_ctrl: false, alt: false, key });
        match (self.alt, crate::commands::mac_shortcuts(ctx)) {
            (false, _) => label,
            (true, true) => format!("⌥{label}"),
            (true, false) => format!("{}+{label}", crate::i18n::key_name("Alt")),
        }
    }
}

/// What a tool acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// The move shown: a tool that acts on a variation picks the nearest it can act on.
    Current,
    /// The move a context menu was opened on.
    Move(NodeId),
    /// This variation's first move (its head), chosen in a group's list.
    Head(NodeId),
}

/// An action: what it does to the board.
pub(crate) type Run = fn(&mut BoardState, Target);

/// What a tool would act on for a move of a game (a context menu), or why it can't.
pub(crate) type OnMove = fn(&Game, NodeId) -> Result<Target, String>;

/// What a tool is.
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    /// Armed until Esc or another mode.
    Mode(PgnTool),
    /// Acts once.
    Action(Run),
    /// Runs a registered command of the app (it acts beyond the board): queued for the app,
    /// which runs it at the start of the next frame (`chess_window::frame`).
    Command(&'static str),
    /// Puts glyph `code` (White's code) on the move shown, or takes it off.
    Glyph(u16),
    /// Switches something of the chess panels on or off (Follow, `chess_source`).
    Switch(fn(&mut ChessState)),
    /// Undo (`true`) or Redo in the chess scope: the game shown's last change or the list of
    /// games' last delete or move, whichever is the newer (`ChessState::history_step`).
    History(bool),
}

/// A tool of the rail.
pub(crate) struct Tool {
    /// Stable id (groups remember their member by index; this names the tool in tests and logs).
    pub id: &'static str,
    /// Its Lucide icon (`icon_data.rs`).
    pub icon: &'static str,
    /// Its name in the current language (Undo's names the step it would undo).
    pub label: fn(&BoardState) -> String,
    /// The keys the scope takes for it; the first is shown in its name.
    pub keys: &'static [ChessKey],
    /// The registered command that runs it (Ctrl+K, automation): a `chess.*` command runs the
    /// tool through [`run_command`], and the palette shows the tool's key for it. A tool without
    /// keys of its own is run by the app's own key for its command (Undo and Redo follow the scope
    /// themselves, D12), and its name shows that command's shortcut.
    pub command: Option<&'static str>,
    pub kind: Kind,
    /// Whether it can act now.
    pub enabled: fn(&BoardState) -> bool,
    /// Whether it is on the rail at all here (its own window: not where windows are drawn
    /// inside the main one).
    pub visible: fn(&egui::Context) -> bool,
    /// The variations it can act on from a move (the move shown, or a context menu's), nearest
    /// first: a group and a move's menu list them when there are several to choose from.
    pub heads: Option<fn(&Game, NodeId) -> Vec<NodeId>>,
    /// In a move's context menu: what it would act on for that move, or why it can't.
    pub on_move: Option<OnMove>,
    /// Whether it shows as on: a panel it opened is open, a switch is on (modes are on while
    /// armed, glyphs while the move shown has them).
    pub on: Option<fn(&ChessState) -> bool>,
}

/// Tools that share one place on the rail.
pub(crate) struct Group {
    pub id: &'static str,
    pub label: fn() -> &'static str,
    /// Shift with this letter switches the member shown.
    pub letter: Option<Key>,
    pub members: &'static [Tool],
}

pub(crate) enum Entry {
    Tool(Tool),
    Group(Group),
}

/// The rail, top to bottom: sections, with a separator between them.
pub(crate) static RAIL: &[&[Entry]] = &[
    &[Entry::Tool(NAVIGATE), Entry::Tool(SETUP)],
    &[Entry::Group(STRUCTURE)],
    &[Entry::Group(crate::chess_capture::CAPTURE), Entry::Tool(crate::chess_capture::COMMENT), Entry::Tool(crate::chess_ocr_area::OCR_AREA)],
    &[
        Entry::Group(MOVE_GLYPHS),
        Entry::Group(EVALUATION_GLYPHS),
        Entry::Group(OTHER_GLYPHS),
        Entry::Tool(BRUSH),
        Entry::Tool(COMMENT),
        Entry::Tool(PALETTE),
    ],
    &[Entry::Tool(crate::chess_source::FOLLOW), Entry::Tool(crate::chess_source::SHOW)],
    &[Entry::Tool(UNDO), Entry::Tool(REDO)],
    &[Entry::Tool(WINDOW)],
];

const ESC: ChessKey = ChessKey::plain(Key::Escape);

fn has_moves(board: &BoardState) -> bool {
    board.illegal.is_none() && !board.game.is_empty()
}

const NAVIGATE: Tool = Tool {
    id: "navigate",
    icon: "mouse-pointer-2",
    label: |_| tl!("Navigate").to_owned(),
    keys: &[ESC],
    command: Some("chess.tool.navigate"),
    kind: Kind::Mode(PgnTool::Navigate),
    enabled: |_| true,
    heads: None,
    visible: |_| true,
    on_move: None,
    on: None,
};

/// Set up a position (T5c): a mode, so Esc or another mode leaves it, as Done and Cancel do.
const SETUP: Tool = Tool {
    id: "setup",
    icon: "layout-grid",
    label: |_| tl!("Set up a position").to_owned(),
    keys: &[],
    command: Some("chess.tool.setup"),
    kind: Kind::Mode(PgnTool::Setup),
    enabled: |_| true,
    heads: None,
    visible: |_| true,
    on_move: None,
    on: None,
};

const STRUCTURE: Group = Group {
    id: "structure",
    label: || tl!("Structure"),
    letter: Some(Key::S),
    members: &[
        Tool {
            id: "promote-one",
            icon: "chevron-up",
            label: |_| tl!("Promote one level").to_owned(),
            keys: &[ChessKey::command(Key::ArrowUp)],
            command: Some("chess.variation.promote_one"),
            kind: Kind::Action(|b, t| structure(b, Op::PromoteOne, t)),
            enabled: has_moves,
            heads: Some(|g, from| Op::PromoteOne.heads(g, from)),
            visible: |_| true,
            on_move: Some(|g, n| Op::PromoteOne.check(g, n)),
            on: None,
        },
        Tool {
            id: "make-main",
            icon: "badge-check",
            label: |_| tl!("Make this the main line").to_owned(),
            keys: &[ChessKey::command_shift(Key::ArrowUp)],
            command: Some("chess.variation.make_main"),
            kind: Kind::Action(|b, t| structure(b, Op::MakeMain, t)),
            enabled: has_moves,
            heads: None,
            visible: |_| true,
            on_move: Some(|g, n| Op::MakeMain.check(g, n)),
            on: None,
        },
        Tool {
            id: "demote",
            icon: "chevron-down",
            label: |_| tl!("Demote").to_owned(),
            keys: &[ChessKey::command(Key::ArrowDown)],
            command: Some("chess.variation.demote"),
            kind: Kind::Action(|b, t| structure(b, Op::Demote, t)),
            enabled: has_moves,
            heads: Some(|g, from| Op::Demote.heads(g, from)),
            visible: |_| true,
            on_move: Some(|g, n| Op::Demote.check(g, n)),
            on: None,
        },
        Tool {
            id: "delete-variation",
            icon: "trash-2",
            label: |_| tl!("Delete variation").to_owned(),
            // Not Backspace, which steps back (A8-W2: pressed to go back, it deleted the line).
            keys: &[ChessKey::plain(Key::Delete)],
            command: Some("chess.variation.delete"),
            kind: Kind::Action(|b, t| structure(b, Op::DeleteVariation, t)),
            enabled: has_moves,
            heads: Some(|g, from| Op::DeleteVariation.heads(g, from)),
            visible: |_| true,
            on_move: Some(|g, n| Op::DeleteVariation.check(g, n)),
            on: None,
        },
        Tool {
            id: "variation-up",
            icon: "arrow-up-down",
            label: |_| tl!("Move variation up").to_owned(),
            keys: &[ChessKey::alt(Key::ArrowUp)],
            command: Some("chess.variation.up"),
            kind: Kind::Action(|b, t| structure(b, Op::MoveUp, t)),
            enabled: has_moves,
            heads: Some(|g, from| Op::MoveUp.heads(g, from)),
            visible: |_| true,
            on_move: Some(|g, n| Op::MoveUp.check(g, n)),
            on: None,
        },
        Tool {
            id: "variation-down",
            icon: "arrow-up-down",
            label: |_| tl!("Move variation down").to_owned(),
            keys: &[ChessKey::alt(Key::ArrowDown)],
            command: Some("chess.variation.down"),
            kind: Kind::Action(|b, t| structure(b, Op::MoveDown, t)),
            enabled: has_moves,
            heads: Some(|g, from| Op::MoveDown.heads(g, from)),
            visible: |_| true,
            on_move: Some(|g, n| Op::MoveDown.check(g, n)),
            on: None,
        },
        Tool {
            id: "cut-after",
            icon: "scissors",
            label: |_| tl!("Cut after this move").to_owned(),
            keys: &[ChessKey::shift(Key::Delete)],
            command: Some("chess.moves.cut"),
            kind: Kind::Action(|b, t| structure(b, Op::CutAfter, t)),
            enabled: has_moves,
            heads: None,
            visible: |_| true,
            on_move: Some(|g, n| Op::CutAfter.check(g, n)),
            on: None,
        },
        // A threat (T15): the side to move passes, and what the other side would play is shown
        // after the null move, as books do (`12... --`).
        Tool {
            id: "threat",
            icon: "triangle-alert",
            label: |_| tl!("Threat (null move)").to_owned(),
            keys: &[],
            command: Some("chess.moves.null"),
            kind: Kind::Action(|b, t| structure(b, Op::Threat, t)),
            enabled: |b| b.illegal.is_none(),
            heads: None,
            visible: |_| true,
            on_move: Some(|g, n| Op::Threat.check(g, n)),
            on: None,
        },
    ],
};

/// A glyph's tool: named and shown by its glyph (`chess_annotate`), off without a move shown.
const fn glyph(id: &'static str, code: u16, command: Option<&'static str>) -> Tool {
    Tool {
        id,
        icon: "",
        label: |_| String::new(),
        keys: &[],
        command,
        kind: Kind::Glyph(code),
        enabled: chess_annotate::has_move,
        visible: |_| true,
        heads: None,
        on_move: None,
        on: None,
    }
}

/// The glyph groups (spec §4.2, §4.4), in `chess_annotate`'s order. The six move-quality glyphs
/// are registered commands too.
const MOVE_GLYPHS: Group = Group {
    id: "glyphs-move",
    label: || tl!("Move glyphs"),
    letter: None,
    members: &[
        glyph("nag-1", 1, Some("chess.nag.1")),
        glyph("nag-2", 2, Some("chess.nag.2")),
        glyph("nag-3", 3, Some("chess.nag.3")),
        glyph("nag-4", 4, Some("chess.nag.4")),
        glyph("nag-5", 5, Some("chess.nag.5")),
        glyph("nag-6", 6, Some("chess.nag.6")),
        glyph("nag-7", 7, None),
        glyph("nag-146", 146, None),
    ],
};

const EVALUATION_GLYPHS: Group = Group {
    id: "glyphs-evaluation",
    label: || tl!("Evaluation glyphs"),
    letter: None,
    members: &[
        glyph("nag-10", 10, None),
        glyph("nag-13", 13, None),
        glyph("nag-14", 14, None),
        glyph("nag-15", 15, None),
        glyph("nag-16", 16, None),
        glyph("nag-17", 17, None),
        glyph("nag-18", 18, None),
        glyph("nag-19", 19, None),
        glyph("nag-44", 44, None),
    ],
};

const OTHER_GLYPHS: Group = Group {
    id: "glyphs-other",
    label: || tl!("Other glyphs"),
    letter: None,
    members: &[
        glyph("nag-32", 32, None),
        glyph("nag-36", 36, None),
        glyph("nag-40", 40, None),
        glyph("nag-132", 132, None),
        glyph("nag-140", 140, None),
        glyph("nag-141", 141, None),
        glyph("nag-22", 22, None),
        glyph("nag-138", 138, None),
        glyph("nag-142", 142, None),
        glyph("nag-143", 143, None),
        glyph("nag-145", 145, None),
    ],
};

/// The glyph brush (T15): a mode, armed with the glyph last chosen in a glyph group (`!` until
/// one is); while it is armed, a glyph chosen in a group becomes the brush's.
const BRUSH: Tool = Tool {
    id: "glyph-brush",
    icon: "brush",
    label: |_| tl!("Glyph brush").to_owned(),
    keys: &[ChessKey::plain(Key::B)],
    command: Some("chess.tool.glyph_brush"),
    kind: Kind::Mode(PgnTool::GlyphBrush(0)),
    enabled: has_moves,
    visible: |_| true,
    heads: None,
    on_move: None,
    on: None,
};

const COMMENT: Tool = Tool {
    id: "comment",
    icon: "message-square-quote",
    label: |_| tl!("Move comment").to_owned(),
    keys: &[ChessKey::plain(Key::Enter)],
    command: Some("chess.comment.edit"),
    kind: Kind::Action(|b, _| b.toggle_comment_editor()),
    enabled: |b| b.illegal.is_none(),
    visible: |_| true,
    heads: None,
    on_move: None,
    on: Some(|c| c.board().comment_open),
};

const PALETTE: Tool = Tool {
    id: "palette",
    icon: "type",
    label: |_| tl!("Glyph palette").to_owned(),
    keys: &[ChessKey::plain(Key::G)],
    command: Some("chess.palette.toggle"),
    kind: Kind::Switch(|c| c.tools.palette_open = !c.tools.palette_open),
    enabled: |_| true,
    visible: |_| true,
    heads: None,
    on_move: None,
    on: Some(|c| c.tools.palette_open),
};

const UNDO: Tool = Tool {
    id: "undo",
    icon: "undo-2",
    label: |b| b.command_text(true).unwrap_or_else(|| tl!("Undo").to_owned()),
    keys: &[],
    command: Some("edit.undo"),
    kind: Kind::History(true),
    enabled: |b| b.history.undo_edit().is_some(),
    heads: None,
    visible: |_| true,
    on_move: None,
    on: None,
};

const REDO: Tool = Tool {
    id: "redo",
    icon: "redo-2",
    label: |b| b.command_text(false).unwrap_or_else(|| tl!("Redo").to_owned()),
    keys: &[],
    command: Some("edit.redo"),
    kind: Kind::History(false),
    enabled: |b| b.history.redo_edit().is_some(),
    heads: None,
    visible: |_| true,
    on_move: None,
    on: None,
};

/// The board and the chess panel's Diagrams and Text tabs in their own window, or back (T13a,
/// T13b): only where separate windows can be opened (not on the web, nor in tests, where windows
/// are drawn inside the main one).
const WINDOW: Tool = Tool {
    id: "window",
    icon: "app-window",
    label: |_| tl!("Chess in its own window").to_owned(),
    keys: &[],
    command: Some("chess.window.detach"),
    kind: Kind::Command("chess.window.detach"),
    enabled: |_| true,
    visible: |ctx| !ctx.embed_viewports(),
    heads: None,
    on_move: None,
    on: None,
};

/// Moving about the game, and Esc: the keys the panel takes besides the tools'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Escape,
    Back,
    Forward,
    /// The variation before the move shown, at its fork.
    Up,
    /// The variation after it.
    Down,
    Start,
    End,
    /// The end of the main line, from anywhere (plan A8 V3).
    MainEnd,
    /// The first move of the previous or the next variation in reading order (plan HC11).
    PreviousVariation,
    NextVariation,
}

const NAV_KEYS: [(ChessKey, Nav); 11] = [
    (ESC, Nav::Escape),
    (ChessKey::plain(Key::ArrowLeft), Nav::Back),
    // Backspace goes back a move, as it goes back in a browser; it deletes nothing.
    (ChessKey::plain(Key::Backspace), Nav::Back),
    (ChessKey::plain(Key::ArrowRight), Nav::Forward),
    (ChessKey::plain(Key::ArrowUp), Nav::Up),
    (ChessKey::plain(Key::ArrowDown), Nav::Down),
    (ChessKey::plain(Key::Home), Nav::Start),
    (ChessKey::plain(Key::End), Nav::End),
    (ChessKey::shift(Key::End), Nav::MainEnd),
    (ChessKey::plain(Key::PageUp), Nav::PreviousVariation),
    (ChessKey::plain(Key::PageDown), Nav::NextVariation),
];

/// Every tool of the rail, groups' members included.
fn tools() -> impl Iterator<Item = &'static Tool> {
    RAIL.iter().flat_map(|section| section.iter()).flat_map(|entry| match entry {
        Entry::Tool(tool) => std::slice::from_ref(tool).iter(),
        Entry::Group(group) => group.members.iter(),
    })
}

fn groups() -> impl Iterator<Item = &'static Group> {
    RAIL.iter().flat_map(|section| section.iter()).filter_map(|entry| match entry {
        Entry::Group(group) => Some(group),
        Entry::Tool(_) => None,
    })
}

/// Whether the scope takes `key` pressed with `m`.
pub(crate) fn scope_key(key: Key, m: Modifiers) -> bool {
    NAV_KEYS.iter().any(|(k, _)| k.matches(key, m))
        || tools().any(|tool| tool.keys.iter().any(|k| k.matches(key, m)))
        || groups().any(|group| group.letter.is_some_and(|letter| ChessKey::shift(letter).matches(key, m)))
}

/// The key scope (§4.2), called from `raw_input_hook` before egui reads the frame's input: take
/// the chess keys out of `raw` for the panel when the scope is on. Nothing is taken when
/// `blocked` (a modal dialog or the command palette is open), while a text field has the
/// keyboard (as the last frame left it), while a popup or menu is open, or when no rail was drawn
/// since the last call. Only presses are taken: a release that egui sees without its press is
/// harmless, while a press seen without its release would leave the key held down. A `Paste` is
/// taken for a capture (`chess_capture`) whenever the scope is on, rail or not.
pub(crate) fn take_scope_keys(chess: &mut ChessState, ctx: &egui::Context, raw: &mut egui::RawInput, blocked: bool) {
    chess.tools.keys.clear();
    chess.games.drop_list_keys();
    let shown = std::mem::take(&mut chess.tools.rail_shown);
    // Enter applies a capture waiting for the user, before anything else can use it (T7b).
    crate::chess_capture::take_enter(chess, &mut raw.events, blocked || ctx.text_edit_focused());
    // The diagram editor's board and the Diagram tab, while they have the keyboard (plan A8, P3c).
    let free = !blocked && !ctx.text_edit_focused() && !egui::Popup::is_any_open(ctx);
    crate::chess_diagram_keys::take_keys(chess, ctx, &mut raw.events, free);
    // A focused row of the Diagrams list: ↑ ↓ Home End go to another diagram (plan A8, P2a1).
    crate::chess_list::take_keys(&mut chess.list, ctx, &mut raw.events, free);
    // A focused row of the games list (T14) has its own keys, scope or not: its Delete deletes
    // no variation (nor a page or an annotation).
    if !blocked && !ctx.text_edit_focused() && !egui::Popup::is_any_open(ctx) && chess.games.list_has_focus(ctx) {
        chess.games.take_list_keys(&mut raw.events);
        if chess.in_scope() {
            crate::chess_capture::take_paste(chess, &mut raw.events);
        }
        return;
    }
    if blocked || !chess.in_scope() || ctx.text_edit_focused() || egui::Popup::is_any_open(ctx) {
        return;
    }
    // A paste goes to the game whether or not the rail is on screen (T7a).
    crate::chess_capture::take_paste(chess, &mut raw.events);
    if !shown {
        return;
    }
    // A focused button is pressed with Enter: Enter is the panel's only while nothing has focus.
    let focused = ctx.memory(|m| m.focused().is_some());
    let tools = &mut chess.tools;
    raw.events.retain(|event| match event {
        Event::Key { key, pressed: true, modifiers, .. } if scope_key(*key, *modifiers) && !(*key == Key::Enter && focused) => {
            if tools.keys.len() < MAX_KEYS {
                tools.keys.push((*key, *modifiers));
            }
            false
        }
        Event::Text(text) if chess_annotate::is_glyph_text(text) => {
            text.chars().for_each(|c| tools.typed.take(c));
            false
        }
        _ => true,
    });
}

/// The main window's key scope, set aside while the chess board's own window draws its rail.
pub(crate) struct MainScope {
    rail_shown: bool,
    keys: Vec<(Key, Modifiers)>,
}

/// The chess board's own window (T13a), at the start of its pass: its keys are its own (its
/// input never reaches `raw_input_hook`, which is the main window's), so the chess keys are
/// taken from it for its rail, unless a text field has the keyboard or a popup is open. The main
/// window's scope is set aside, and put back by [`end_window_keys`]: a rail drawn in the window
/// takes no keys from the main one.
pub(crate) fn take_window_keys(chess: &mut ChessState, ctx: &egui::Context) -> MainScope {
    let main = MainScope { rail_shown: chess.tools.rail_shown, keys: std::mem::take(&mut chess.tools.keys) };
    if !ctx.text_edit_focused() && !egui::Popup::is_any_open(ctx) {
        // A focused row of the window's games list has its own keys (T14), as in the main window.
        let listed = chess.games.list_has_focus(ctx);
        // A paste in the chess window goes to the game (T7a), and Enter applies a capture
        // waiting for the user before the window's rail can use it (T7b).
        ctx.input_mut(|i| {
            crate::chess_capture::take_enter(chess, &mut i.events, false);
            crate::chess_capture::take_paste(chess, &mut i.events);
            if listed {
                chess.games.take_list_keys(&mut i.events);
            }
        });
        if listed {
            return main;
        }
        // As in the main window's scope: Enter only while nothing has focus, and typed glyphs.
        let focused = ctx.memory(|m| m.focused().is_some());
        let tools = &mut chess.tools;
        ctx.input_mut(|i| {
            i.events.retain(|event| match event {
                Event::Key { key, pressed: true, modifiers, .. } if scope_key(*key, *modifiers) && !(*key == Key::Enter && focused) => {
                    if tools.keys.len() < MAX_KEYS {
                        tools.keys.push((*key, *modifiers));
                    }
                    false
                }
                Event::Text(text) if chess_annotate::is_glyph_text(text) => {
                    text.chars().for_each(|c| tools.typed.take(c));
                    false
                }
                _ => true,
            });
        });
    }
    main
}

/// The end of the chess window's pass: the main window's key scope as it was.
pub(crate) fn end_window_keys(chess: &mut ChessState, main: MainScope) {
    chess.tools.rail_shown = main.rail_shown;
    chess.tools.keys = main.keys;
}

/// A tool's label in the current language: a glyph's is the glyph's name.
fn label(tool: &Tool, board: &BoardState) -> String {
    match tool.kind {
        Kind::Glyph(code) => chess_annotate::nag_name(code).unwrap_or_default().to_owned(),
        _ => (tool.label)(board),
    }
}

/// A tool's label where the whole chess state is known: Undo's and Redo's name the step they
/// would take, of the game or of its list.
fn tool_label(tool: &Tool, chess: &ChessState) -> String {
    match tool.kind {
        Kind::History(undo) => chess.games.step_text(undo).unwrap_or_else(|| if undo { tl!("Undo") } else { tl!("Redo") }.to_owned()),
        _ => label(tool, chess.board()),
    }
}

/// Whether a tool can act now: Undo and Redo when the game or its list has a step to take.
fn can_act(tool: &Tool, chess: &ChessState) -> bool {
    match tool.kind {
        Kind::History(undo) => chess.games.can_step(undo),
        // While a text field had the keyboard, a glyph with a sign types it there, move or not.
        Kind::Glyph(code) => (tool.enabled)(chess.board()) || chess_annotate::glyph_typed(chess.board(), code).is_some(),
        _ => (tool.enabled)(chess.board()),
    }
}

/// Whether a tool shows as on: the mode armed, a glyph the move shown has, a panel it opened, a
/// switch on.
fn is_on(tool: &Tool, chess: &ChessState) -> bool {
    match tool.kind {
        Kind::Mode(m) => m.same(chess.tools.tool),
        // With the brush armed, its glyph shows as on; else the glyphs of the move shown.
        Kind::Glyph(code) => match chess.tools.brush() {
            Some(brush) => brush == code,
            None => chess_annotate::has_glyph(chess.board(), code),
        },
        Kind::Action(_) | Kind::Command(_) | Kind::Switch(_) | Kind::History(_) => tool.on.is_some_and(|on| on(chess)),
    }
}

/// A tool's button on the rail: its icon, or for a glyph its sign as the fonts can show it (or
/// painted, where none can: `⌓`).
fn tool_button(ui: &mut egui::Ui, tool: &Tool, on: bool, name: &str) -> egui::Response {
    match tool.kind {
        Kind::Glyph(code) => {
            let ctx = ui.ctx().clone();
            match chess_annotate::seen(code, |s| chess_annotate::drawable(&ctx, s)) {
                chess_annotate::Seen::Icon(icon) => {
                    icons::painted_button(ui, BUTTON, on, name, |painter, rect, tint| chess_annotate::paint_icon(painter, rect, tint, icon))
                }
                chess_annotate::Seen::Text(face) => icons::text_button(ui, &face, BUTTON, on, name),
                chess_annotate::Seen::Name(_) => icons::text_button(ui, &chess_annotate::face(code, |_| true), BUTTON, on, name),
            }
        }
        _ => icons::button(ui, tool.icon, BUTTON, on, name),
    }
}

/// The name a tool is known by: its label and its key ("Demote (Ctrl+↓)").
fn name(ctx: &egui::Context, tool: &Tool, chess: &ChessState) -> String {
    let label = tool_label(tool, chess);
    let key = match (tool.keys.first(), tool.command) {
        (Some(k), _) => k.label(ctx),
        (None, Some(id)) => crate::commands::command_shortcut_label(ctx, id),
        (None, None) => String::new(),
    };
    if key.is_empty() { label } else { format!("{label} ({key})") }
}

/// The rail tool registered command `id` runs (only `chess.*` commands are the rail's), and the
/// group it belongs to with its place there. A tool that asks the app for a command of its own
/// ([`Kind::Command`], the chess window) is not one: the app runs and enables that command.
fn command_tool(id: &str) -> Option<(&'static Tool, Option<(&'static str, usize)>)> {
    if !id.starts_with("chess.") {
        return None;
    }
    let runs = |tool: &Tool| tool.command == Some(id) && !matches!(tool.kind, Kind::Command(_));
    RAIL.iter().flat_map(|section| section.iter()).find_map(|entry| match entry {
        Entry::Tool(tool) => runs(tool).then_some((tool, None)),
        Entry::Group(group) => group.members.iter().enumerate().find(|(_, t)| runs(t)).map(|(i, t)| (t, Some((group.id, i)))),
    })
}

/// Whether registered command `id` is a rail tool's.
pub(crate) fn is_tool_command(id: &str) -> bool {
    command_tool(id).is_some()
}

/// Whether the rail tool of command `id` can act now (`None`: not a rail tool's command).
pub(crate) fn command_enabled(chess: &ChessState, id: &str) -> Option<bool> {
    command_tool(id).map(|(tool, _)| can_act(tool, chess))
}

/// The chess panel's key for the rail tool of command `id`, as written on this platform: the
/// palette shows it, since the registry has no shortcut for these commands.
pub(crate) fn command_key(ctx: &egui::Context, id: &str) -> Option<String> {
    command_tool(id).and_then(|(tool, _)| tool.keys.first()).map(|k| k.label(ctx))
}

/// Run the rail tool of command `id` as its button would, on the move shown: the same action, so
/// the rail, its keys, its menu and the command never differ. A group shows the tool after.
/// Returns whether `id` is a rail tool's command and it could act.
pub(crate) fn run_command(chess: &mut ChessState, id: &str) -> bool {
    let Some((tool, place)) = command_tool(id) else { return false };
    if !can_act(tool, chess) {
        return false;
    }
    if let Some((group, i)) = place {
        chess.tools.show(group, i);
    }
    activate(chess, tool, Target::Current);
    true
}

/// Use `tool` on `target`: arm a mode, or act.
fn activate(chess: &mut ChessState, tool: &Tool, target: Target) {
    match tool.kind {
        // The brush takes the glyph last chosen; arming it turns the chess scope on (its keys, and
        // Undo on the game).
        Kind::Mode(PgnTool::GlyphBrush(_)) => {
            chess.tools.tool = PgnTool::GlyphBrush(chess.tools.brush_glyph());
            chess.enter_scope();
        }
        Kind::Mode(mode) => crate::chess_capture::arm(chess, mode),
        Kind::Action(run) => run(chess.board_mut(), target),
        Kind::Command(id) => {
            if chess.tools.commands.len() < MAX_COMMANDS && !chess.tools.commands.contains(&id) {
                chess.tools.commands.push(id);
            }
        }
        Kind::Glyph(code) => {
            chess.tools.last_glyph = code;
            // With the brush armed, a glyph chosen is the brush's glyph, not a change to the move.
            // Else, as in the palette, it goes into the text field that had the keyboard (the
            // correction field of a capture, a comment, FEN or PGN) when it has a sign, or on the
            // move shown.
            if chess.tools.brush().is_some() {
                chess.tools.tool = PgnTool::GlyphBrush(code);
            } else {
                chess_annotate::pick_glyph(chess.board_mut(), code);
            }
        }
        Kind::Switch(flip) => flip(chess),
        Kind::History(undo) => {
            chess.history_step(undo);
        }
    }
}

/// The chess board panel with the rail glued to its left edge: the rail takes the full height
/// (outside `body`, so it does not scroll with it where the space is bounded), then `body` draws
/// the panel in the rest. The keys the scope took this frame are handled between the two.
pub(crate) fn with_rail<R>(ui: &mut egui::Ui, chess: &mut ChessState, body: impl FnOnce(&mut egui::Ui, &mut ChessState) -> R) -> R {
    let t = Tokens::get(ui.ctx());
    chess.tools.rail_shown = true;
    let area = ui.available_rect_before_wrap();
    // In a scroll area the height is not bounded: the rail is as tall as it needs.
    let bounded = area.height().is_finite();
    let rail_rect = Rect::from_min_size(area.min, vec2(RAIL_WIDTH, if bounded { area.height() } else { 4096.0 }));
    let mut rail_ui = ui.new_child(UiBuilder::new().id_salt("chess-rail").max_rect(rail_rect).layout(Layout::top_down(Align::Center)));
    let focused_group = rail(&mut rail_ui, &t, chess);
    for (key, m) in std::mem::take(&mut chess.tools.keys) {
        handle_key(ui.ctx(), chess, key, m, focused_group);
    }
    typed_glyphs(ui, chess);
    // Moves that left the game (the keys just handled, or the panel last frame): how many, and
    // Undo, in a strip at the top of the panel that pushes it down rather than cover it.
    let game = chess.games.scope_stamp().0;
    if let Some(deleted) = chess.tools.deleted.see(game, chess.games.board()) {
        // The game as it was goes into the document's trash, which outlives Undo (V5).
        chess.games.collection_mut().trash_deleted(deleted);
    }
    let mut top = area.min.y;
    if chess.tools.deleted.text().is_some() {
        let strip = Rect::from_min_max(pos2(area.min.x + RAIL_WIDTH + 5.0, area.min.y + 4.0), pos2(area.max.x - 4.0, area.max.y));
        let mut notice_ui = ui.new_child(UiBuilder::new().id_salt("chess-deleted").max_rect(strip).layout(Layout::top_down(Align::Min)));
        if chess.tools.deleted.show(&mut notice_ui, &t) {
            chess.board_mut().undo();
        }
        top = notice_ui.min_rect().bottom() + 4.0;
    }
    let body_rect = Rect::from_min_max(pos2(area.min.x + RAIL_WIDTH + 1.0, top), area.max);
    let mut body_ui = ui.new_child(UiBuilder::new().id_salt("chess-rail-body").max_rect(body_rect).layout(*ui.layout()));
    let out = body(&mut body_ui, chess);
    // The glyph palette floats over the window the rail is in (U1).
    let anchor = chess.tools.palette_anchor;
    chess_annotate::palette_window(ui.ctx(), chess, anchor);
    let bottom = if bounded { area.bottom() } else { rail_ui.min_rect().bottom().max(body_ui.min_rect().bottom()) };
    ui.painter().vline(area.min.x + RAIL_WIDTH + 0.5, area.min.y..=bottom, Stroke::new(1.0, t.divider));
    ui.advance_cursor_after_rect(Rect::from_min_max(area.min, pos2(area.max.x, bottom)));
    out
}

/// The glyph signs typed since the last frame, and the one waiting once its wait is over.
fn typed_glyphs(ui: &egui::Ui, chess: &mut ChessState) {
    let now = ui.input(|i| i.time);
    let mut codes = Vec::new();
    for c in std::mem::take(&mut chess.tools.typed.incoming) {
        codes.extend(chess.tools.typed.push(c, now));
    }
    codes.extend(chess.tools.typed.tick(now));
    for code in codes {
        chess_annotate::typed_glyph(chess.board_mut(), code);
    }
    if chess.tools.typed.waiting() {
        ui.ctx().request_repaint_after(Duration::from_secs_f64(TYPED_WAIT));
    }
}

/// Draw the rail. Returns the popup id of the group whose button has the keyboard focus, if any.
fn rail(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) -> Option<egui::Id> {
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.add_space(6.0);
    let mut focused = None;
    let mut any_focus = false;
    let ctx = ui.ctx().clone();
    let visible = |entry: &Entry| match entry {
        Entry::Tool(tool) => (tool.visible)(&ctx),
        Entry::Group(_) => true,
    };
    for (i, section) in RAIL.iter().filter(|section| section.iter().any(visible)).enumerate() {
        if i > 0 {
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 9.0), Sense::hover());
            ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, t.divider));
        }
        for entry in section.iter().filter(|entry| visible(entry)) {
            match entry {
                Entry::Tool(tool) => {
                    let name = name(ui.ctx(), tool, chess);
                    let on = is_on(tool, chess);
                    let enabled = can_act(tool, chess);
                    let r = ui.push_id(tool.id, |ui| ui.add_enabled_ui(enabled, |ui| tool_button(ui, tool, on, &name)).inner).inner;
                    // Off: why, as a move's menu says it (Show in PDF: no place on the page).
                    let game = &chess.board().game;
                    let r = match tool.on_move.map(|on_move| on_move(game, game.current())) {
                        Some(Err(why)) if !enabled => r.on_disabled_hover_text(why),
                        _ => r,
                    };
                    any_focus |= r.has_focus();
                    if tool.id == PALETTE.id {
                        chess.tools.palette_anchor = Some(r.rect.right_top() + vec2(8.0, 0.0));
                    }
                    if r.clicked() {
                        activate(chess, tool, Target::Current);
                    }
                }
                Entry::Group(group) => {
                    let (r, popup) = ui.push_id(group.id, |ui| group_button(ui, t, chess, group)).inner;
                    if r.has_focus() {
                        any_focus = true;
                        focused = Some(popup);
                    }
                }
            }
        }
    }
    // The keyboard in the rail puts Undo on the game, as a click in the panel does (D12).
    if any_focus {
        chess.enter_scope();
    }
    focused
}

/// A group's button, showing its last used member, and its popup with all of them. Returns the
/// button's response and the popup's id.
fn group_button(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, group: &'static Group) -> (egui::Response, egui::Id) {
    let ctx = ui.ctx().clone();
    let shown = chess.tools.shown(group.id).min(group.members.len().saturating_sub(1));
    let tool = group.members.get(shown);
    let name = tool.map(|tool| name(&ctx, tool, chess)).unwrap_or_default();
    let on = tool.is_some_and(|tool| is_on(tool, chess));
    let enabled = group.members.iter().any(|m| can_act(m, chess));
    let r = ui
        .add_enabled_ui(enabled, |ui| match tool {
            Some(tool) => tool_button(ui, tool, on, &name),
            None => icons::button(ui, "square", BUTTON, false, &name),
        })
        .inner;
    let popup = r.id.with("tools");
    let key = group.letter.map(|l| ChessKey::shift(l).label(&ctx)).unwrap_or_default();
    let r = r.on_hover_text(crate::i18n::fmt(
        tl!("More {group} tools: right-click, hold, or press Enter. {key} switches between them."),
        &[("group", (group.label)()), ("key", &key)],
    ));
    // The triangle that says there is more.
    let corner = r.rect.right_bottom() + vec2(-3.0, -3.0);
    let ink = if enabled { t.text_muted } else { t.text_faint };
    ui.painter().add(egui::Shape::convex_polygon(vec![corner, corner + vec2(-5.0, 0.0), corner + vec2(0.0, -5.0)], ink, Stroke::NONE));

    let down = r.is_pointer_button_down_on();
    if down {
        ctx.request_repaint_after(Duration::from_secs_f64(LONG_PRESS));
    }
    let held = down && ui.input(|i| i.pointer.press_start_time().is_some_and(|start| i.time - start >= LONG_PRESS));
    let mut open = false;
    if held && chess.tools.long_press != Some(r.id) {
        chess.tools.long_press = Some(r.id);
        open = true;
    }
    let after_long_press = chess.tools.long_press == Some(r.id);
    // Let go after a long press: that click (elsewhere, for the popup) must not close it.
    let letting_go = after_long_press && !down;
    if letting_go {
        chess.tools.long_press = None;
    }
    let enter = r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if r.secondary_clicked() || enter {
        open = true;
        chess.tools.focus_popup = enter;
    }
    if r.clicked()
        && !after_long_press
        && !enter
        && let Some(tool) = tool
        && can_act(tool, chess)
    {
        activate(chess, tool, Target::Current);
    }
    let behavior = if letting_go { egui::PopupCloseBehavior::IgnoreClicks } else { egui::PopupCloseBehavior::CloseOnClickOutside };
    let mut chosen = None;
    egui::Popup::from_response(&r)
        .id(popup)
        .open_memory(open.then_some(egui::SetOpenCommand::Bool(true)))
        .close_behavior(behavior)
        .align(egui::RectAlign::RIGHT_START)
        .gap(4.0)
        .show(|ui| {
            ui.set_min_width(200.0);
            ui.label(egui::RichText::new((group.label)()).small().color(t.text_muted));
            for (i, tool) in group.members.iter().enumerate() {
                let mut glyph_icon = None;
                let label = tool_label(tool, chess);
                let key = tool.keys.first().map(|k| k.label(&ctx)).unwrap_or_default();
                let button = match tool.kind {
                    // A glyph: its sign, then its name; ticked while the move shown has it.
                    Kind::Glyph(code) => {
                        // A painted sign is drawn over the room its text would take.
                        let face = match chess_annotate::seen(code, |s| chess_annotate::drawable(&ctx, s)) {
                            chess_annotate::Seen::Text(face) => face,
                            chess_annotate::Seen::Icon(icon) => {
                                glyph_icon = Some(icon);
                                String::new()
                            }
                            chess_annotate::Seen::Name(_) => chess_annotate::face(code, |_| true),
                        };
                        let mark = if is_on(tool, chess) { "✓" } else { "" };
                        egui::Button::new(egui::RichText::new(format!("{face:<4} {label}"))).shortcut_text(mark)
                    }
                    _ => egui::Button::image_and_text(icons::image(tool.icon, 16.0, t.icon), label.as_str()).shortcut_text(key),
                };
                let b = ui.add_enabled(can_act(tool, chess), button);
                if let Some(icon) = glyph_icon.take() {
                    let at = b.rect.left_center() + egui::vec2(ui.spacing().button_padding.x + 7.0, 0.0);
                    let color = ui.style().interact(&b).fg_stroke.color;
                    chess_annotate::paint_icon(ui.painter(), egui::Rect::from_center_size(at, egui::vec2(13.0, 13.0)), color, icon);
                }
                b.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, b.enabled(), i == shown, &label));
                if i == 0 && std::mem::take(&mut chess.tools.focus_popup) {
                    b.request_focus();
                }
                if b.clicked() {
                    chosen = Some((i, Target::Current));
                    ui.close();
                }
                // Several variations it could act on: each is a choice ("Demote: 20.fxg6 · 18…Qd8").
                let game = &chess.board().game;
                if can_act(tool, chess)
                    && let Some(head) = head_choices(ui, t, game, tool, &label, game.current())
                {
                    chosen = Some((i, Target::Head(head)));
                    ui.close();
                }
            }
        });
    if let Some((i, target)) = chosen
        && let Some(tool) = group.members.get(i)
    {
        chess.tools.show(group.id, i);
        activate(chess, tool, target);
    }
    (r, popup)
}

/// Under a tool's entry in a group's popup or a move's menu: when the tool could act on several
/// variations on the way to move `from` (§4.5), a row naming them nearest first, each a button
/// ("Demote: 20.fxg6 · 18…Qd8"). Returns the head chosen.
fn head_choices(ui: &mut egui::Ui, t: &Tokens, game: &Game, tool: &Tool, label: &str, from: NodeId) -> Option<NodeId> {
    let heads = tool.heads.map(|heads| heads(game, from)).unwrap_or_default();
    if heads.len() < 2 {
        return None;
    }
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(22.0);
        ui.label(egui::RichText::new(format!("{label}:")).small().color(t.text_muted));
        for (k, head) in heads.iter().enumerate() {
            if k > 0 {
                ui.label(egui::RichText::new("·").small().color(t.text_faint));
            }
            let mv = move_name(game, *head);
            let h = ui.small_button(&mv);
            let full = format!("{label}: {mv}");
            h.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &full));
            if h.clicked() {
                chosen = Some(*head);
            }
        }
    });
    chosen
}

/// A key the scope took: Esc and moving about first, then a group's letter, then a tool's key.
/// `focused_group` is the popup of the group whose button has the keyboard focus (Down opens it).
fn handle_key(ctx: &egui::Context, chess: &mut ChessState, key: Key, m: Modifiers, focused_group: Option<egui::Id>) {
    if let Some(nav) = NAV_KEYS.iter().find(|(k, _)| k.matches(key, m)).map(|(_, nav)| *nav) {
        match (nav, focused_group) {
            (Nav::Down, Some(popup)) => {
                chess.tools.focus_popup = true;
                egui::Popup::open_id(ctx, popup);
            }
            _ => navigate(chess, nav),
        }
        return;
    }
    if let Some(group) = groups().find(|g| g.letter.is_some_and(|letter| ChessKey::shift(letter).matches(key, m))) {
        let next = chess.tools.shown(group.id).saturating_add(1) % group.members.len().max(1);
        chess.tools.show(group.id, next);
        // A group of modes (Capture) arms the tool it switched to.
        if let Some(tool) = group.members.get(next).filter(|tool| matches!(tool.kind, Kind::Mode(_)) && can_act(tool, chess)) {
            activate(chess, tool, Target::Current);
        }
        return;
    }
    for entry in RAIL.iter().flat_map(|section| section.iter()) {
        let (group, members) = match entry {
            Entry::Tool(tool) => (None, std::slice::from_ref(tool)),
            Entry::Group(group) => (Some(group.id), group.members),
        };
        if let Some((i, tool)) = members.iter().enumerate().find(|(_, tool)| tool.keys.iter().any(|k| k.matches(key, m))) {
            if can_act(tool, chess) {
                if let Some(id) = group {
                    chess.tools.show(id, i);
                }
                activate(chess, tool, Target::Current);
            }
            return;
        }
    }
}

/// Esc, the arrows, Home and End.
fn navigate(chess: &mut ChessState, nav: Nav) {
    // A step forward that asked which reply takes the keys first (plan A8 V3).
    {
        use crate::chess_moves_tree::{ChooseKey, choose_key};
        let key = match nav {
            Nav::Up => Some(ChooseKey::Up),
            Nav::Down => Some(ChooseKey::Down),
            Nav::Forward => Some(ChooseKey::Enter),
            Nav::Escape => Some(ChooseKey::Main),
            Nav::Back => Some(ChooseKey::Leave),
            _ => None,
        };
        if let Some(key) = key
            && choose_key(chess.board_mut(), key)
        {
            return;
        }
    }
    if nav == Nav::Escape {
        // In order: the capture preview closes, then the comment editor and the glyph palette,
        // then an armed tool goes back to Navigate, then the piece picked up (or the pawn waiting
        // for its piece) is put down.
        if crate::chess_capture::escape_preview(chess) {
            return;
        }
        let palette = std::mem::take(&mut chess.tools.palette_open);
        let board = chess.board_mut();
        if board.comment_open || palette {
            board.comment_open = false;
        } else if chess.tools.tool != PgnTool::Navigate {
            chess.tools.tool = PgnTool::Navigate;
        } else {
            let board = chess.board_mut();
            board.selected = None;
            board.promotion = None;
        }
        return;
    }
    let board = chess.board_mut();
    if board.illegal.is_some() {
        return;
    }
    board.selected = None;
    board.promotion = None;
    match nav {
        Nav::Back => board.step(false),
        Nav::Forward => board.step(true),
        Nav::Start => board.game.go_to(0),
        Nav::End => board.game.go_to(usize::MAX),
        Nav::MainEnd => board.game.go_to_main_end(),
        Nav::Up | Nav::Down => {
            if let Some(node) = sibling(&board.game, board.game.current(), nav == Nav::Down) {
                board.game.go_to_node(node);
            }
        }
        Nav::PreviousVariation | Nav::NextVariation => {
            let current = board.game.current();
            let to = if nav == Nav::NextVariation { board.game.next_variation(current) } else { board.game.previous_variation(current) };
            if let Some(node) = to {
                board.game.go_to_node(node);
            }
        }
        Nav::Escape => {}
    }
}

/// The reply after (`next`) or before move `node` among its parent's replies: the next or the
/// previous variation at its fork. `None` at either end and for the start.
fn sibling(game: &Game, node: NodeId, next: bool) -> Option<NodeId> {
    let mut path = game.path_of(node)?.indices().to_vec();
    let last = path.last_mut()?;
    *last = if next { last.checked_add(1)? } else { last.checked_sub(1)? };
    game.node_at(&NodePath::new(path))
}

/// Move `node`'s index among its parent's replies (0: it continues its parent's line).
fn reply_index(game: &Game, node: NodeId) -> Option<u16> {
    game.path_of(node)?.indices().last().copied()
}

/// Whether `node` is on the game's main line (the start is).
fn on_main_line(game: &Game, node: NodeId) -> bool {
    game.path_of(node).is_some_and(|p| p.indices().iter().all(|&i| i == 0))
}

/// Whether a move follows `node`.
fn has_reply(game: &Game, node: NodeId) -> bool {
    game.line_of(node).last().is_some_and(|&last| last != node)
}

/// A move as books write it in running text: "20.fxg6", "18…Qd8".
pub(crate) fn move_name(game: &Game, node: NodeId) -> String {
    let san = game.san(node);
    let half = game.path_of(node).and_then(|p| {
        let mut up = p.indices().to_vec();
        up.pop()?;
        game.abs_ply(game.node_at(&NodePath::new(up))?)
    });
    match half {
        Some(half) if half % 2 == 1 => format!("{}…{san}", half / 2 + 1),
        Some(half) => format!("{}.{san}", half / 2 + 1),
        None => san.to_owned(),
    }
}

/// The structure tools (§4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    PromoteOne,
    MakeMain,
    Demote,
    DeleteVariation,
    CutAfter,
    /// A variation one place up or down among the others at its fork (T15).
    MoveUp,
    MoveDown,
    /// A null move after the move (T15).
    Threat,
}

impl Op {
    /// Whether variation head `head` is one this tool acts on: its reply index, and for moving a
    /// variation down, whether a variation comes after it.
    fn takes(self, game: &Game, head: NodeId) -> bool {
        let Some(index) = reply_index(game, head) else { return false };
        match self {
            Op::PromoteOne | Op::DeleteVariation => index != 0,
            Op::Demote => index == 0,
            Op::MoveUp => index >= 2,
            Op::MoveDown => index != 0 && sibling(game, head, true).is_some(),
            Op::MakeMain | Op::CutAfter | Op::Threat => false,
        }
    }

    /// The heads on the way to `from` this tool can act on, nearest first.
    fn heads(self, game: &Game, from: NodeId) -> Vec<NodeId> {
        game.heads_on_path(from).into_iter().filter(|&h| self.takes(game, h)).collect()
    }

    /// The move this tool acts on for `target`. For a variation tool, from a move: the nearest
    /// head it can act on, else the nearest head (to say why not).
    fn resolve(self, game: &Game, target: Target) -> Result<NodeId, String> {
        match self {
            Op::PromoteOne | Op::Demote | Op::DeleteVariation | Op::MoveUp | Op::MoveDown => {
                let from = match target {
                    Target::Head(head) => return Ok(head),
                    Target::Current => game.current(),
                    Target::Move(node) => node,
                };
                let nearest = game.heads_on_path(from).first().copied();
                self.heads(game, from).first().copied().or(nearest).ok_or_else(|| self.refusal(StructureError::NoSuchMove))
            }
            Op::MakeMain | Op::CutAfter | Op::Threat => Ok(match target {
                Target::Current => game.current(),
                Target::Move(node) | Target::Head(node) => node,
            }),
        }
    }

    /// What the tool would act on for move `node` (a context menu), or why it can't, without
    /// changing anything. Variations nesting too deep are only found when it acts.
    fn check(self, game: &Game, node: NodeId) -> Result<Target, String> {
        let at = self.resolve(game, Target::Move(node))?;
        match self {
            Op::PromoteOne | Op::DeleteVariation | Op::Demote | Op::MoveUp | Op::MoveDown => match reply_index(game, at) {
                Some(_) if self.takes(game, at) => Ok(Target::Head(at)),
                Some(i) if i != 0 && matches!(self, Op::MoveUp | Op::MoveDown) => Err(self.refusal(StructureError::NoSibling)),
                Some(0) => Err(self.refusal(if self == Op::Demote { StructureError::NoAlternative } else { StructureError::AlreadyFirst })),
                _ => Err(self.refusal(StructureError::NotFirst)),
            },
            Op::Threat if game.in_check_at(at) => Err(threat_refusal()),
            Op::Threat => Ok(Target::Move(at)),
            Op::MakeMain if on_main_line(game, at) => Err(tl!("This move is on the main line already.").to_owned()),
            Op::CutAfter if !has_reply(game, at) => Err(tl!("There are no moves after this one.").to_owned()),
            Op::MakeMain | Op::CutAfter => Ok(Target::Move(at)),
        }
    }

    /// Change `game`; the undo step's name, or why it was not done (the game is then unchanged).
    fn apply(self, game: &mut Game, target: Target) -> Result<ChessEdit, String> {
        let at = self.resolve(game, target)?;
        let refused = |e| self.refusal(e);
        match self {
            Op::PromoteOne => game.promote_one(at).map(|()| ChessEdit::PromoteOne).map_err(refused),
            Op::Demote => game.demote(at).map(|()| ChessEdit::Demote).map_err(refused),
            Op::DeleteVariation => game.delete_variation(at).map(|()| ChessEdit::DeleteVariation).map_err(refused),
            Op::MoveUp => game.move_variation(at, Shift::Up).map(|()| ChessEdit::MoveVariation).map_err(refused),
            Op::MoveDown => game.move_variation(at, Shift::Down).map(|()| ChessEdit::MoveVariation).map_err(refused),
            Op::Threat => {
                let shown = game.current();
                game.go_to_node(at);
                match game.play_null() {
                    Ok(()) => Ok(ChessEdit::NullMove),
                    Err(_) => {
                        game.go_to_node(shown);
                        Err(threat_refusal())
                    }
                }
            }
            Op::MakeMain => {
                if on_main_line(game, at) {
                    return Err(tl!("This move is on the main line already.").to_owned());
                }
                let revision = game.revision();
                game.promote(at);
                // `promote` leaves the game as it is when the old main line would nest too deep.
                if game.revision() == revision { Err(refused(StructureError::TooDeep)) } else { Ok(ChessEdit::MakeMainLine) }
            }
            Op::CutAfter => {
                if !has_reply(game, at) {
                    return Err(tl!("There are no moves after this one.").to_owned());
                }
                game.go_to_node(at);
                game.truncate();
                Ok(ChessEdit::DeleteMoves)
            }
        }
    }

    /// Why the tool was refused, in the current language.
    fn refusal(self, e: StructureError) -> String {
        let text = match (self, e) {
            (_, StructureError::TooDeep) => tl!("Not done: variations would nest too deep."),
            (Op::Demote, StructureError::NotFirst) => tl!("Not demoted: this move is a variation; only the line that continues a move is demoted."),
            (Op::Demote, _) => tl!("Not demoted: there is no variation to take its place."),
            (Op::PromoteOne, StructureError::NoSuchMove) => tl!("Not promoted: there is no variation on the way to this move."),
            (Op::PromoteOne, _) => tl!("Not promoted: this line already continues its move."),
            (Op::DeleteVariation, StructureError::NoSuchMove) => tl!("Not deleted: there is no variation on the way to this move."),
            (Op::DeleteVariation, _) => tl!("Not deleted: this is the main line here, not a variation."),
            (Op::MoveUp | Op::MoveDown, StructureError::NoSuchMove) => tl!("Not moved: there is no variation on the way to this move."),
            (Op::MoveUp | Op::MoveDown, StructureError::AlreadyFirst) => tl!("Not moved: this is the line itself, not a variation."),
            (Op::MoveUp, _) => tl!("Not moved: it is the first variation here already."),
            (Op::MoveDown, _) => tl!("Not moved: it is the last variation here already."),
            (Op::MakeMain | Op::CutAfter | Op::Threat, _) => tl!("Not done: there is no such move."),
        };
        text.to_owned()
    }
}

fn threat_refusal() -> String {
    tl!("No threat here: the side to move is in check and cannot pass.").to_owned()
}

/// Use structure tool `op` on `target` as one undo step; a refusal is said in the panel, never
/// silently nothing.
fn structure(board: &mut BoardState, op: Op, target: Target) {
    if board.illegal.is_some() {
        return;
    }
    board.selected = None;
    board.promotion = None;
    let mut refused = None;
    board.edit(|s| match op.apply(&mut s.game, target) {
        Ok(edit) => Some(edit),
        Err(why) => {
            refused = Some(why);
            None
        }
    });
    board.message = refused;
}

/// The rail's tools a move's context menu offers, for move `node`: each acts on what it would
/// for that move (the nearest variation it can act on, for the variation tools), or is off with
/// the reason; where a variation tool could act on several variations on the way to the move,
/// they are listed under it to choose from (§4.5, U1). Returns the one chosen, with its target.
pub(crate) fn move_menu(ui: &mut egui::Ui, board: &BoardState, node: NodeId) -> Option<(Run, Target)> {
    let t = Tokens::get(ui.ctx());
    let mut chosen = None;
    for tool in tools() {
        let (Kind::Action(run), Some(on_move)) = (tool.kind, tool.on_move) else { continue };
        let target = on_move(&board.game, node);
        let label = label(tool, board);
        let r = ui.add_enabled(target.is_ok(), egui::Button::new(label.as_str()));
        let r = match &target {
            Ok(Target::Head(head)) if *head != node => {
                r.on_hover_text(crate::i18n::fmt(tl!("Acts on {move}"), &[("move", &move_name(&board.game, *head))]))
            }
            Err(why) => r.on_disabled_hover_text(why.as_str()),
            Ok(_) => r,
        };
        if r.clicked()
            && let Ok(target) = target
        {
            chosen = Some((run, target));
        }
        if target.is_ok()
            && let Some(head) = head_choices(ui, &t, &board.game, tool, &label, node)
        {
            chosen = Some((run, Target::Head(head)));
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game of `pgn`, at the move reached by `path` (reply indices from the start).
    fn game(pgn: &str, path: &[u16]) -> Game {
        let (mut game, stopped) = Game::from_pgn(pgn).unwrap();
        assert_eq!(stopped, None);
        let node = game.node_at(&NodePath::new(path.to_vec())).unwrap();
        game.go_to_node(node);
        game
    }

    const FORKS: &str = "1. e4 e5 (1... c5 2. Nf3 (2. c3) d6) 2. Nf3 *";

    #[test]
    fn variation_tools_act_on_the_nearest_variation_they_can() {
        // At 2...d6 in the 1...c5 variation, the heads are 2.Nf3 (continuing c5's line) and
        // 1...c5 (a variation).
        let mut g = game(FORKS, &[0, 1, 0, 0]);
        assert_eq!(g.san(g.current()), "d6");
        let names: Vec<String> = g.heads_on_path(g.current()).into_iter().map(|h| move_name(&g, h)).collect();
        assert_eq!(names, ["2.Nf3", "1…c5"]);
        assert_eq!(Op::PromoteOne.heads(&g, g.current()).iter().map(|&h| move_name(&g, h)).collect::<Vec<_>>(), ["1…c5"]);
        assert_eq!(Op::Demote.heads(&g, g.current()).iter().map(|&h| move_name(&g, h)).collect::<Vec<_>>(), ["2.Nf3"]);
        assert_eq!(Op::PromoteOne.apply(&mut g, Target::Current), Ok(ChessEdit::PromoteOne));
        assert_eq!(g.movetext(), "1. e4 c5 (1... e5 2. Nf3) 2. Nf3 (2. c3) 2... d6");
        let mut g = game(FORKS, &[0, 1, 0, 0]);
        assert_eq!(Op::Demote.apply(&mut g, Target::Current), Ok(ChessEdit::Demote));
        assert_eq!(g.movetext(), "1. e4 e5 (1... c5 2. c3 (2. Nf3 d6)) 2. Nf3");
        let mut g = game(FORKS, &[0, 1, 0, 0]);
        assert_eq!(Op::DeleteVariation.apply(&mut g, Target::Current), Ok(ChessEdit::DeleteVariation));
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3");
        assert_eq!(g.san(g.current()), "e4", "the move shown went with the variation: its fork's move is shown");
    }

    #[test]
    fn refusals_say_why_and_change_nothing() {
        // On the main line before any fork: nothing to promote, demote or delete.
        let mut g = game(FORKS, &[0]);
        let before = g.movetext();
        for op in [Op::PromoteOne, Op::Demote, Op::DeleteVariation, Op::MakeMain] {
            let why = op.apply(&mut g, Target::Current).unwrap_err();
            assert!(!why.is_empty());
            assert_eq!(g.movetext(), before, "{op:?}");
        }
        assert_eq!(Op::Demote.apply(&mut g, Target::Current), Err("Not demoted: there is no variation to take its place.".to_owned()));
        // On 1...c5 itself: a variation is not demoted.
        let mut g = game(FORKS, &[0, 1]);
        assert!(Op::Demote.apply(&mut g, Target::Current).unwrap_err().contains("is a variation"));
        // At the end of a line: nothing to cut.
        let mut g = game(FORKS, &[0, 0, 0]);
        assert_eq!(Op::CutAfter.apply(&mut g, Target::Current), Err("There are no moves after this one.".to_owned()));
        assert_eq!(g.movetext(), before);
    }

    #[test]
    fn a_move_menu_names_what_each_tool_acts_on() {
        let g = game(FORKS, &[0]);
        let d6 = g.node_at(&NodePath::new(vec![0, 1, 0, 0])).unwrap();
        let c5 = g.node_at(&NodePath::new(vec![0, 1])).unwrap();
        let e4 = g.node_at(&NodePath::new(vec![0])).unwrap();
        assert_eq!(Op::DeleteVariation.check(&g, d6), Ok(Target::Head(c5)));
        assert_eq!(Op::MakeMain.check(&g, d6), Ok(Target::Move(d6)));
        assert!(Op::MakeMain.check(&g, e4).is_err());
        assert_eq!(Op::CutAfter.check(&g, e4), Ok(Target::Move(e4)));
        assert!(Op::CutAfter.check(&g, d6).is_err());
        assert!(Op::DeleteVariation.check(&g, e4).is_err());
    }

    const THREE: &str = "1. e4 e5 (1... c5) (1... e6 2. d4) (1... d5) *";

    #[test]
    fn variations_move_up_and_down_among_their_siblings_and_never_past_the_line() {
        // At 2.d4 in the 1...e6 variation: up puts it before 1...c5, down after 1...d5.
        let mut g = game(THREE, &[0, 2, 0]);
        assert_eq!(Op::MoveUp.apply(&mut g, Target::Current), Ok(ChessEdit::MoveVariation));
        assert_eq!(g.movetext(), "1. e4 e5 (1... e6 2. d4) (1... c5) (1... d5)");
        assert_eq!(g.san(g.current()), "d4", "the move shown stays");
        assert!(Op::MoveUp.apply(&mut g, Target::Current).unwrap_err().contains("first variation"));
        assert_eq!(Op::MoveDown.apply(&mut g, Target::Current), Ok(ChessEdit::MoveVariation));
        assert_eq!(g.movetext(), "1. e4 e5 (1... c5) (1... e6 2. d4) (1... d5)");
        let mut g = game(THREE, &[0, 3]);
        assert!(Op::MoveDown.apply(&mut g, Target::Current).unwrap_err().contains("last variation"));
        // On the main line there is no variation to move; a menu says so for the move.
        let g = game(THREE, &[0, 0]);
        assert!(Op::MoveUp.check(&g, g.current()).unwrap_err().contains("the line itself"));
        let e6 = g.node_at(&NodePath::new(vec![0, 2])).unwrap();
        assert_eq!(Op::MoveDown.check(&g, e6), Ok(Target::Head(e6)));
        assert_eq!(Op::MoveUp.heads(&g, e6), [e6]);
    }

    #[test]
    fn a_threat_is_a_null_move_after_the_move_and_never_in_check() {
        let mut g = game("1. e4 e5 2. Nf3 *", &[0, 0]);
        assert_eq!(Op::Threat.apply(&mut g, Target::Current), Ok(ChessEdit::NullMove));
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 (2. --)");
        assert!(g.is_null(g.current()));
        let mut g = game("1. e4 f5 2. Qh5+ *", &[0, 0, 0]);
        let before = g.movetext();
        assert!(Op::Threat.check(&g, g.current()).is_err());
        assert!(Op::Threat.apply(&mut g, Target::Current).unwrap_err().contains("in check"));
        assert_eq!((g.movetext(), g.san(g.current())), (before, "Qh5+"));
    }

    #[test]
    fn up_and_down_choose_among_the_replies_at_a_fork() {
        let g = game(FORKS, &[0, 0]);
        let c5 = sibling(&g, g.current(), true).unwrap();
        assert_eq!(g.san(c5), "c5");
        assert_eq!(sibling(&g, c5, true), None);
        assert_eq!(sibling(&g, c5, false), Some(g.current()));
        assert_eq!(sibling(&g, g.current(), false), None);
        assert_eq!(sibling(&g, 0, true), None, "the start has no fork");
    }

    #[test]
    fn the_keymap_takes_its_keys_and_no_others() {
        let cmd = Modifiers::COMMAND;
        assert!(scope_key(Key::Delete, Modifiers::NONE));
        assert!(scope_key(Key::Backspace, Modifiers::NONE));
        assert!(scope_key(Key::Delete, Modifiers::SHIFT));
        assert!(scope_key(Key::ArrowUp, cmd));
        assert!(scope_key(Key::ArrowUp, cmd | Modifiers::SHIFT));
        assert!(scope_key(Key::ArrowDown, cmd));
        assert!(scope_key(Key::Escape, Modifiers::NONE));
        assert!(scope_key(Key::S, Modifiers::SHIFT), "the Structure group's letter");
        assert!(scope_key(Key::Enter, Modifiers::NONE), "the comment editor");
        assert!(scope_key(Key::G, Modifiers::NONE), "the glyph palette");
        assert!(!scope_key(Key::G, cmd), "Ctrl+G is the app's (find next)");
        // Alt with ↑ and ↓ moves a variation (T15); B arms the glyph brush.
        assert!(scope_key(Key::ArrowUp, Modifiers::ALT));
        assert!(scope_key(Key::ArrowDown, Modifiers::ALT));
        assert!(scope_key(Key::B, Modifiers::NONE));
        // Not ours: the app's Undo, Select all, plain letters, Alt with the other arrows or with
        // more modifiers.
        assert!(!scope_key(Key::Z, cmd));
        assert!(!scope_key(Key::A, cmd));
        assert!(!scope_key(Key::S, Modifiers::NONE));
        assert!(!scope_key(Key::ArrowLeft, Modifiers::ALT));
        assert!(!scope_key(Key::ArrowUp, Modifiers::ALT | Modifiers::SHIFT));
        assert!(!scope_key(Key::ArrowUp, Modifiers::ALT | cmd));
        assert!(!scope_key(Key::Delete, cmd));
        // macOS: Ctrl with the arrows is the system's, not the command key's.
        assert!(!scope_key(Key::ArrowUp, Modifiers { ctrl: true, ..Modifiers::NONE }));
    }

    /// Each rail tool with a command is registered, without a shortcut of the registry's own (the
    /// key is the panel's), and every `chess.*` command of the rail's kinds is a rail tool's; the
    /// palette shows the panel's key for it.
    #[test]
    fn the_rails_tools_and_their_commands_match() {
        use pdfcraft_engine::commands::{COMMANDS, command};
        for tool in tools() {
            if let Some(id) = tool.command {
                assert!(command(id).is_some(), "{}: {id} is not registered", tool.id);
            }
        }
        let rail = |id: &str| {
            ["chess.tool.", "chess.variation.", "chess.moves.", "chess.nag.", "chess.comment.", "chess.palette.", "chess.source."]
                .iter()
                .any(|p| id.starts_with(p))
        };
        // Copying a line goes to the clipboard, which only the app reaches: its commands are the app's.
        for spec in COMMANDS.iter().filter(|c| rail(c.id) && !c.id.starts_with("chess.moves.copy_")) {
            assert!(is_tool_command(spec.id), "{} runs no rail tool", spec.id);
            assert!(spec.shortcut.is_none(), "{}: the chess keys are the panel's, not the registry's", spec.id);
        }
        assert!(!is_tool_command("edit.undo"), "the app's Undo is not the rail's to run");
        assert!(!is_tool_command("chess.window.detach"), "the app runs the chess window's command");
        let ctx = egui::Context::default();
        ctx.set_os(egui::os::OperatingSystem::Nix);
        assert_eq!(command_key(&ctx, "chess.variation.promote_one").as_deref(), Some("Ctrl+↑"));
        assert_eq!(command_key(&ctx, "chess.variation.delete").as_deref(), Some("Delete"));
        assert_eq!(command_key(&ctx, "chess.variation.up").as_deref(), Some("Alt+↑"));
        ctx.set_os(egui::os::OperatingSystem::Mac);
        assert_eq!(command_key(&ctx, "chess.variation.down").as_deref(), Some("⌥↓"));
        assert_eq!(command_key(&ctx, "chess.scan"), None);
    }

    #[test]
    fn every_rail_tool_has_an_icon_and_its_own_keys() {
        let mut keys = Vec::new();
        for tool in tools() {
            assert!(matches!(tool.kind, Kind::Glyph(_)) || icons::exists(tool.icon), "{}: no icon {}", tool.id, tool.icon);
            for k in tool.keys {
                assert!(!keys.contains(k), "{}: {k:?} is another tool's", tool.id);
                assert!(!NAV_KEYS.iter().any(|(n, _)| n == k) || tool.id == "navigate", "{}: {k:?} moves about", tool.id);
                keys.push(*k);
            }
        }
        for group in groups() {
            assert!(!group.members.is_empty());
            assert!(group.letter.is_none_or(|l| l.name().len() == 1), "a group's key is a letter");
        }
    }
}
