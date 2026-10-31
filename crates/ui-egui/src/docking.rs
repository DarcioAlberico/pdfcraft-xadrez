//! Application panel identities and persistence around the shared docking renderer.
//! Document contents remain owned by their existing view implementations.

use craft_ui::docking::{Action, DockArea, DockContent, DockStyle, Layout, Location, Node, PanelLimits, Permissions, Placement, Zone};
use craft_ui::layout::{SplitAxis, SplitSize};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Panel {
    Canvas,
    Tools,
    Inspector,
    /// Chess ▸ Board: the analysis board, a tab beside the Inspector.
    Board,
    /// The board's game as a list of moves, against the page's left or right edge.
    Moves,
    /// Chess ▸ Diagrams: the selected diagram's position to check and confirm (plan A8, P3b2),
    /// a tab beside the Inspector.
    Diagram,
}

impl Panel {
    pub const ALL: &[Self] = &[Self::Canvas, Self::Tools, Self::Inspector, Self::Board, Self::Moves, Self::Diagram];
    pub fn id(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Tools => "tools",
            Self::Inspector => "inspector",
            Self::Board => "board",
            Self::Moves => "moves",
            Self::Diagram => "diagram",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Canvas => "Document",
            Self::Tools => "Tools",
            Self::Inspector => "Inspector",
            Self::Board => "Chess board",
            Self::Moves => "Move list",
            Self::Diagram => "Diagram",
        }
    }
    fn default_side(self) -> Zone {
        match self {
            Self::Canvas => Zone::Right,
            Self::Tools => Zone::Left,
            Self::Inspector | Self::Board | Self::Moves | Self::Diagram => Zone::Right,
        }
    }
    fn parse(id: &str) -> Result<Self, String> {
        Self::ALL.iter().copied().find(|panel| panel.id() == id).ok_or_else(|| format!("Unknown panel: {id}"))
    }
    /// The width a panel of the right-hand column or the Move list takes in a split of its own
    /// across, where the split would give it half the page: they are as wide as they are
    /// dragged ([`panel_limits`]). The page and the Tools have none.
    fn opening_width(self) -> Option<f32> {
        match self {
            Self::Canvas | Self::Tools => None,
            Self::Inspector | Self::Board => Some(SIDE_WIDTH),
            Self::Diagram => Some(DIAGRAM_WIDTH),
            Self::Moves => Some(MOVES_WIDTH),
        }
    }
}

fn permissions(panel: &Panel) -> Permissions {
    if *panel == Panel::Canvas { Permissions::PROTECTED } else { Permissions::default() }
}

/// The Inspector's and the chess board's width when they open beside the page, in points.
pub(crate) const SIDE_WIDTH: f32 = 330.0;

/// The Move list's width when it opens beside the page, in points.
pub(crate) const MOVES_WIDTH: f32 = 260.0;

/// The Diagram tab's width when it opens beside the Inspector, in points: a 280 pt board and its
/// margins.
pub(crate) const DIAGRAM_WIDTH: f32 = 320.0;

/// What was asked of the Move list in a frame, done where the layout is not being drawn
/// ([`drain_moves`]): shown or closed, or put against the page's left (`true`) or right edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MovesRequest {
    Show(bool),
    Side { left: bool },
}

fn tabs(panel: Panel) -> Node<Panel> {
    Node::Tabs { panels: vec![panel], active: 0 }
}

fn split(axis: SplitAxis, size: SplitSize, first: Node<Panel>, second: Node<Panel>) -> Node<Panel> {
    Node::Split { axis, size, first: Box::new(first), second: Box::new(second) }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub layout: Layout<Panel>,
    /// Closed panels retain their most recent group, side, size or floating rectangle.
    pub hidden: Vec<(Panel, Location<Panel>)>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            layout: Layout {
                root: Some(split(
                    SplitAxis::Horizontal,
                    SplitSize::FixedFirst(272.0),
                    tabs(Panel::Tools),
                    split(SplitAxis::Horizontal, SplitSize::FixedSecond(SIDE_WIDTH), tabs(Panel::Canvas), tabs(Panel::Inspector)),
                )),
                floating: Vec::new(),
            },
            hidden: Vec::new(),
        }
    }
}

impl Workspace {
    /// Side of the Tools panel, including its saved position while closed.
    pub fn tools_on_right(&self) -> bool {
        let location = self
            .layout
            .location(&Panel::Tools)
            .ok()
            .or_else(|| self.hidden.iter().find(|(panel, _)| *panel == Panel::Tools).map(|(_, location)| location.clone()));
        location.is_some_and(|location| matches!(location.placement, Placement::Split(Zone::Right)))
    }

    /// Place Tools at the outer left or right edge while preserving its open state.
    pub fn set_tools_on_right(&mut self, right: bool) -> Result<(), String> {
        let was_open = self.layout.contains(&Panel::Tools);
        self.set_visible(Panel::Tools, true)?;
        let anchor = if right && self.layout.contains(&Panel::Inspector) { Panel::Inspector } else { Panel::Canvas };
        self.apply(Action::Move { panel: Panel::Tools, anchor, placement: Placement::Split(if right { Zone::Right } else { Zone::Left }) })?;
        if !was_open {
            self.set_visible(Panel::Tools, false)?;
        }
        Ok(())
    }

    /// Whether the Move list is at the page's left edge, including its saved place while closed.
    pub fn moves_on_left(&self) -> bool {
        let location = self
            .layout
            .location(&Panel::Moves)
            .ok()
            .or_else(|| self.hidden.iter().find(|(panel, _)| *panel == Panel::Moves).map(|(_, location)| location.clone()));
        location.is_some_and(|location| matches!(location.placement, Placement::Split(Zone::Left)))
    }

    /// Put the Move list against the page's left or right edge, [`MOVES_WIDTH`] wide, open or
    /// closed as it was.
    pub fn set_moves_side(&mut self, left: bool) -> Result<(), String> {
        let was_open = self.layout.contains(&Panel::Moves);
        self.set_visible(Panel::Moves, true)?;
        let placement = Placement::Split(if left { Zone::Left } else { Zone::Right });
        // [`Self::apply`] gives it its opening width.
        self.apply(Action::Move { panel: Panel::Moves, anchor: Panel::Canvas, placement })?;
        if !was_open {
            self.set_visible(Panel::Moves, false)?;
        }
        Ok(())
    }

    /// The split holding `panel` alone on one side gives it the width it opens at
    /// ([`Panel::opening_width`]), when it has one: a new split would give it half the page.
    fn fit_opening(&mut self, panel: Panel) {
        if let Some(width) = panel.opening_width() {
            self.fit_width(panel, width);
        }
    }

    /// Give `panel`, alone in its group at one side of a split across, `width` points.
    fn fit_width(&mut self, panel: Panel, width: f32) {
        // A valid tree has a level per panel at most: the depth only bounds a damaged one.
        fn fit(node: &mut Node<Panel>, panel: Panel, width: f32, depth: usize) -> bool {
            let Node::Split { axis, size, first, second } = node else { return false };
            let alone = |node: &Node<Panel>| matches!(node, Node::Tabs { panels, .. } if panels.as_slice() == [panel]);
            if *axis == SplitAxis::Horizontal && alone(first) {
                *size = SplitSize::FixedFirst(width);
                return true;
            }
            if *axis == SplitAxis::Horizontal && alone(second) {
                *size = SplitSize::FixedSecond(width);
                return true;
            }
            depth < 32 && (fit(first, panel, width, depth + 1) || fit(second, panel, width, depth + 1))
        }
        if let Some(root) = &mut self.layout.root {
            fit(root, panel, width, 0);
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.layout.validate().map_err(|error| error.to_string())?;
        if !self.layout.contains(&Panel::Canvas) || self.hidden.len() > Panel::ALL.len() {
            return Err("The panel layout must contain its document view and bounded hidden panels".into());
        }
        // Saved settings cannot turn the protected document view into a floating or closable tab.
        if self.layout.floating.iter().any(|group| group.panels.contains(&Panel::Canvas)) {
            return Err("The document view cannot float".into());
        }
        let mut nodes: Vec<_> = self.layout.root.iter().collect();
        while let Some(node) = nodes.pop() {
            match node {
                Node::Split { first, second, .. } => {
                    nodes.push(first);
                    nodes.push(second);
                }
                Node::Tabs { panels, .. } if panels.contains(&Panel::Canvas) && panels.len() != 1 => {
                    return Err("The document view cannot share a tab group".into());
                }
                Node::Stack { entries } if entries.iter().any(|entry| entry.panel == Panel::Canvas) => {
                    return Err("The document view cannot be collapsed".into());
                }
                _ => {}
            }
        }
        let mut seen = std::collections::HashSet::new();
        for (panel, location) in &self.hidden {
            if *panel == Panel::Canvas || self.layout.contains(panel) || !seen.insert(*panel) {
                return Err("A hidden panel has an invalid or duplicate identity".into());
            }
            if location.anchor == Some(Panel::Canvas) && matches!(location.placement, Placement::Tab { .. } | Placement::Split(Zone::Center)) {
                return Err("A hidden panel cannot restore over the document view".into());
            }
            let mut probe = self.layout.clone();
            if let Err(error) = probe.restore(*panel, location)
                && error != craft_ui::docking::DockError::MissingTarget
            {
                return Err(error.to_string());
            }
        }
        Ok(())
    }

    pub fn apply(&mut self, action: Action<Panel>) -> Result<(), String> {
        self.validate()?;
        let mut next = self.clone();
        if let Action::Close { panel } = &action {
            let location = next.layout.location(panel).map_err(|error| error.to_string())?;
            next.hidden.retain(|(id, _)| id != panel);
            next.hidden.push((*panel, location));
        } else if let Action::Open { panel, .. } | Action::OpenAt { panel, .. } = &action {
            next.hidden.retain(|(id, _)| id != panel);
        }
        // Put beside another panel, a panel goes into a split of its own (dropped there by a drag).
        let beside = match &action {
            Action::Move { panel, placement: Placement::Split(Zone::Left | Zone::Right), .. }
            | Action::OpenAt { panel, placement: Placement::Split(Zone::Left | Zone::Right), .. } => Some(*panel),
            _ => None,
        };
        next.layout.apply_with_permissions(action, permissions).map_err(|error| error.to_string())?;
        if let Some(panel) = beside {
            next.fit_opening(panel);
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Whether a panel's body is exposed by its selected tab or open stack entry.
    pub(crate) fn exposed(&self, panel: Panel) -> bool {
        exposed_in(&self.layout, panel)
    }

    pub fn set_visible(&mut self, panel: Panel, visible: bool) -> Result<(), String> {
        if self.layout.contains(&panel) == visible {
            return Ok(());
        }
        if !visible {
            return self.apply(Action::Close { panel });
        }
        self.validate()?;
        let mut next = self.clone();
        let mut location = next.hidden.iter().find(|(id, _)| *id == panel).map(|(_, location)| location.clone());
        next.hidden.retain(|(id, _)| *id != panel);
        if let Some(saved) = &mut location
            && saved.floating.is_some()
            && saved.anchor.as_ref().is_some_and(|anchor| !next.layout.floating.iter().any(|group| group.panels.contains(anchor)))
        {
            // The other members of a floating group may have been closed too.
            saved.anchor = None;
            saved.placement = Placement::Tab { before: None };
        }
        let restored = location.as_ref().is_some_and(|location| next.layout.restore(panel, location).is_ok());
        // The Diagram tab opens between the page and the Inspector, so the diagrams listed there
        // stay in sight. A layout saved before the tab existed has no place for it, so it opens
        // there too.
        let restored = restored
            || (panel == Panel::Diagram
                && next.layout.contains(&Panel::Inspector)
                && next
                    .layout
                    .apply_with_permissions(Action::OpenAt { panel, anchor: Panel::Canvas, placement: Placement::Split(Zone::Right) }, permissions)
                    .is_ok()
                && {
                    next.fit_opening(Panel::Diagram);
                    true
                });
        // The board opens as a tab beside the Inspector, where the diagrams it shows are listed.
        let restored = restored
            || (panel == Panel::Board
                && next.layout.contains(&Panel::Inspector)
                && next
                    .layout
                    .apply_with_permissions(
                        Action::OpenAt { panel, anchor: Panel::Inspector, placement: Placement::Tab { before: None } },
                        permissions,
                    )
                    .is_ok());
        if !restored {
            next.layout
                .apply_with_permissions(
                    Action::OpenAt { panel, anchor: Panel::Canvas, placement: Placement::Split(panel.default_side()) },
                    permissions,
                )
                .map_err(|error| error.to_string())?;
            // In a split of its own beside the page: at its opening width, not half the page.
            next.fit_opening(panel);
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    fn command(&mut self, params: &Value) -> Result<(), String> {
        self.validate()?;
        if let Some(action) = params.get("action") {
            let action: Action<Panel> = serde_json::from_value(action.clone()).map_err(|error| error.to_string())?;
            return self.apply(action);
        }
        let operation = params.get("operation").and_then(Value::as_str).ok_or("operation is required")?;
        if operation == "resizeSplit" {
            let path: Vec<bool> =
                serde_json::from_value(params.get("path").cloned().ok_or("path is required")?).map_err(|error| error.to_string())?;
            let size: SplitSize =
                serde_json::from_value(params.get("size").cloned().ok_or("size is required")?).map_err(|error| error.to_string())?;
            return self.apply(Action::ResizeSplit { path, size });
        }
        let panel = Panel::parse(params.get("panel").and_then(Value::as_str).ok_or("panel is required")?)?;
        let action = match operation {
            "open" => {
                self.set_visible(panel, true)?;
                return self.apply(Action::Activate { panel });
            }
            "close" => Action::Close { panel },
            "activate" => Action::Activate { panel },
            "setStackOpen" => Action::SetStackOpen { panel, open: params.get("open").and_then(Value::as_bool).ok_or("open must be a boolean")? },
            "resizeStack" => {
                let value = params.get("height").ok_or("height is required")?;
                let height = if value.is_null() {
                    None
                } else {
                    Some(
                        value
                            .as_f64()
                            .filter(|height| height.is_finite() && (0.0..=1_000_000.0).contains(height))
                            .ok_or("height must be finite and bounded")? as f32,
                    )
                };
                Action::ResizeStack { panel, height }
            }
            "float" | "moveFloating" => {
                let rect = params.get("rect").and_then(Value::as_array).filter(|rect| rect.len() == 4).ok_or("rect must be [x,y,width,height]")?;
                let mut values = [0.0f32; 4];
                for (output, value) in values.iter_mut().zip(rect) {
                    *output = value
                        .as_f64()
                        .filter(|number| number.is_finite() && number.abs() <= 1_000_000.0)
                        .ok_or("rect values must be finite and bounded")? as f32;
                }
                if operation == "float" { Action::Float { panel, rect: values } } else { Action::MoveFloating { panel, rect: values } }
            }
            "move" => {
                let anchor = Panel::parse(params.get("target").and_then(Value::as_str).ok_or("target is required")?)?;
                let placement = match params.get("zone").and_then(Value::as_str).unwrap_or("center") {
                    "center" => Placement::Split(Zone::Center),
                    "left" => Placement::Split(Zone::Left),
                    "right" => Placement::Split(Zone::Right),
                    "top" => Placement::Split(Zone::Top),
                    "bottom" => Placement::Split(Zone::Bottom),
                    _ => return Err("zone must be center, left, right, top or bottom".into()),
                };
                let placement = if let Some(before) = params.get("before") {
                    if !matches!(placement, Placement::Split(Zone::Center)) {
                        return Err("before only applies to a center move".into());
                    }
                    Placement::Tab {
                        before: if before.is_null() {
                            None
                        } else {
                            Some(Panel::parse(before.as_str().ok_or("before must be a panel ID or null")?)?)
                        },
                    }
                } else {
                    placement
                };
                Action::Move { panel, anchor, placement }
            }
            _ => return Err("operation must be open, close, activate, float, move, moveFloating, resizeSplit, setStackOpen or resizeStack".into()),
        };
        self.apply(action)
    }
}

/// Whether `panel`'s body is exposed in `layout` by its selected tab or open stack entry.
fn exposed_in(layout: &Layout<Panel>, panel: Panel) -> bool {
    if layout.floating.iter().any(|group| group.panels.get(group.active) == Some(&panel)) {
        return true;
    }
    let mut pending: Vec<_> = layout.root.iter().collect();
    while let Some(node) = pending.pop() {
        match node {
            Node::Split { first, second, .. } => {
                pending.push(first);
                pending.push(second);
            }
            Node::Tabs { panels, active } if panels.get(*active) == Some(&panel) => return true,
            Node::Stack { entries } if entries.iter().any(|entry| entry.panel == panel && entry.open) => return true,
            _ => {}
        }
    }
    false
}

/// What was asked of the Move list since the layout was last drawn, done on `workspace`.
pub(crate) fn drain_moves(workspace: &mut Workspace, request: Option<MovesRequest>) -> Result<(), String> {
    match request {
        None => Ok(()),
        Some(MovesRequest::Show(false)) => workspace.set_visible(Panel::Moves, false),
        Some(MovesRequest::Show(true)) => {
            workspace.set_visible(Panel::Moves, true)?;
            workspace.apply(Action::Activate { panel: Panel::Moves })
        }
        Some(MovesRequest::Side { left }) => workspace.set_moves_side(left),
    }
}

fn sync(workspace: &mut Workspace, app: &crate::PdfCraftApp) -> Result<(), String> {
    workspace.set_visible(Panel::Tools, app.left_open)?;
    workspace.set_visible(Panel::Inspector, app.right.is_some())
}

fn drain_reveals(workspace: &mut Workspace, app: &mut crate::PdfCraftApp) {
    for panel in std::mem::take(&mut app.pending_panel_reveals) {
        // The board's and the move list's visibility is their own (the others follow the app's
        // state): revealing opens them.
        if matches!(panel, Panel::Board | Panel::Moves | Panel::Diagram)
            && let Err(error) = workspace.set_visible(panel, true)
        {
            app.notify_fmt("Panel move was not applied: {error}", &[("error", &error)]);
        }
        if workspace.layout.contains(&panel) {
            let _ = workspace.apply(Action::Activate { panel });
        }
    }
}

fn publish_visibility(app: &mut crate::PdfCraftApp, workspace: &Workspace) {
    app.left_open = workspace.layout.contains(&Panel::Tools);
    if !workspace.layout.contains(&Panel::Inspector) {
        if app.right.is_some() {
            app.choose_right_panel(None);
        }
    } else if app.right.is_none() {
        app.choose_right_panel(Some(crate::RightPanel::Comments));
    }
}

pub fn command(app: &mut crate::PdfCraftApp, reset: bool, params: &Value) -> Result<Value, String> {
    let mut workspace = app.docking.clone();
    if reset {
        workspace = Workspace::default();
        workspace.set_visible(Panel::Inspector, app.right.is_some())?;
    } else {
        sync(&mut workspace, app)?;
        workspace.command(params)?;
    }
    publish_visibility(app, &workspace);
    app.docking = workspace;
    Ok(json!({"docking": app.docking}))
}

// Split paths change when a temporarily unavailable pane is pruned. Translate renderer
// paths back to the saved tree before applying geometry changes.
fn saved_split_path(saved: &Layout<Panel>, rendered: &Layout<Panel>, path: &[bool]) -> Option<Vec<bool>> {
    let mut target = rendered.root.as_ref()?;
    for second in path {
        let Node::Split { first, second: other, .. } = target else { return None };
        target = if *second { other } else { first };
    }
    let mut pending = vec![(saved.root.as_ref()?, Vec::new())];
    while let Some((node, original_path)) = pending.pop() {
        if let Node::Split { first, second, .. } = node {
            let mut projection = Layout { root: Some(node.clone()), floating: Vec::new() };
            if projection.contains(&Panel::Inspector) {
                let _ = projection.apply(Action::Close { panel: Panel::Inspector });
            }
            let has_available_panel = |child: &Node<Panel>| {
                Layout { root: Some(child.clone()), floating: Vec::new() }.panels().into_iter().any(|panel| *panel != Panel::Inspector)
            };
            // An ancestor whose Inspector-only side disappeared projects to its surviving
            // child. Its divider is absent, so only the child's split may own this resize.
            if has_available_panel(first) && has_available_panel(second) && projection.root.as_ref() == Some(target) {
                return Some(original_path);
            }
            let mut second_path = original_path.clone();
            second_path.push(true);
            pending.push((second, second_path));
            let mut first_path = original_path;
            first_path.push(false);
            pending.push((first, first_path));
        }
    }
    None
}

pub fn show(app: &mut crate::PdfCraftApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let mut workspace = std::mem::take(&mut app.docking);
    if let Err(error) = workspace.validate().and_then(|()| sync(&mut workspace, app)) {
        app.notify_fmt("Panel layout could not be restored: {error}", &[("error", &error)]);
        workspace = Workspace::default();
        let _ = sync(&mut workspace, app);
    }
    let mut style = DockStyle::from_ui(ui);
    style.min_pane = 96.0;
    style.float_label = crate::i18n::t("Float panel").into();
    style.close_label = crate::i18n::t("Close panel").into();
    style.move_label = crate::i18n::t("Move to group").into();
    style.panels_label = crate::i18n::t("Panels").into();
    style.resize_label = crate::i18n::t("Resize panels").into();
    style.resize_window_label = crate::i18n::t("Resize panel window").into();
    style.background = t.panel;
    style.tab_background = t.chrome;
    style.active_background = t.selected;
    style.text = t.text;
    style.inactive_text = t.text_muted;
    style.border = egui::Stroke::new(1.0, t.divider);
    style.accent = t.accent;
    style.font = crate::theme::medium(12.0);
    drain_reveals(&mut workspace, app);
    let mut rendered = workspace.layout.clone();
    if app.active.is_none() && rendered.contains(&Panel::Inspector) {
        // Availability is a render projection: the saved group, selection and width survive Home.
        let _ = rendered.apply(Action::Close { panel: Panel::Inspector });
    }
    // The notation is drawn in one place: the Move list while it shows, else under the board.
    app.chess.moves_beside = exposed_in(&rendered, Panel::Moves);
    let output = DockArea::new(egui::Id::new("pdfcraft-docking")).show_customized(
        ui,
        &rendered,
        &style,
        |panel| crate::i18n::t(panel.label()).into(),
        permissions,
        panel_limits,
        &mut Content { app: &mut *app, t },
    );
    let _ = sync(&mut workspace, app);
    drain_reveals(&mut workspace, app);
    publish_visibility(app, &workspace);
    app.docking = workspace;
    for mut action in output.actions {
        if app.active.is_none()
            && let Action::ResizeSplit { path, .. } = &mut action
        {
            let Some(saved_path) = saved_split_path(&app.docking.layout, &rendered, path) else { continue };
            *path = saved_path;
        }
        if let Err(error) =
            serde_json::to_value(action).map_err(|error| error.to_string()).and_then(|action| command(app, false, &json!({"action": action})))
        {
            app.notify_fmt("Panel move was not applied: {error}", &[("error", &error)]);
        }
    }
}

/// The panels' bodies, and the app's items in a tab's context menu.
struct Content<'a> {
    app: &'a mut crate::PdfCraftApp,
    t: crate::theme::Tokens,
}

impl DockContent<Panel> for Content<'_> {
    fn body(&mut self, ui: &mut egui::Ui, panel: &Panel) {
        let (app, t) = (&mut *self.app, &self.t);
        match panel {
            Panel::Canvas => {
                ui.painter().rect_filled(ui.max_rect(), 0.0, t.pasteboard);
                match app.active {
                    None if app.combine_showing() => crate::combine_ui::page(app, ui),
                    None => crate::home::show(app, ui),
                    Some(index) => match crate::compare_ui::side_by_side(app, index) {
                        Some((old, new)) => crate::compare_ui::compare_area(app, old, new, ui),
                        None => crate::canvas::document_area(app, index, ui),
                    },
                }
            }
            Panel::Tools => crate::panels::left_panel(app, ui),
            Panel::Inspector => {
                ui.data_mut(|data| data.insert_temp(egui::Id::new("pdfcraft-inspector-body"), ui.max_rect()));
                crate::panels::right_panel(app, ui);
            }
            Panel::Board => crate::chess_ui::board_panel(app, ui),
            Panel::Moves => crate::chess_ui::moves_panel(app, ui),
            Panel::Diagram => crate::chess_diagram_tab::panel(app, ui),
        }
    }

    fn panel_menu(&mut self, ui: &mut egui::Ui, panel: &Panel) {
        // The Board tab: the board in its own window (T13a).
        crate::chess_window::dock_menu(self.app, ui, *panel);
        // The Move list's tab: the page's edge it stands against.
        if *panel == Panel::Moves {
            crate::chess_ui::moves_side_menu(self.app, ui);
        }
    }
}

/// The page's least width beside the panels, in points.
pub(crate) const PAGE_MIN_WIDTH: f32 = 160.0;

/// The widest a panel beside the page may be dragged when `available` points are left for it
/// and the page: all but [`PAGE_MIN_WIDTH`], and never under its own `least` width.
pub(crate) fn widest_beside_page(available: f32, least: f32) -> f32 {
    if available.is_finite() { (available - PAGE_MIN_WIDTH).max(least) } else { least }
}

fn panel_limits(panel: &Panel) -> PanelLimits {
    // The right-hand column and the Move list are as wide as they are dragged: only the page's
    // least width stops them. The whole column, since the board is a tab beside the Inspector
    // and a tab group is no wider than the widest each of its panels allows.
    let (min, max_width) = match panel {
        Panel::Canvas => (egui::vec2(PAGE_MIN_WIDTH, 160.0), 1_000_000.0),
        Panel::Tools => (egui::vec2(240.0, 120.0), 520.0),
        Panel::Inspector | Panel::Board | Panel::Diagram => (egui::vec2(260.0, 120.0), 1_000_000.0),
        Panel::Moves => (egui::vec2(180.0, 120.0), 1_000_000.0),
    };
    PanelLimits { min, max: egui::vec2(max_width, 1_000_000.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panel beside the page in Read mode may take all but the page's least width, never less
    /// than its own least, whatever width it is given.
    #[test]
    fn a_panel_beside_the_page_leaves_the_page_its_least_width() {
        assert_eq!(widest_beside_page(1400.0, 260.0), 1400.0 - PAGE_MIN_WIDTH);
        assert_eq!(widest_beside_page(300.0, 260.0), 260.0);
        for odd in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -50.0, 0.0] {
            assert_eq!(widest_beside_page(odd, 180.0), 180.0, "{odd}");
        }
    }

    /// A panel of the right-hand column opened or dropped beside the page in a split of its own
    /// takes its opening width there, not half the page: no widest stops it at 520 pt any more.
    /// The Tools keep the split the drop makes.
    #[test]
    fn side_panels_take_their_width_in_a_split_of_their_own() {
        fn width(workspace: &Workspace, panel: Panel) -> Option<SplitSize> {
            fn find(node: &Node<Panel>, panel: Panel) -> Option<SplitSize> {
                let Node::Split { size, first, second, .. } = node else { return None };
                let alone = |node: &Node<Panel>| matches!(node, Node::Tabs { panels, .. } if panels.as_slice() == [panel]);
                if alone(first) || alone(second) { Some(*size) } else { find(first, panel).or_else(|| find(second, panel)) }
            }
            workspace.layout.root.as_ref().and_then(|root| find(root, panel))
        }
        let mut workspace = Workspace::default();
        workspace.set_visible(Panel::Inspector, false).unwrap();
        // No Inspector to be a tab beside: each in a split of its own.
        workspace.set_visible(Panel::Board, true).unwrap();
        assert_eq!(width(&workspace, Panel::Board), Some(SplitSize::FixedSecond(SIDE_WIDTH)));
        workspace.set_visible(Panel::Diagram, true).unwrap();
        assert_eq!(width(&workspace, Panel::Diagram), Some(SplitSize::FixedSecond(DIAGRAM_WIDTH)));
        // Dragged to the page's left edge.
        workspace.apply(Action::Move { panel: Panel::Board, anchor: Panel::Canvas, placement: Placement::Split(Zone::Left) }).unwrap();
        assert_eq!(width(&workspace, Panel::Board), Some(SplitSize::FixedFirst(SIDE_WIDTH)));
        workspace.apply(Action::Move { panel: Panel::Tools, anchor: Panel::Canvas, placement: Placement::Split(Zone::Right) }).unwrap();
        assert!(matches!(width(&workspace, Panel::Tools), Some(SplitSize::Ratio(_))), "{:?}", workspace.layout);
    }

    #[test]
    fn lithuanian_translates_every_dynamic_panel_title() {
        let lang = crate::i18n::Lang::from_code("lt").expect("Lithuanian is registered");
        for panel in Panel::ALL {
            assert_ne!(crate::i18n::tr(lang, panel.label()), panel.label(), "{} remains untranslated", panel.id());
        }
    }

    #[test]
    fn move_list_opens_against_the_page_at_either_edge_and_keeps_its_side_closed() {
        // The width the split gives the list, and the side of the page it stands on.
        fn width(node: &Node<Panel>) -> Option<SplitSize> {
            let Node::Split { size, first, second, .. } = node else { return None };
            let alone = |node: &Node<Panel>| matches!(node, Node::Tabs { panels, .. } if panels.as_slice() == [Panel::Moves]);
            if alone(first) || alone(second) { Some(*size) } else { width(first).or_else(|| width(second)) }
        }
        let beside = |workspace: &Workspace, zone: Zone| {
            let location = workspace.layout.location(&Panel::Moves).unwrap();
            location.anchor == Some(Panel::Canvas) && location.placement == Placement::Split(zone)
        };
        let mut workspace = Workspace::default();
        workspace.set_visible(Panel::Moves, true).unwrap();
        assert!(workspace.exposed(Panel::Moves) && !workspace.moves_on_left());
        assert!(beside(&workspace, Zone::Right), "{:?}", workspace.layout);
        assert_eq!(workspace.layout.root.as_ref().and_then(width), Some(SplitSize::FixedSecond(MOVES_WIDTH)), "not half the page");
        workspace.set_moves_side(true).unwrap();
        assert!(workspace.moves_on_left() && beside(&workspace, Zone::Left), "{:?}", workspace.layout);
        assert_eq!(workspace.layout.root.as_ref().and_then(width), Some(SplitSize::FixedFirst(MOVES_WIDTH)));
        assert!(workspace.layout.contains(&Panel::Tools) && workspace.layout.contains(&Panel::Inspector), "the others stay");
        // Closed, it keeps its side; moved while closed, it opens there.
        drain_moves(&mut workspace, Some(MovesRequest::Show(false))).unwrap();
        assert!(!workspace.layout.contains(&Panel::Moves) && workspace.moves_on_left());
        drain_moves(&mut workspace, Some(MovesRequest::Show(true))).unwrap();
        assert!(beside(&workspace, Zone::Left));
        workspace.set_visible(Panel::Moves, false).unwrap();
        drain_moves(&mut workspace, Some(MovesRequest::Side { left: false })).unwrap();
        assert!(!workspace.layout.contains(&Panel::Moves) && !workspace.moves_on_left());
        workspace.set_visible(Panel::Moves, true).unwrap();
        assert!(beside(&workspace, Zone::Right));
        workspace.validate().unwrap();
    }

    #[test]
    fn tools_side_moves_to_both_edges_and_survives_closed_restore() {
        let mut workspace = Workspace::default();
        assert!(!workspace.tools_on_right());
        workspace.set_tools_on_right(true).unwrap();
        assert!(workspace.tools_on_right());
        workspace.set_visible(Panel::Tools, false).unwrap();
        assert!(workspace.tools_on_right());
        workspace.set_tools_on_right(false).unwrap();
        assert!(!workspace.tools_on_right());
        assert!(!workspace.layout.contains(&Panel::Tools));
        workspace.set_visible(Panel::Tools, true).unwrap();
        assert!(!workspace.tools_on_right());
        workspace.validate().unwrap();
    }

    #[test]
    fn exposed_respects_collapsed_stack_and_inactive_floating_tabs() {
        let mut workspace = Workspace {
            layout: Layout {
                root: Some(split(
                    SplitAxis::Horizontal,
                    SplitSize::FixedFirst(272.0),
                    Node::Stack {
                        entries: vec![
                            craft_ui::docking::StackEntry { panel: Panel::Tools, open: true, height: None },
                            craft_ui::docking::StackEntry { panel: Panel::Inspector, open: false, height: None },
                        ],
                    },
                    tabs(Panel::Canvas),
                )),
                floating: Vec::new(),
            },
            hidden: Vec::new(),
        };
        workspace.validate().unwrap();
        assert!(workspace.exposed(Panel::Tools));
        assert!(!workspace.exposed(Panel::Inspector));
        workspace.apply(Action::Activate { panel: Panel::Inspector }).unwrap();
        assert!(workspace.exposed(Panel::Inspector));
        workspace.apply(Action::Float { panel: Panel::Tools, rect: [10.0, 20.0, 300.0, 400.0] }).unwrap();
        workspace.apply(Action::Move { panel: Panel::Inspector, anchor: Panel::Tools, placement: Placement::Split(Zone::Center) }).unwrap();
        assert!(!workspace.exposed(Panel::Tools));
        assert!(workspace.exposed(Panel::Inspector));
    }

    #[test]
    fn unavailable_inspector_split_resize_targets_saved_tools_split() {
        let mut workspace = Workspace::default();
        // Inspector at the outer edge makes the visible Tools split move to the root.
        workspace.apply(Action::Move { panel: Panel::Inspector, anchor: Panel::Tools, placement: Placement::Split(Zone::Left) }).unwrap();
        let mut rendered = workspace.layout.clone();
        rendered.apply(Action::Close { panel: Panel::Inspector }).unwrap();
        let path = saved_split_path(&workspace.layout, &rendered, &[]).unwrap();
        workspace.apply(Action::ResizeSplit { path, size: SplitSize::FixedFirst(290.0) }).unwrap();
        let mut resized = workspace.layout.clone();
        resized.apply(Action::Close { panel: Panel::Inspector }).unwrap();
        assert!(matches!(resized.root, Some(Node::Split { size: SplitSize::FixedFirst(290.0), .. })));
    }

    #[test]
    fn unavailable_outer_inspector_does_not_capture_its_childs_resize() {
        let mut workspace = Workspace {
            layout: Layout {
                root: Some(split(
                    SplitAxis::Horizontal,
                    SplitSize::FixedSecond(330.0),
                    split(SplitAxis::Horizontal, SplitSize::FixedFirst(272.0), tabs(Panel::Tools), tabs(Panel::Canvas)),
                    tabs(Panel::Inspector),
                )),
                floating: Vec::new(),
            },
            hidden: Vec::new(),
        };
        workspace.validate().unwrap();
        let mut rendered = workspace.layout.clone();
        rendered.apply(Action::Close { panel: Panel::Inspector }).unwrap();
        let path = saved_split_path(&workspace.layout, &rendered, &[]).unwrap();
        assert_eq!(path, vec![false]);
        workspace.apply(Action::ResizeSplit { path, size: SplitSize::FixedFirst(290.0) }).unwrap();
        assert!(matches!(workspace.layout.root, Some(Node::Split { size: SplitSize::FixedSecond(330.0), .. })));
        let mut resized = workspace.layout.clone();
        resized.apply(Action::Close { panel: Panel::Inspector }).unwrap();
        assert!(matches!(resized.root, Some(Node::Split { size: SplitSize::FixedFirst(290.0), .. })));
    }
}
