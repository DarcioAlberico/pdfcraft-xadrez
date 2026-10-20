//! The app's native windows (egui viewports) by name, and which of them has the keyboard.
//!
//! A separate app window is an immediate viewport (`Context::show_viewport_immediate`): eframe
//! runs its egui pass, with its own input, inside the main window's pass. So keys typed there
//! never reach the main window's input, and the main window reports losing the focus when the
//! user clicks into it. The control channel addresses these windows by name (`ui.inspect
//! {"viewport": "chess"}`), and the app reads its shortcuts from the window with the focus.
//!
//! Nothing here opens a window: with none open, every function answers for the main window
//! alone, exactly as before separate windows existed.

use egui::ViewportId;

/// The main window's name on the control channel.
pub const ROOT: &str = "root";

/// The key the chess window's [`ViewportId`] is hashed from.
const CHESS_KEY: &str = "pdfcraft-chess";

/// The chess window's control-channel name.
pub const CHESS: &str = "chess";

/// The separate windows the app may open: control-channel name and the key their [`ViewportId`]
/// is hashed from. A window gets an entry here when it gets its `show_viewport_immediate` call.
const WINDOWS: &[(&str, &str)] = &[(CHESS, CHESS_KEY)];

/// The chess window's viewport.
pub fn chess() -> ViewportId {
    ViewportId::from_hash_of(CHESS_KEY)
}

/// The viewport named `name` (`root`, or one of the separate windows), if there is one.
pub fn id(name: &str) -> Option<ViewportId> {
    if name == ROOT {
        return Some(ViewportId::ROOT);
    }
    WINDOWS.iter().find(|(n, _)| *n == name).map(|(_, key)| ViewportId::from_hash_of(key))
}

/// The control-channel name of viewport `id`, if it is one of the app's.
pub fn name(id: ViewportId) -> Option<&'static str> {
    if id == ViewportId::ROOT {
        return Some(ROOT);
    }
    WINDOWS.iter().find(|(_, key)| ViewportId::from_hash_of(key) == id).map(|(n, _)| *n)
}

/// Every name [`id`] accepts, for error messages.
pub fn names() -> impl Iterator<Item = &'static str> {
    std::iter::once(ROOT).chain(WINDOWS.iter().map(|(n, _)| *n))
}

fn windows() -> impl Iterator<Item = ViewportId> {
    WINDOWS.iter().map(|(_, key)| ViewportId::from_hash_of(key))
}

/// Whether `id` is open as a native window now: the backend lists it among the viewports it
/// gave this frame's input. The main window always is. An embedded window (`embed_viewports`:
/// the web, kittest) is drawn inside the main one and never is.
pub(crate) fn is_open(ctx: &egui::Context, id: ViewportId) -> bool {
    id == ViewportId::ROOT || ctx.input_for(ViewportId::ROOT, |i| i.raw.viewports.contains_key(&id))
}

/// The app window with the keyboard focus: the main window when it has it, else a separate
/// window whose last reported state says so. `None` when another application has it.
///
/// A separate window's state is the one eframe recorded when it last drew it, during the main
/// window's previous pass: for one pass after a click moves the focus into it, neither window
/// reports having it ([`FocusLoss`] waits that pass out).
pub(crate) fn focused(ctx: &egui::Context) -> Option<ViewportId> {
    ctx.input_for(ViewportId::ROOT, |i| {
        if i.focused {
            return Some(ViewportId::ROOT);
        }
        windows().find(|id| i.raw.viewports.get(id).is_some_and(|info| info.focused == Some(true)))
    })
}

/// The window whose input the app's shortcuts read: the focused app window, the main window
/// when none has the focus (as before separate windows existed).
pub(crate) fn input_viewport(ctx: &egui::Context) -> ViewportId {
    focused(ctx).unwrap_or(ViewportId::ROOT)
}

/// Whether a separate app window is open.
fn any_window_open(ctx: &egui::Context) -> bool {
    windows().any(|id| is_open(ctx, id))
}

/// Whether the app as a whole has lost the keyboard focus, which cancels transient gestures
/// (autoscroll, drag previews) so they don't resume when the user comes back.
///
/// The focus moving between the app's own windows is not a loss. Because a separate window
/// reports its focus a pass late (see [`focused`]), a pass where no app window has the focus
/// while a separate window is open is only counted as a loss if the next pass agrees.
#[derive(Debug, Default)]
pub(crate) struct FocusLoss {
    /// The previous pass found no app window focused while a separate window was open.
    pending: bool,
}

impl FocusLoss {
    /// Call once per frame, from the main window's pass.
    pub(crate) fn lost(&mut self, ctx: &egui::Context) -> bool {
        self.update(focused(ctx).is_some(), any_window_open(ctx))
    }

    fn update(&mut self, app_focused: bool, window_open: bool) -> bool {
        if app_focused {
            self.pending = false;
            return false;
        }
        if window_open && !self.pending {
            self.pending = true;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_ids_round_trip() {
        assert_eq!(id("root"), Some(ViewportId::ROOT));
        assert_eq!(id("chess"), Some(chess()));
        assert_ne!(chess(), ViewportId::ROOT);
        assert_eq!(id("Chess"), None, "names are exact");
        assert_eq!(id(""), None);
        assert_eq!(name(chess()), Some("chess"));
        assert_eq!(name(ViewportId::ROOT), Some("root"));
        assert_eq!(name(ViewportId::from_hash_of("someone else's")), None);
        assert_eq!(names().collect::<Vec<_>>(), ["root", "chess"]);
    }

    #[test]
    fn focus_loss_waits_one_pass_only_while_a_window_is_open() {
        // No separate window: lost at once, as before.
        let mut f = FocusLoss::default();
        assert!(f.update(false, false));
        assert!(!f.update(true, false));
        // A window open: the first unfocused pass may be the focus moving into it.
        assert!(!f.update(false, true), "grace pass");
        assert!(f.update(false, true), "still nobody's: lost");
        assert!(f.update(false, true));
        // Focus back: the grace is available again.
        assert!(!f.update(true, true));
        assert!(!f.update(false, true));
        assert!(!f.update(true, true), "the window reported it: not lost");
        assert!(!f.update(false, true));
    }

    #[test]
    fn a_focused_window_routes_input_and_closed_ones_are_ignored() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput { focused: false, ..Default::default() };
        input.viewports.insert(ViewportId::ROOT, egui::ViewportInfo::default());
        // Run a pass so the main viewport's input holds `input`.
        let run = |ctx: &egui::Context, input: &egui::RawInput| {
            ctx.run_ui(input.clone(), |_| {}).textures_delta.clear();
        };
        run(&ctx, &input);
        assert_eq!(focused(&ctx), None);
        assert_eq!(input_viewport(&ctx), ViewportId::ROOT);
        assert!(!is_open(&ctx, chess()));
        input.viewports.insert(chess(), egui::ViewportInfo { focused: Some(false), ..Default::default() });
        run(&ctx, &input);
        assert!(is_open(&ctx, chess()));
        assert_eq!(focused(&ctx), None);
        input.viewports.insert(chess(), egui::ViewportInfo { focused: Some(true), ..Default::default() });
        run(&ctx, &input);
        assert_eq!(focused(&ctx), Some(chess()));
        assert_eq!(input_viewport(&ctx), chess());
        // A foreign viewport reporting focus is not an app window.
        input.viewports.remove(&chess());
        input.viewports.insert(ViewportId::from_hash_of("other"), egui::ViewportInfo { focused: Some(true), ..Default::default() });
        run(&ctx, &input);
        assert_eq!(focused(&ctx), None);
        // The main window wins when it has the focus.
        input.focused = true;
        input.viewports.insert(chess(), egui::ViewportInfo { focused: Some(true), ..Default::default() });
        run(&ctx, &input);
        assert_eq!(focused(&ctx), Some(ViewportId::ROOT));
    }
}
