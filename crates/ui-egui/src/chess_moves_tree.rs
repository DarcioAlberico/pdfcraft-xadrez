//! The move list as a tree (plan A8 V3): variations that fold and unfold, a trail saying where in
//! the tree the move shown is, the continuations at a fork, and the notice when a move played
//! starts a variation. The game is never changed by folding: only what the list shows.

use std::collections::{HashMap, HashSet};

use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodeId, NodePath, Token};

use crate::chess_board_ui::BoardState;
use crate::chess_history::ChessEdit;
use crate::theme::Tokens;

/// What a step forward does where the move shown has more than one reply (plan A8 V3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AtFork {
    /// Follow the line stepped back along, else the main line.
    #[default]
    Follow,
    /// Stop and offer the replies, the remembered one picked: ↑ ↓ choose, → or Enter go in, Esc
    /// takes the main line.
    Ask,
}

/// What a move played beside the reply already there does (plan A8 V3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewMove {
    /// It starts a variation, and the notice offers Make main line and Undo.
    #[default]
    Variation,
    /// It starts a variation, and the notice asks: new variation, new main line, replace the
    /// continuation, or cancel.
    Ask,
    /// It replaces the continuation: the other replies go, with the deletion notice and Undo.
    Replace,
}

/// The two choices, kept with the preferences (`chess_variations`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VariationRules {
    pub at_fork: AtFork,
    pub new_move: NewMove,
}

impl VariationRules {
    pub fn to_json(self) -> serde_json::Value {
        let at_fork = match self.at_fork {
            AtFork::Follow => "follow",
            AtFork::Ask => "ask",
        };
        let new_move = match self.new_move {
            NewMove::Variation => "variation",
            NewMove::Ask => "ask",
            NewMove::Replace => "replace",
        };
        serde_json::json!({ "at_fork": at_fork, "new_move": new_move })
    }

    /// From the settings file, which is untrusted: an unknown value keeps the default.
    pub fn from_json(v: &serde_json::Value) -> VariationRules {
        let mut rules = VariationRules::default();
        let _ = rules.set("at-fork", v["at_fork"].as_str().unwrap_or_default());
        let _ = rules.set("new-move", v["new_move"].as_str().unwrap_or_default());
        rules
    }

    /// Set one by name, as the control channel names them: `at-fork` (follow, ask) and
    /// `new-move` (variation, ask, replace).
    pub fn set(&mut self, part: &str, value: &str) -> Result<(), String> {
        match (part, value) {
            ("at-fork", "follow") => self.at_fork = AtFork::Follow,
            ("at-fork", "ask") => self.at_fork = AtFork::Ask,
            ("new-move", "variation") => self.new_move = NewMove::Variation,
            ("new-move", "ask") => self.new_move = NewMove::Ask,
            ("new-move", "replace") => self.new_move = NewMove::Replace,
            ("at-fork", _) => return Err("variations-at-fork must be follow or ask".into()),
            ("new-move", _) => return Err("variations-new-move must be variation, ask or replace".into()),
            _ => return Err(format!("unknown part {part}")),
        }
        Ok(())
    }
}

/// Keep only the move shown among its parent's replies: the continuation it was played beside,
/// and every other variation there, go (each one an ordinary deletion). Ids change as moves go,
/// so the parent is found again by path each time; bounded by the replies a move can have.
pub(crate) fn replace_continuation(game: &mut Game) {
    for _ in 0..crate::chess_moves_tree::MAX_REPLIES {
        let current = game.current();
        let Some(path) = game.path_of(current) else { return };
        let mut up = path.indices().to_vec();
        if up.pop().is_none() {
            return;
        }
        let Some(parent) = game.node_at(&NodePath::new(up)) else { return };
        let Some(&other) = game.replies(parent).iter().find(|&&n| n != current) else { return };
        game.delete(other);
    }
}

/// Most replies [`replace_continuation`] looks at.
const MAX_REPLIES: usize = 512;

/// The first moves of the variations that hold `node`, outermost first: the trail from the main
/// line to it.
pub(crate) fn variations_holding(game: &Game, node: NodeId) -> Vec<NodeId> {
    let Some(path) = game.path_of(node) else { return Vec::new() };
    let indices = path.indices();
    let mut out = Vec::new();
    for (k, &index) in indices.iter().enumerate() {
        if index > 0
            && let Some(start) =
                game.node_at(&pdfcraft_engine::chess::pdfcraft_chess::game::NodePath::new(indices.get(..=k).unwrap_or_default().to_vec()))
        {
            out.push(start);
        }
    }
    out
}

/// The notation's tokens as the list shows them: a folded variation keeps its `(`, its first move
/// and its `)`, the rest left out and counted (by its first move); with `main_only`, every
/// variation is left out. A variation holding the move shown is never folded.
pub(crate) fn fold<'a>(
    tokens: &'a [Token],
    folded: &HashSet<NodeId>,
    main_only: bool,
    open: &HashSet<NodeId>,
) -> (Vec<&'a Token>, HashMap<NodeId, usize>) {
    let mut out = Vec::with_capacity(tokens.len());
    let mut hidden: HashMap<NodeId, usize> = HashMap::new();
    let mut i = 0;
    while let Some(token) = tokens.get(i) {
        let first = match (token, tokens.get(i + 1)) {
            (Token::VariationStart, Some(Token::Move { node, .. })) => Some(*node),
            _ => None,
        };
        let fold_here = first.is_some_and(|n| main_only || (folded.contains(&n) && !open.contains(&n)));
        if !fold_here {
            out.push(token);
            i += 1;
            continue;
        }
        // Find the matching `)`, counting the moves on the way.
        let mut depth = 0usize;
        let mut moves = 0usize;
        let mut end = i;
        for (k, t) in tokens.iter().enumerate().skip(i) {
            match t {
                Token::VariationStart => depth += 1,
                Token::VariationEnd => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = k;
                        break;
                    }
                }
                Token::Move { .. } => moves += 1,
                Token::Comment { .. } => {}
            }
            end = k;
        }
        if !main_only {
            out.push(token);
            if let Some(m) = tokens.get(i + 1) {
                out.push(m);
            }
            if let (Some(n), Some(close)) = (first, tokens.get(end)) {
                hidden.insert(n, moves.saturating_sub(1));
                if matches!(close, Token::VariationEnd) {
                    out.push(close);
                }
            }
        }
        i = end + 1;
    }
    (out, hidden)
}

/// What the tree's header asked for.
pub(crate) enum TreeAction {
    Go(NodeId),
    ExpandAll,
    CollapseAll,
    MainOnly(bool),
}

/// Above the list, when the game has variations: the tree menu (expand all, collapse all, the
/// main line only) and, while the move shown is in a variation, the trail to it ("Main line ›
/// 2…Nf6 › 3.Bc4"), each segment going to the first move of its line.
pub(crate) fn header(ui: &mut egui::Ui, t: &Tokens, state: &BoardState, tokens: &[Token]) -> Option<TreeAction> {
    // From the notation's tokens, worked out once per change (a 10,000-move game is not walked
    // every frame).
    if !has_variations(tokens) {
        return None;
    }
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        action = tree_menu(ui, t, state);
        if let Some(a) = trail(ui, t, state, f32::INFINITY) {
            action = Some(a);
        }
    });
    action
}

/// Whether the notation's `tokens` have a variation.
pub(crate) fn has_variations(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| matches!(t, Token::VariationStart))
}

/// Do what the tree's menu or trail asked for.
pub(crate) fn apply(state: &mut BoardState, asked: Option<TreeAction>) {
    match asked {
        Some(TreeAction::Go(node)) => state.game.go_to_node(node),
        Some(TreeAction::ExpandAll) => {
            state.folded.clear();
            state.main_only = false;
        }
        Some(TreeAction::CollapseAll) => state.folded = state.game.variation_starts().into_iter().collect(),
        Some(TreeAction::MainOnly(on)) => state.main_only = on,
        None => {}
    }
}

/// The tree's menu: expand all, collapse all, the main line only.
pub(crate) fn tree_menu(ui: &mut egui::Ui, t: &Tokens, state: &BoardState) -> Option<TreeAction> {
    let menu = egui::containers::menu::MenuButton::from_button(egui::Button::image(crate::icons::image("list", 14.0, t.icon)));
    let (r, picked) = menu.ui(ui, |ui| {
        let mut pick = None;
        if crate::widgets::menu_entry(ui, true, egui::Button::new(tl!("Expand all"))).clicked() {
            pick = Some(TreeAction::ExpandAll);
        }
        if crate::widgets::menu_entry(ui, true, egui::Button::new(tl!("Collapse all"))).clicked() {
            pick = Some(TreeAction::CollapseAll);
        }
        let mut only = state.main_only;
        if ui.add(crate::widgets::checkbox(&mut only, tl!("Only the main line"))).changed() {
            pick = Some(TreeAction::MainOnly(only));
            ui.close();
        }
        pick
    });
    let name = tl!("Variations");
    r.on_hover_text(name).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    picked.and_then(|p| p.inner)
}

/// While the move shown is in a variation, the trail to it ("Main line › 2…Nf6 › 3.Bc4"), each
/// segment going to the first move of its line, in `room` points at most: the segments that do
/// not fit are left out from the second on, with a "…" in their place (the trail's last
/// segments, nearest the move shown, are the ones kept; its whole is its accessible name).
pub(crate) fn trail(ui: &mut egui::Ui, t: &Tokens, state: &BoardState, room: f32) -> Option<TreeAction> {
    let game = &state.game;
    let current = game.current();
    let holding = variations_holding(game, current);
    let &innermost = holding.last()?;
    let mut action = None;
    // In one line for screen readers: which variation at its fork, and the move shown
    // ("Variation 1 of 2 after 2. Nf3, move 3. Bc4").
    let fork = game_parent_of(game, innermost);
    let replies = game.replies(fork);
    let k = replies.iter().position(|&n| n == innermost).unwrap_or(0);
    let said = crate::i18n::fmt(
        tl!("Variation {i} of {n} after {fork}, move {move}"),
        &[
            ("i", &k.to_string()),
            ("n", &replies.len().saturating_sub(1).to_string()),
            ("fork", &if fork == 0 { tl!("the start").to_owned() } else { crate::chess_tools::move_name(game, fork) }),
            ("move", &crate::chess_tools::move_name(game, current)),
        ],
    );
    let id = ui.id().with("chess-trail");
    let r = ui.interact(egui::Rect::from_min_size(ui.cursor().min, egui::vec2(1.0, 1.0)), id, egui::Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
    // The segments: the variations' first moves, then the move shown when it is further on.
    let mut segments: Vec<(String, Option<NodeId>)> = holding.iter().map(|&n| (crate::chess_tools::move_name(game, n), Some(n))).collect();
    if holding.last() != Some(&current) {
        segments.push((crate::chess_tools::move_name(game, current), None));
    }
    let font = egui::TextStyle::Small.resolve(ui.style());
    let gap = ui.spacing().item_spacing.x;
    let width = |text: &str| crate::widgets::text_width(ui, text, &font) + gap;
    let arrow = width("›");
    let main = width(tl!("Main line"));
    // From the last segment back, as many as fit beside "Main line" (and a "…" when some don't).
    let mut used = main;
    let mut first = segments.len();
    for (i, (name, _)) in segments.iter().enumerate().rev() {
        let w = arrow + width(name);
        let ellipsis = if i > 0 { arrow + width("…") } else { 0.0 };
        if used + w + ellipsis > room && first < segments.len() {
            break;
        }
        used += w;
        first = i;
    }
    if ui.link(egui::RichText::new(tl!("Main line")).small()).clicked() {
        // The main line's move the outermost variation forks from.
        action = Some(TreeAction::Go(holding.first().map_or(0, |&n| game_parent_of(game, n))));
    }
    if first > 0 {
        ui.label(egui::RichText::new("›").small().color(t.text_faint));
        ui.label(egui::RichText::new("…").small().color(t.text_faint));
    }
    for (name, node) in segments.iter().skip(first) {
        ui.label(egui::RichText::new("›").small().color(t.text_faint));
        match node {
            Some(n) => {
                if ui.link(egui::RichText::new(name).small()).clicked() {
                    action = Some(TreeAction::Go(*n));
                }
            }
            None => {
                ui.label(egui::RichText::new(name).small().strong());
            }
        }
    }
    action
}

/// The move a line's first move follows (the fork), or the start.
fn game_parent_of(game: &Game, node: NodeId) -> NodeId {
    let Some(path) = game.path_of(node) else { return 0 };
    let mut up = path.indices().to_vec();
    up.pop();
    game.node_at(&pdfcraft_engine::chess::pdfcraft_chess::game::NodePath::new(up)).unwrap_or(0)
}

/// Under the board, when the move shown has more than one continuation: a chip for each, the
/// one a step forward takes marked; a click steps into it (plan A8 V3). While a step forward
/// asks (`AtFork::Ask`), the chip picked is the one the keys move, framed, with what to press.
///
/// In one row of `room` points (the Board tab's trail row, plan A9-I0b): the chips that fit, and
/// the rest in a "…" menu; while a step forward asks, the keys to press are the row's tooltip
/// (and a line of the moves' region).
/// Whether the move shown is at a fork (and the row was drawn).
pub(crate) fn continuations(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState, room: f32) -> bool {
    let current = state.game.current();
    let replies = state.game.replies(current).to_vec();
    if replies.len() < 2 {
        state.choosing = None;
        return false;
    }
    let remembered = state.game.next_reply(current);
    let choosing = state.choosing.filter(|k| *k < replies.len());
    let mut pick = None;
    let names: Vec<String> = replies.iter().map(|&reply| crate::chess_tools::move_name(&state.game, reply)).collect();
    let marked = |i: usize| match choosing {
        Some(k) => k == i,
        None => replies.get(i).copied() == remembered,
    };
    let frame = if choosing.is_some() {
        egui::Frame::NONE
            .stroke(egui::Stroke::new(1.0, t.accent))
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(2, 0))
    } else {
        egui::Frame::NONE
    };
    let shown = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let font = egui::TextStyle::Small.resolve(ui.style());
            let gap = ui.spacing().item_spacing.x;
            let pad = 2.0 * ui.spacing().button_padding.x;
            let chip = |ui: &egui::Ui, name: &str| crate::widgets::text_width(ui, name, &font) + pad + gap;
            let more = chip(ui, "…");
            let start = ui.cursor().min.x;
            ui.label(egui::RichText::new(tl!("Continuations:")).small().color(t.text_muted));
            let mut left = names.len();
            for (i, name) in names.iter().enumerate() {
                let used = ui.cursor().min.x - start;
                let rest_after = if i + 1 < names.len() { more } else { 0.0 };
                if i > 0 && used + chip(ui, name) + rest_after > room {
                    left = i;
                    break;
                }
                if ui.selectable_label(marked(i), egui::RichText::new(name).small()).clicked() {
                    pick = Some(i);
                }
            }
            if left < names.len() {
                let menu = egui::containers::menu::MenuButton::new(egui::RichText::new("…").small());
                let (r, _) = menu.ui(ui, |ui| {
                    for (i, name) in names.iter().enumerate().skip(left) {
                        if crate::widgets::menu_entry(ui, true, egui::Button::new(name.as_str()).selected(marked(i))).clicked() {
                            pick = Some(i);
                        }
                    }
                });
                let all = names.join(" · ");
                let said = crate::i18n::fmt(tl!("More continuations: {moves}"), &[("moves", &all)]);
                r.on_hover_text(&all).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &said));
            }
        });
    });
    if choosing.is_some() {
        // The keys are said under the trail too (the Board tab's moves).
        shown.response.on_hover_text(tl!("↑ ↓ choose, → or Enter goes in, Esc takes the main line."));
    }
    if let Some(i) = pick {
        state.choosing = None;
        state.game.forward_to(i);
        state.selected = None;
    }
    true
}

/// A key while a step forward asks which reply (`AtFork::Ask`): ↑ ↓ move the choice, → or Enter
/// go into it, Esc into the main line, ← leaves the question. Whether the key was taken.
pub(crate) fn choose_key(state: &mut BoardState, key: ChooseKey) -> bool {
    let Some(k) = state.choosing else { return false };
    let n = state.game.replies(state.game.current()).len();
    if n < 2 {
        state.choosing = None;
        return false;
    }
    match key {
        ChooseKey::Up => state.choosing = Some(k.checked_sub(1).unwrap_or(n - 1)),
        ChooseKey::Down => state.choosing = Some(if k + 1 >= n { 0 } else { k + 1 }),
        ChooseKey::Enter => {
            state.choosing = None;
            state.game.forward_to(k.min(n - 1));
        }
        ChooseKey::Main => {
            state.choosing = None;
            state.game.forward_to(0);
        }
        ChooseKey::Leave => state.choosing = None,
    }
    true
}

/// The keys [`choose_key`] takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChooseKey {
    Up,
    Down,
    Enter,
    Main,
    Leave,
}

/// While the move shown is the one a move played just made into a variation: "Variation created:
/// 2…Nf6", with Make main line and Undo.
pub(crate) fn created_notice(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState) {
    let Some(node) = state.created_variation.filter(|&n| n == state.game.current()) else {
        state.created_variation = None;
        return;
    };
    let mut make_main = false;
    let mut undo = false;
    let mut replace = false;
    let mut keep = false;
    let ask = state.rules.new_move == NewMove::Ask;
    ui.horizontal_wrapped(|ui| {
        let text = crate::i18n::fmt(tl!("Variation created: {move}"), &[("move", &crate::chess_tools::move_name(&state.game, node))]);
        ui.label(egui::RichText::new(text).small().color(t.text_muted));
        if ask {
            // Asked (plan A8 V3): a new variation (as it is), a new main line, the continuation
            // replaced, or the move taken back.
            keep = ui.small_button(tl!("New variation")).clicked();
            make_main = ui.small_button(tl!("New main line")).clicked();
            replace = ui.small_button(tl!("Replace the continuation")).clicked();
            undo = ui.small_button(tl!("Cancel")).clicked();
        } else {
            make_main = ui.small_button(tl!("Make main line")).clicked();
            undo = ui.small_button(tl!("Undo")).clicked();
        }
    });
    if keep {
        state.created_variation = None;
    } else if replace {
        state.edit(|s| {
            replace_continuation(&mut s.game);
            Some(ChessEdit::DeleteMoves)
        });
        state.created_variation = None;
    } else if make_main {
        state.edit(|s| match s.game.promote(node) {
            Ok(()) => Some(ChessEdit::MakeMainLine),
            // A line that would nest the old one too deep is left as it is, and said (plan
            // A9-F5, A1-19: it was silent).
            Err(_) => {
                s.message = Some(tl!("Not done: variations would nest too deep.").to_owned());
                None
            }
        });
        state.created_variation = None;
    } else if undo {
        state.undo();
        state.created_variation = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(pgn: &str) -> Game {
        let (g, stopped) = Game::from_pgn(pgn).unwrap();
        assert_eq!(stopped, None);
        g
    }

    fn sans(game: &Game, tokens: &[&Token]) -> String {
        tokens
            .iter()
            .map(|t| match t {
                Token::Move { node, .. } => game.san(*node).to_owned(),
                Token::VariationStart => "(".to_owned(),
                Token::VariationEnd => ")".to_owned(),
                Token::Comment { text, .. } => format!("{{{text}}}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_folded_variation_keeps_its_first_move_and_counts_the_rest() {
        let g = game("1. e4 e5 (1... c5 2. Nf3 (2. c3 d5) d6) 2. Nf3 *");
        let tokens = g.tokens();
        let c5 = g.variation_starts()[0];
        let none = HashSet::new();
        let (all, hidden) = fold(&tokens, &none, false, &none);
        assert_eq!(sans(&g, &all), "e4 e5 ( c5 Nf3 ( c3 d5 ) d6 ) Nf3");
        assert!(hidden.is_empty());
        let folded: HashSet<NodeId> = [c5].into();
        let (shown, hidden) = fold(&tokens, &folded, false, &none);
        assert_eq!(sans(&g, &shown), "e4 e5 ( c5 ) Nf3");
        assert_eq!(hidden.get(&c5), Some(&4), "Nf3, c3, d5 and d6 folded");
        // Open (the move shown is in it): not folded.
        let open: HashSet<NodeId> = [c5].into();
        assert_eq!(fold(&tokens, &folded, false, &open).0.len(), all.len());
        // The main line only.
        assert_eq!(sans(&g, &fold(&tokens, &none, true, &none).0), "e4 e5 Nf3");
    }

    #[test]
    fn the_trail_names_each_variation_holding_the_move() {
        let g = game("1. e4 e5 (1... c5 2. Nf3 (2. c3 d5) d6) 2. Nf3 *");
        let starts = g.variation_starts();
        let (c5, c3) = (starts[0], starts[1]);
        let d5 = g.replies(c3)[0];
        assert_eq!(variations_holding(&g, d5), [c5, c3]);
        assert_eq!(variations_holding(&g, c5), [c5]);
        assert!(variations_holding(&g, 0).is_empty());
        assert_eq!(game_parent_of(&g, c5), g.replies(0)[0], "1.e4, where 1…c5 forks");
    }
}
