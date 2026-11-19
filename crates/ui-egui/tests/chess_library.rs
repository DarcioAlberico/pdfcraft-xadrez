//! Chess ▸ Library… (plan A9-N15c, finding A8-08; decisions D22 and D31), in the real shell
//! (egui_kittest): two synthetic books in a folder of their own, built into the Library, searched
//! for a position both reach, and a result opened at its page; the build stopped from outside the
//! dialog and continued; a book that allows no copying said in words; what can't be read never a
//! panic. The engine's half is `pdfcraft_engine::chess::library`'s (plans A9-N15a, N15a2, N15b).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::library::{Library, State};
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_ui_egui::PdfCraftApp;

#[path = "common/gpu.rs"]
mod gpu;

/// How long a test waits for work on another thread: a net against a hang (plan A9-Q4b).
const NET: Duration = Duration::from_secs(120);

/// A PDF of 600 × 800 pt pages, each with its line (empty: a blank page) in Helvetica near the
/// top; its pages numbered from 11 (`/PageLabels`), as a book's are.
fn book(pages: &[&str]) -> Vec<u8> {
    let n = pages.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R /PageLabels << /Nums [0 << /S /D /St 11 >>] >> >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    for (i, line) in pages.iter().enumerate() {
        let content = if line.is_empty() { String::new() } else { format!("BT /F2 11 Tf 60 730 Td ({line}) Tj ET\n") };
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F2 3 0 R >> >> >>", 5 + 2 * i));
        objs.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// [`book`], encrypted with `permissions` (`!0x10`: no copying, so never kept or read).
fn encrypted(pages: &[&str], permissions: i32) -> Vec<u8> {
    let mut doc = pdfcraft_cos::Document::open(Arc::new(book(pages))).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        permissions,
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    pdfcraft_cos::write_full(&doc, &Default::default()).unwrap()
}

/// A folder of its own under the temp folder, empty.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-library-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The books: `a.pdf` plays the Spanish opening on its second page (p. 12), `b.pdf` reaches the
/// same position by another order on its first (p. 11), `locked.pdf` allows no copying.
fn books(dir: &std::path::Path) {
    std::fs::write(dir.join("a.pdf"), book(&["Chapter one. The Spanish opening.", "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6"])).unwrap();
    std::fs::write(dir.join("b.pdf"), book(&["1.Nf3 Nc6 2.e4 e5 3.Bb5 a6"])).unwrap();
    std::fs::write(dir.join("locked.pdf"), encrypted(&["1.d4 d5"], !0x10)).unwrap();
}

/// The position after 3.Bb5, which both books reach (the second by another order).
fn spanish() -> String {
    let mut g = Game::standard();
    for m in ["e4", "e5", "Nf3", "Nc6", "Bb5"] {
        g.play_san(m).unwrap();
    }
    g.fen()
}

/// The app with no document open, its chess kept in `data`, in a window of `size`, in `language`
/// and `theme`, at `scale` (the interface size), with no piece model.
fn harness_in(data: &std::path::Path, size: egui::Vec2, language: &str, theme: &str, scale: &str) -> Harness<'static, PdfCraftApp> {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
    let (data, language, theme, scale) = (data.to_path_buf(), language.to_owned(), theme.to_owned(), scale.to_owned());
    let mut h = Harness::builder().with_size(size).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", &language).unwrap();
        app.set_option("theme", &theme).unwrap();
        app.set_option("ui-scale", &scale).unwrap();
        app.use_chess_data(data, None);
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

fn harness(data: &std::path::Path) -> Harness<'static, PdfCraftApp> {
    harness_in(data, egui::vec2(1366.0, 768.0), "en", "light", "100%")
}

/// Run frames until `done`, at most [`NET`].
fn wait_for(h: &mut Harness<'static, PdfCraftApp>, done: impl Fn(&Harness<'static, PdfCraftApp>) -> bool) -> bool {
    let deadline = std::time::Instant::now() + NET;
    loop {
        h.run_steps(1);
        if done(h) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Chess ▸ Library… through the registry, as the Chess menu and the palette run it.
fn open_library(h: &mut Harness<'static, PdfCraftApp>) {
    h.state_mut().run_command("chess.library");
    h.run_steps(3);
    assert!(h.state().chess.library.is_open(), "the dialog is shown");
}

/// Add folder `dir` through the dialog's Add a folder… (the picker answered by the test).
fn add_folder(h: &mut Harness<'static, PdfCraftApp>, dir: &std::path::Path) {
    h.state_mut().pick_override = Some(vec![dir.to_string_lossy().into_owned()]);
    h.get_by_label("Add a folder…").click();
    h.run_steps(3);
    assert!(h.state_mut().wait_library(NET), "the catalog changed");
    h.run_steps(3);
}

/// Whether a notice (or an error) says `text`.
fn said(h: &Harness<'static, PdfCraftApp>, text: &str) -> bool {
    h.query_all_by_label_contains(text).next().is_some()
}

/// Type `text` into the field named `label` (focused first: typing goes to the keyboard's widget).
fn type_in(h: &mut Harness<'static, PdfCraftApp>, label: &str, text: &str) {
    h.get_by_label(label).focus();
    h.run_steps(1);
    h.get_by_label(label).type_text(text);
    h.run_steps(1);
}

/// Every name on screen, for a failure's message.
fn names(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    h.query_all_by(|n| n.label().or_else(|| n.value()).is_some())
        .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
        .collect()
}

/// Run frames until the dialog stands still (what changed in it moved it: it is centred), as a
/// person sees it before clicking; at most 30.
fn settle(h: &mut Harness<'static, PdfCraftApp>) {
    let dialog = |h: &Harness<'static, PdfCraftApp>| h.query_by(|n| n.role() == Role::Dialog).map(|n| n.rect());
    let mut last = dialog(h);
    for _ in 0..30 {
        h.run_steps(1);
        let now = dialog(h);
        if now == last {
            return;
        }
        last = now;
    }
}

/// The dialogs' nodes, their roles, names and boxes, for a failure's message.
fn tree(h: &Harness<'static, PdfCraftApp>) -> String {
    let mut out = String::new();
    for d in h.query_all_by(|n| n.role() == Role::Dialog) {
        out.push_str(&format!("DIALOG {:?}\n", d.rect()));
        for n in d.query_all_by(|n| n.bounding_box().is_some()) {
            let a = n.accesskit_node();
            out.push_str(&format!("  {:?} {:?} {:?} {:?}\n", a.role(), a.label(), a.value(), n.rect()));
        }
    }
    out
}

/// The name of the widget with the keyboard.
fn focused(h: &Harness<'static, PdfCraftApp>) -> String {
    h.query_all_by(|n| n.is_focused()).find_map(|n| n.accesskit_node().label()).unwrap_or_default()
}

/// Press `key` as the desktop app gets it: through `raw_input_hook` first.
fn host_key(h: &mut Harness<'static, PdfCraftApp>, key: egui::Key) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    h.run_steps(2);
}

/// A picture of the window, when `PDFCRAFT_CHESS_SHOTS` names a folder.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        let _gpu = gpu::gpu();
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

/// The state of the book at `name` in the catalog.
fn state_of(h: &Harness<'static, PdfCraftApp>, name: &str) -> Option<State> {
    let catalog = h.state().chess.library.catalog()?;
    catalog.books.iter().find(|b| b.paths.iter().any(|p| p.ends_with(name))).map(|b| b.state)
}

/// Plan A9-N15c (the card's check): two synthetic books in a folder are built into the Library,
/// a position both reach is found in both (two results, each at its page), and a result opens its
/// PDF at the page with the move outlined. The build is stopped from the row outside the dialog,
/// closed while it runs, and goes on with Continue; the book that allows no copying is said so in
/// words; the rows are walked with ↑ ↓ Home End.
#[test]
fn two_books_are_built_searched_and_a_result_opens_at_its_page() {
    let data = temp_dir("journey");
    let shelf = data.join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    books(&shelf);
    let mut h = harness(&data);
    open_library(&mut h);
    h.get_by_label("No folder yet: add the folders where your chess books are.");
    add_folder(&mut h, &shelf);
    h.get_by_label(&shelf.to_string_lossy());
    // The estimate, before anything starts: the three books' four pages.
    assert!(wait_for(&mut h, |h| said(h, "To read: 3 books, 4 pages")), "the estimate: {:#?}", names(&h));
    h.get_by_label_contains("(an estimate)");
    assert!(said(&h, "cores of this computer"), "the cores the scan uses");
    // A build held at the second book's start, to stop it there from outside the dialog.
    let (reached, at) = channel::<()>();
    let (go, gate) = channel::<()>();
    let gate = Mutex::new(gate);
    let armed = AtomicBool::new(true);
    h.state_mut().set_library_build_hook(Arc::new(move |book, _, _| {
        if book == 1 && armed.swap(false, Ordering::SeqCst) {
            let _ = reached.send(());
            let _ = gate.lock().unwrap().recv_timeout(NET);
        }
    }));
    h.get_by_role_and_label(Role::Button, "Build").click();
    h.run_steps(2);
    at.recv_timeout(NET).expect("the build reaches the second book");
    assert!(h.state().chess.library.building(), "a job of its own thread, never inline");
    shot(&mut h, "n15c-building-dialog.png");
    // Closed while it runs: it goes on, its row at the foot of the page area, with Stop.
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(!h.state().chess.library.is_open() && h.state().chess.library.building());
    h.get_by_label_contains("Building the Library: book 2 of 3");
    shot(&mut h, "n15c-building-card.png");
    h.get_by_label("Stop").click();
    h.run_steps(2);
    h.get_by_label("Stopping after this page…");
    go.send(()).unwrap();
    assert!(h.state_mut().wait_library(NET), "the build ends");
    h.run_steps(3);
    assert!(said(&h, "The Library's build was stopped"), "said");
    assert_eq!(state_of(&h, "a.pdf"), Some(State::Indexed));
    assert_eq!(state_of(&h, "b.pdf"), Some(State::Pending), "stopped before it");
    // Reopened: the first book read, the second not yet, and Continue.
    open_library(&mut h);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(2);
    h.get_by_label_contains("Pages read: 2 · diagrams: 0");
    assert!(said(&h, "Not read yet"), "the second book");
    h.get_by_role_and_label(Role::Button, "Continue").click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET), "the build ends");
    h.run_steps(3);
    assert!(said(&h, "The Library is built"), "said");
    assert_eq!(state_of(&h, "b.pdf"), Some(State::Indexed));
    // The book that allows no copying, said in words, never as its reason's name.
    h.get_by_label("Skipped: copying not allowed");
    shot(&mut h, "n15c-books.png");
    // The search: the position both books reach, two results, each at its page.
    h.get_by_role_and_label(Role::RadioButton, "Search").click();
    h.run_steps(2);
    type_in(&mut h, "Position (FEN)", &spanish());
    h.get_by_role_and_label(Role::Button, "Search").click();
    h.run_steps(3);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(2);
    let found = h.state().chess.library.found().cloned().expect("found");
    assert_eq!(
        (found.total, found.books.len()),
        (2, 2),
        "{found:?} {:#?}",
        h.state().chess.library.catalog().map(|c| c.books.iter().map(|b| (b.paths.clone(), b.counts)).collect::<Vec<_>>())
    );
    h.get_by_label_contains("Show in PDF, p. 12 · 3. bishop b5");
    h.get_by_label_contains("Show in PDF, p. 11 · 3. bishop b5");
    shot(&mut h, "n15c-search.png");
    // The keyboard: ↓ and ↑ go from row to row.
    h.get_by_label_contains("Show in PDF, p. 12 · 3. bishop b5").focus();
    h.run_steps(1);
    host_key(&mut h, egui::Key::ArrowDown);
    assert!(focused(&h).starts_with("Show in PDF, p. 11"), "{}", focused(&h));
    host_key(&mut h, egui::Key::Home);
    assert!(focused(&h).starts_with("Show in PDF, p. 12"), "{}", focused(&h));
    // The result opens its PDF at its page, the move outlined; the dialog closes.
    h.get_by_label_contains("Show in PDF, p. 12 · 3. bishop b5").click();
    assert!(wait_for(&mut h, |h| h.state().views.len() == 1 && h.state().views[0].flash.is_some()), "opened and outlined");
    let view = &h.state().views[0];
    assert_eq!(view.current, 1, "at its page");
    assert_eq!(view.flash.map(|(page, _, _)| page), Some(1), "the move outlined there");
    let path = h.state().session.get(view.id).and_then(|d| d.path.clone()).unwrap_or_default();
    assert!(path.ends_with("a.pdf"), "{path}");
    assert!(!h.state().chess.library.is_open(), "the dialog closed");
    shot(&mut h, "n15c-opened.png");
    // Reopened: the same search and results, there to open another.
    open_library(&mut h);
    h.get_by_label_contains("Show in PDF, p. 11 · 3. bishop b5");
    // A book deleted between the search and the open: said, never opened.
    std::fs::remove_file(shelf.join("b.pdf")).unwrap();
    h.get_by_label_contains("Show in PDF, p. 11 · 3. bishop b5").click();
    h.run_steps(3);
    assert!(said(&h, "is no longer there"), "said");
    assert_eq!(h.state().views.len(), 1, "nothing else opened");
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}

/// Plan A9-N15c (Look in the Library): the Board tab's In the book list looks for the board's
/// position in every book of the Library, the dialog opening on its search.
#[test]
fn the_board_looks_for_its_position_in_the_library() {
    let data = temp_dir("board");
    let shelf = data.join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    books(&shelf);
    let mut h = harness(&data);
    h.state_mut().open_bytes("study.pdf", None, book(&["Study"])).unwrap();
    h.run_steps(3);
    open_library(&mut h);
    add_folder(&mut h, &shelf);
    h.get_by_role_and_label(Role::Button, "Build").click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET));
    // Close, clicked where it is once the build's end changed what the dialog shows.
    settle(&mut h);
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(!h.state().chess.library.is_open(), "closed: {}", tree(&h));
    h.state_mut().chess.board_mut().game = Game::from_fen(&spanish()).unwrap();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h.get_by_label("In the book").click();
    h.run_steps(3);
    h.get_by_label("Look in the Library").click();
    h.run_steps(3);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(2);
    assert!(h.state().chess.library.is_open(), "the dialog opens on its search");
    assert_eq!(h.state().chess.library.found().map(|f| f.total), Some(2), "{:#?}", names(&h));
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}

/// Plan A9-N15c, never a panic: a Library folder that can't be written (a file in its place) says
/// so when a folder is added; a folder chosen and gone since is said as not looked in, the build
/// going on.
#[test]
fn what_cannot_be_written_or_reached_is_said_never_a_panic() {
    let data = temp_dir("unreachable");
    // The Library's folder is a file: nothing can be written there.
    std::fs::write(data.join("library"), b"not a folder").unwrap();
    let shelf = data.join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    books(&shelf);
    let mut h = harness(&data);
    open_library(&mut h);
    add_folder(&mut h, &shelf);
    assert!(said(&h, "The Library's folders couldn't be changed"), "said: {:#?}", names(&h));
    drop(h);
    // A folder chosen, then gone.
    std::fs::remove_file(data.join("library")).unwrap();
    let mut h = harness(&data);
    open_library(&mut h);
    let gone = data.join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    add_folder(&mut h, &gone);
    std::fs::remove_dir_all(&gone).unwrap();
    h.get_by_role_and_label(Role::Button, "Build").click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(3);
    assert!(said(&h, "folders that can't be listed 1"), "said");
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}

/// A build held at the second book until `go` is sent, once.
fn hold_at_second_book(h: &mut Harness<'static, PdfCraftApp>) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (reached, at) = channel::<()>();
    let (go, gate) = channel::<()>();
    let gate = Mutex::new(gate);
    let armed = AtomicBool::new(true);
    h.state_mut().set_library_build_hook(Arc::new(move |book, _, _| {
        if book == 1 && armed.swap(false, Ordering::SeqCst) {
            let _ = reached.send(());
            let _ = gate.lock().unwrap().recv_timeout(NET);
        }
    }));
    (at, go)
}

/// The folders the catalog on disk names.
fn dirs_on_disk(data: &std::path::Path) -> Vec<String> {
    Library::new(data.join("library")).catalog().dirs
}

/// The review of #431 (1): a folder the picker gives while a build runs (the picker was opened
/// before it) waits for the build's end, said, and is added then; never written over by the
/// build's catalog, never refused.
#[test]
fn a_folder_chosen_while_a_build_runs_is_added_when_it_ends() {
    let data = temp_dir("race");
    let (shelf, more) = (data.join("shelf"), data.join("more"));
    std::fs::create_dir_all(&shelf).unwrap();
    std::fs::create_dir_all(&more).unwrap();
    books(&shelf);
    std::fs::write(more.join("c.pdf"), book(&["1.d4 Nf6 2.c4 e6"])).unwrap();
    let mut h = harness(&data);
    open_library(&mut h);
    add_folder(&mut h, &shelf);
    let (at, go) = hold_at_second_book(&mut h);
    h.get_by_role_and_label(Role::Button, "Build").click();
    h.run_steps(2);
    at.recv_timeout(NET).expect("the build reaches the second book");
    // The picker's answer comes while the build runs.
    h.state_mut().add_library_folder(more.clone());
    h.run_steps(3);
    assert!(said(&h, "The folder will be added when the build ends."), "said: {:#?}", names(&h));
    assert_eq!(dirs_on_disk(&data).len(), 1, "not yet");
    go.send(()).unwrap();
    assert!(h.state_mut().wait_library(NET), "the build ends");
    h.run_steps(3);
    assert!(h.state_mut().wait_library(NET), "the folder is added");
    h.run_steps(3);
    let dirs = dirs_on_disk(&data);
    assert!(dirs.len() == 2 && dirs.iter().any(|d| d.ends_with("more")) && dirs.iter().any(|d| d.ends_with("shelf")), "{dirs:?} {:#?}", names(&h));
    assert!(!said(&h, "Another build of this Library is running"), "never refused: {:#?}", names(&h));
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}

/// The review of #431 (2): while another build or change holds the Library (another window,
/// another PdfCraft, the command line), the dialog says so and Build waits; a change asked then is
/// refused with the same words and changes nothing; once it is let go of, Build is ready again.
#[test]
fn a_library_held_elsewhere_is_said_and_build_waits() {
    let data = temp_dir("held");
    let shelf = data.join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    books(&shelf);
    let mut h = harness(&data);
    open_library(&mut h);
    add_folder(&mut h, &shelf);
    settle(&mut h);
    h.get_by_label("Close").click();
    h.run_steps(2);
    let held = Library::new(data.join("library")).lock().expect("held by the test");
    open_library(&mut h);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(3);
    let busy = "Another build of this Library is running (in another window, another PdfCraft or on the command line)";
    assert!(said(&h, busy), "said: {:#?}", names(&h));
    assert!(h.get_by_role_and_label(Role::Button, "Build").accesskit_node().is_disabled(), "Build waits");
    // A change asked meanwhile: refused, the folder kept.
    let dir = dirs_on_disk(&data).into_iter().next().unwrap();
    settle(&mut h);
    h.get_by_label(&format!("Remove {dir}")).click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(2);
    assert_eq!(dirs_on_disk(&data), [dir], "nothing changed");
    assert!(h.state().alert.as_ref().is_some_and(|a| a.text.contains(busy)), "the refusal said: {:?}", h.state().alert);
    h.state_mut().alert = None;
    // Let go of: Build is ready again, the catalog looked at again on its own.
    drop(held);
    let in_dialog =
        |h: &Harness<'static, PdfCraftApp>| h.query_by(|n| n.role() == Role::Dialog).is_some_and(|d| d.query_by_label_contains(busy).is_some());
    assert!(in_dialog(&h), "the dialog says it");
    assert!(wait_for(&mut h, |h| !in_dialog(h)), "the words go: {:#?}", names(&h));
    assert!(wait_for(&mut h, |h| h.query_by_role_and_label(Role::Button, "Build").is_some_and(|b| !b.accesskit_node().is_disabled())));
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}

/// The review of #431 (3): a build that never ended (PdfCraft closed mid-book, or it stopped on an
/// error: here a panic in the build) leaves the catalog marked, and the next start says it was cut
/// short, with Continue; Continue reads what was left and the words go.
#[test]
fn a_build_cut_short_is_said_at_the_next_start() {
    let data = temp_dir("cut");
    let shelf = data.join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    books(&shelf);
    let mut h = harness(&data);
    open_library(&mut h);
    add_folder(&mut h, &shelf);
    h.state_mut().set_library_build_hook(Arc::new(|book, _, _| {
        if book == 1 {
            panic!("a test: the build ends partway, as when PdfCraft closes");
        }
    }));
    h.get_by_role_and_label(Role::Button, "Build").click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET), "the build ends");
    h.run_steps(3);
    assert!(said(&h, "The Library couldn't be built"), "said: {:#?}", names(&h));
    drop(h);
    // The next start.
    let mut h = harness(&data);
    open_library(&mut h);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(3);
    let cut = "The last build was cut short (PdfCraft closed, or it stopped on an error)";
    assert!(said(&h, cut), "said: {:#?}", names(&h));
    assert_eq!(state_of(&h, "a.pdf"), Some(State::Indexed), "what was done is kept");
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Continue").click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET), "the build ends");
    h.run_steps(3);
    assert!(said(&h, "The Library is built"), "said: {:#?}", names(&h));
    assert!(!said(&h, cut), "the words go");
    assert_eq!(state_of(&h, "b.pdf"), Some(State::Indexed));
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}

/// The review of #431 (4, 5, 6): a result opens in the tab already showing its book, whatever
/// became of the file since; a book changed since the Library read it is said so and only its page
/// shown; a folder removed takes its books' results with it, never left clickable.
#[test]
fn a_result_is_opened_as_its_book_is_now() {
    let data = temp_dir("now");
    let shelf = data.join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    books(&shelf);
    let mut h = harness(&data);
    open_library(&mut h);
    add_folder(&mut h, &shelf);
    h.get_by_role_and_label(Role::Button, "Build").click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET));
    settle(&mut h);
    h.get_by_role_and_label(Role::RadioButton, "Search").click();
    h.run_steps(2);
    type_in(&mut h, "Position (FEN)", &spanish());
    h.get_by_role_and_label(Role::Button, "Search").click();
    h.run_steps(3);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(2);
    assert_eq!(h.state().chess.library.found().map(|f| f.total), Some(2), "{:#?}", names(&h));
    h.get_by_label_contains("Show in PDF, p. 12 · 3. bishop b5").click();
    assert!(wait_for(&mut h, |h| h.state().views.len() == 1 && h.state().views[0].flash.is_some()), "opened and outlined");
    // (6) Its file gone since: the tab showing it is used, nothing said against it.
    std::fs::remove_file(shelf.join("a.pdf")).unwrap();
    h.state_mut().notice = None;
    open_library(&mut h);
    settle(&mut h);
    h.get_by_label_contains("Show in PDF, p. 12 · 3. bishop b5").click();
    h.run_steps(3);
    assert!(!said(&h, "is no longer there") && h.state().views.len() == 1, "the tab: {:#?}", names(&h));
    assert_eq!(h.state().active, Some(0));
    // (4) A book changed since the Library read it: said, its page shown, nothing outlined there.
    std::fs::write(shelf.join("b.pdf"), book(&["1.d4 d5 2.c4 e6 3.Nc3 Nf6 4.Bg5 Be7"])).unwrap();
    open_library(&mut h);
    settle(&mut h);
    h.get_by_label_contains("Show in PDF, p. 11 · 3. bishop b5").click();
    assert!(wait_for(&mut h, |h| h.state().views.len() == 2), "opened");
    h.run_steps(5);
    assert!(said(&h, "This book changed since the Library read it: build the Library again."), "said: {:#?}", names(&h));
    let view = &h.state().views[1];
    assert_eq!((view.current, view.flash.is_none()), (0, true), "its page, nothing outlined");
    // (5) The folder removed: its books' results go with it.
    open_library(&mut h);
    settle(&mut h);
    h.get_by_role_and_label(Role::RadioButton, "Books").click();
    h.run_steps(2);
    let dir = dirs_on_disk(&data).into_iter().next().unwrap();
    settle(&mut h);
    h.get_by_label(&format!("Remove {dir}")).click();
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET));
    h.run_steps(2);
    assert!(h.state_mut().wait_library(NET));
    assert_eq!(h.state().chess.library.found().map(|f| f.total), Some(0), "asked again of the Library as it is now");
    settle(&mut h);
    h.get_by_role_and_label(Role::RadioButton, "Search").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Show in PDF").is_none(), "no row left to click: {:#?}", names(&h));
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}

/// Plan A9-N15c at the reference screen (1366 × 768), light and dark, in English and in
/// Brazilian Portuguese, and at 150 % (911 × 512 points): the dialog is on screen and nothing in it
/// passes its right edge, in both views, the search's results listed.
#[test]
fn the_library_fits_1366x768_light_and_dark() {
    for (language, theme, scale) in [("en", "light", "100%"), ("en", "dark", "100%"), ("pt-br", "light", "100%"), ("pt-br", "dark", "150%")] {
        let data = temp_dir(&format!("fit-{language}-{theme}-{}", scale.trim_end_matches('%')));
        let shelf = data.join("a folder of chess books with a long name, as people name them");
        std::fs::create_dir_all(&shelf).unwrap();
        books(&shelf);
        let mut h = harness_in(&data, egui::vec2(1366.0, 768.0), language, theme, scale);
        open_library(&mut h);
        let screen = h.ctx.content_rect();
        h.state_mut().pick_override = Some(vec![shelf.to_string_lossy().into_owned()]);
        let add = if language == "en" { "Add a folder…" } else { "Acrescentar uma pasta…" };
        h.get_by_label(add).click();
        h.run_steps(3);
        assert!(h.state_mut().wait_library(NET));
        settle(&mut h);
        let build = if language == "en" { "Build" } else { "Construir" };
        h.get_by_role_and_label(Role::Button, build).click();
        h.run_steps(2);
        assert!(h.state_mut().wait_library(NET));
        settle(&mut h);
        let name = format!("{language}-{theme}-{}", scale.trim_end_matches('%'));
        inside(&h, screen, &format!("books {name}"));
        shot(&mut h, &format!("n15c-fit-books-{name}.png"));
        let search = if language == "en" { "Search" } else { "Pesquisar" };
        h.get_by_role_and_label(Role::RadioButton, search).click();
        h.run_steps(2);
        let fen = if language == "en" { "Position (FEN)" } else { "Posição (FEN)" };
        assert!(said(&h, fen), "{name}: {}", tree(&h));
        type_in(&mut h, fen, &spanish());
        h.get_by_role_and_label(Role::Button, search).click();
        h.run_steps(3);
        assert!(h.state_mut().wait_library(NET));
        settle(&mut h);
        assert_eq!(h.state().chess.library.found().map(|f| f.total), Some(2), "{name}");
        inside(&h, screen, &format!("search {name}"));
        shot(&mut h, &format!("n15c-fit-search-{name}.png"));
        drop(h);
        let _ = std::fs::remove_dir_all(data);
    }
}

/// The dialog is on `screen`, and every node in it ends before its right edge.
fn inside(h: &Harness<'static, PdfCraftApp>, screen: egui::Rect, what: &str) {
    let dialog = h.get_by(|n| n.role() == Role::Dialog);
    let rect = dialog.rect();
    assert!(screen.expand(0.5).contains_rect(rect), "{what}: the dialog {rect:?} on {screen:?}");
    let mut past = Vec::new();
    let mut seen = 0usize;
    // The nodes drawn (a node with no box, a group, has no edge to pass).
    for n in dialog.query_all_by(|n| n.bounding_box().is_some()) {
        let r = n.rect();
        seen += 1;
        if r.max.x > rect.max.x + 0.5 {
            past.push(format!("{:?} {:?} {r:?}", n.accesskit_node().role(), n.accesskit_node().label()));
        }
    }
    assert!(seen > 20, "{what}: the dialog's nodes: {seen}");
    // And what is painted on the dialog's layer (the Chess column's guard of #429, extended to the
    // Library): a text or a shape that starts in it and runs past its right edge. A vertical scroll
    // area clips nothing sideways, so the clip says nothing here; the dialog's shapes are those
    // painted after its backdrop, the translucent rect over the whole window.
    let shapes = &h.output().shapes;
    let backdrop = shapes
        .iter()
        .position(|s| matches!(&s.shape, egui::Shape::Rect(b) if b.rect.contains_rect(screen.shrink(1.0)) && b.fill.a() > 0 && b.fill.a() < 255));
    assert!(backdrop.is_some(), "{what}: the dialog's backdrop is painted");
    for clipped in shapes.iter().skip(backdrop.map_or(0, |b| b + 1)) {
        let r = clipped.shape.visual_bounding_rect();
        let level = r.center().y > rect.min.y && r.center().y < rect.max.y;
        if level && r.is_finite() && r.min.x >= rect.min.x && r.min.x < rect.max.x && r.max.x > rect.max.x + 1.0 {
            let shape = match &clipped.shape {
                egui::Shape::Text(text) => format!("the text {:?}", text.galley.text()),
                _ => "a shape".to_owned(),
            };
            past.push(format!("{shape} at {r:?}"));
        }
    }
    assert!(past.is_empty(), "{what}: past the dialog's right edge {rect:?}: {}", past.join("; "));
}
