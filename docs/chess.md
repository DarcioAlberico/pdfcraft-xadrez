# Chess books

**Status:** current to 2026-10-22 (blueprint steps S1–S18, see [`chess/blueprint.md`](chess/blueprint.md); PGN toolbar steps T1–T16; wave 10 steps U1–U5 and U7).
**Revision history:** 2026-10-18 — first version, after S18. 2026-10-19 — the chess board panel, its variations, annotations, game details and `chess_game`; book origin, and hidden OCR layers over scans. 2026-10-20 — the main line by printed weight (bold moves); `chess_pgn_edit`. 2026-10-20 — reading a selection of move text (fragment mode of the walker, PGN toolbar T2a). 2026-10-20 — where a selection goes (capture planning, PGN toolbar T3) and the `capture` operation of `chess_pgn_edit`. 2026-10-20 — capturing text from the page or the clipboard in the app (PGN toolbar T7a). 2026-10-20 — the capture preview, its notice and the layer of captured text (T7b). 2026-10-21 — the board is the study board: set-up as a rail tool, the Analysis section (T5c). 2026-10-21 — the PGN toolbar chapter (T16): the rail, capturing and where a capture goes, glyphs, comments before and after, structure, undo, the games of a document and their autosave, the chess window, Follow, the spec's scenario as a worked example, the keys and the measurements; the capture planning section folded into it. 2026-10-21 — drawings belong to the move and go into the PGN (`[%csl]`/`[%cal]`), and the engine picked is remembered (U7). 2026-10-21 — the glyph palette floats (U1); a move's menu has the glyphs and lists the variations a structure tool could act on. 2026-10-21 — Undo of a delete or a move in the games list, dragging rows, and `chess_games` (wave 10, U2). 2026-10-21 — `chess_glyph_sample` (wave 10, U3). 2026-10-21 — Chess models… chooses the models folder in the app, remembered in the settings, and on the web loads the model files for the session (U4). 2026-10-21 — the chess window comes back on its monitor, keeps one size docked or not, and docks back without covering the Inspector (wave 10, U5). 2026-10-22 — every NAG of the PGN standard and the ChessPad extension: the table, the signs read in books, the palette's positional row and list of assessments, painted signs. 2026-10-23 — Undo of games added to the list (New game, Duplicate, New game from this position, Open PGN…, Open as a new game); Load PGN adds the field's other games to the list; the engine searches on from the lines it had for a position.

PdfCraft can read a chess book in PDF. It finds the diagrams, reads each one as a position (FEN), and reads the moves in the text, including figurines and annotation signs. It can redraw a diagram, put a lichess link under it, and export the book as HTML, EPUB, Word, RTF, Markdown or PGN. Everything runs on your machine; nothing is sent anywhere.

## What you need

None of these files ship with PdfCraft (fork decisions D2 and D4). Without them, the rest still works.

| File | What it does | Without it |
|---|---|---|
| `chess-pieces.onnx` + `.json` | reads drawn and scanned diagrams | only diagrams set in a chess diagram font are read; the others are found and marked |
| `chess-glyphs.onnx` + `.json` | chess OCR: reads moves, figurines and signs from page images | pages without a usable text layer are listed as needing OCR |
| `text-detection.rten`, `text-recognition.rten` (ocrs) | reads the prose around the notation | the glyph reader reads the prose too, less well |
| a chess diagram font (Chess Merida), or a `pieces` folder of 12 PNG or SVG images | draws redrawn diagrams with real pieces | plain geometric pieces |
| a figurine font (SemFig, or one with the Unicode chess symbols such as DejaVu Sans) | shows the figurines (♘f3) in HTML and EPUB exports | the reader's own fonts |
| ISChess, the Chess Informant symbol font | shows the annotation signs (⩲ ± □ →) in HTML and EPUB exports | the reader's own fonts |

**Where the models come from.** **Chess models…** (the Chess panel's Diagrams tab, the `chess.models` command, or Preferences ▸ Chess models) chooses them:

- **Desktop:** pick the folder that holds them. It is checked at once (each model loaded, not found, or why it couldn't be loaded, such as a `.json` missing beside its `.onnx`) and remembered in the settings (`chess_models`). PdfCraft looks in that folder first, then the folder named by `PDFCRAFT_MODELS`, then a `models` folder beside the executable, then the executable's own folder; the first that holds `chess-pieces.onnx` or `chess-glyphs.onnx` is used. A chosen folder that is gone or holds no model gives way to that search, and Preferences says so (the folder stays chosen, in case a drive is just out). **Check again** reads the folder anew after you replace its files, and **Forget** goes back to the search. A remembered folder that is empty, longer than 4,096 bytes, relative or has a NUL in it is ignored.
- **Web:** there is no folder to look in, so pick the files themselves (several at once: `chess-pieces.onnx`, `chess-pieces.json`, `chess-glyphs.onnx`, `chess-glyphs.json`). Each is read into memory, up to 64 MB a file (the models are about 3 and 9 MB), and the models built from them are used until the page is closed; files with other names are listed and left out, and a model with only one of its two files says which is missing. They are not kept: the browser storage the app has (where the games go) is far too small for them, so the next visit picks them again.

Choosing again while a scan or a reading runs is safe: it finishes with the models it started with, and the next one uses the new ones.

**Diagrams** (redrawn on the page, in the editor and in every export) are drawn with the font or image folder named by `PDFCRAFT_CHESS_PIECES`, then a chess diagram font in a `fonts` folder beside the executable, then an installed Chess Merida, then a `pieces` folder of images beside the executable. The **analysis board** uses the `pieces` folder first, so a book's diagrams can be in Chess Merida while the board to play on shows your own pictures. A piece folder holds one image per piece, PNG or SVG, named `wK wQ wR wB wN wP bK bQ bR bB bN bP` (`.png` or `.svg`; an SVG wins over a PNG of the same name), each filling one square with a transparent background. An SVG keeps its own paths (a gradient is drawn in its first colour); a PNG is traced into outlines. Either way they draw sharply in the board, the PDF and every export.

**Text fonts** for HTML and EPUB come from the files named by `PDFCRAFT_CHESS_TEXT_FONT` (several separated as in `PATH`), else from the `fonts` folder beside the executable, each for what it draws:

- a figurine font that draws pieces on the letters K Q R B N, such as **SemFig**: each move's piece letters are written in it, and nothing else, so "Kasparov" in the prose stays a word;
- a font with the Unicode chess symbols (DejaVu Sans…): used for those symbols only, so the prose keeps the body font;
- **ISChess**, the Chess Informant symbol font: the annotation signs it draws (⩲ ⩱ ± ∓ +− −+ = ∞ © !? ?! □ △ ⌓ → ↑ ⇄ ⟳ ⨀ ○ ⊕, and the ChessPad signs ⇔ ⇗ ⊞ ⟫ ⟪ ✕ ⊥ ∟ ⯾, the pawn signs and the bishop signs) are written in it; ! ? !! ?? stay plain characters. Read from a book set in it, its letters give the same signs (checked against the font's own drawings, 2026-10-22: `W` is the ring for space and `[` zugzwang's ringed dot, `k` the weak point's cross, never the capture sign; `!`, `∞` over `=`, is compensation, as the user identified it from a book's key, not "good move").

HTML embeds the fonts (the file stays self-contained); an EPUB carries them as files of the book. A diagram font in the same `fonts` folder is for the pieces. A diagram drawn with Merida or cburnett pieces, and a book exported with SemFig or ISChess, carry those fonts into what you export. That is fine for personal use, but not for publishing.

**Reading books set in ISChess.** Quality Chess and Chess Informant books often set their signs, and sometimes their figurines, in ISChess, whose letters stand for something else (`O` is ±, `Q` is +−, `a` is !?, `C` a white knight, `J` a black queen). PdfCraft reads the font by its name, so a page in it reads as the signs and pieces the book prints: `2.Cf3O` in the text layer becomes 2.♘f3 ±, an annotation of the move. The pieces sit on `A`–`L` in White/Black pairs; the other letters are signs, those with a PGN code read as that annotation. No font file is needed for reading.

## In the app

Open the **Chess diagrams** panel (`chess.panel`), or run **Chess ▸ Scan** (`chess.scan`).

- **Scan this page** or **Scan all pages** find the diagrams. Pages are scanned in batches in the background, and each batch shows as soon as it is done.
  - Boxes on the page show each diagram's state: read, needs review, not read, or failed.
  - Any edit makes the scan stale. The boxes leave the page until you scan again.
  - **Side to move.** In a born-digital PDF the game usually goes on under the diagram with its first move number in bold: **23.** Nf3 means White is to move, **23...** Rxe4 means Black is. The first bold move number under a diagram (in its column, before the next diagram) sets the side to move and the move number in the FEN and the lichess link. A move number in the body font is prose and is passed over. When the position proves the other side (the side not to move would be in check), the position wins and the reading is flagged. Without either, White is assumed.
- **Pick a diagram**, in the list or on the page, to open the **editor**.
  - The editor shows the position on a board. Click a square to put the chosen piece there, and click it again to empty it.
  - You can also set the side to move and the castling rights, flip the board, or go back to the reading.
  - The editor says at once whether the position is legal.
  - Squares the model was unsure of are outlined. Each comes with its second reading, which you can take in one click.
- From the editor:
  - **Replace on the page** draws the corrected position over the diagram, as one undo step.
  - **Add lichess link** puts a link to the corrected position under the diagram.
  - **Copy FEN** and **Open in lichess** work on the position as edited.
- The **review queue** holds the diagrams that are not read, could not be read, or did not pass the reading gate. Repaired readings never pass the gate.
  - **Only diagrams to review** filters the list.
  - **Previous / Next** walk the queue in reading order.
- The **Text** tab shows the page beside it read as a chess document: headings, paragraphs and moves.
  - Moves are coloured by what replaying them found: valid, illegal, not reached, or unchecked.
  - Click a move or a paragraph to see where it is on the page, or click a diagram to edit it.
  - **Read this page with chess OCR** reads the page image instead of its text layer.
  - **Book origin** says where the book's pages came from, for the open document: **Auto** decides page by page, **Born digital** never uses chess OCR (a page whose layer looks poor is read as it is, with a note), and **Scanned** reads every page, and every export, with chess OCR.
- **Export…** writes the whole book as HTML, EPUB, Word, RTF, Markdown or PGN. Markdown's diagram images go in a folder beside the file.
- The **Chess board** is an analysis board in its own tab, beside the Inspector.
  - **Double-click a diagram** on the page, or choose **Open on the board** in the list or the editor, to load its position. The position being edited is used when the editor has that diagram.
  - Play moves by clicking a piece and then where it goes, or by dragging it. A pawn reaching the last rank asks for its piece.
  - The arrows, or the arrow keys, Home and End with the pointer over the panel, walk the line. A move made at an earlier point starts a **variation**; forward follows each move's main reply, as on lichess.
  - The move list shows variations in brackets and comments in italics. Click a move to go to it; right-click it to **make it the main line** or **delete from this move**.
  - The move shown takes annotation glyphs (`!`, `?`, `!!`, `??`, `!?`, `?!` and the evaluations) and a comment.
  - **Game details** holds the players, event, site, date, round and result, written as the PGN's tags.
  - Before the first move, choose who is to move. **Flip board** shows it from Black's side.
  - The **FEN** and **PGN** fields follow the board. Edit one and press Enter (FEN) or **Load PGN** to load it. A PGN's variations, comments, glyphs and tags are read; a move that is not legal ends only its own line, and the first one is named. With several games in the PGN field, the first goes on the board and the others are added to the document's games after the rest (at most 256 in all, as **Open PGN…** reads a file); a notice says how many, and Undo takes the games added back before the game loaded.
  - A diagram whose position is not legal is shown in a red frame and cannot be played until it is corrected in the editor.
  - **Draw on the board** with the right button, as on lichess: a click circles a square, a drag draws an arrow. Hold Shift for red, Alt for blue or Ctrl for yellow; green otherwise. Drawing the same thing again takes it off. Drawings belong to the move shown, so they come back when you return to it, and each one is an undo step. They are written into the PGN in the move's comment, in the convention ChessBase and lichess use: `{[%csl Gd5][%cal Rg1f3] the rest of the comment}` (squares, then arrows; G, R, Y and B for green, red, yellow and blue). A PGN with them shows them on the board, and the rest of its comment, other `[%…]` commands such as `[%clk]` and `[%eval]` included, is kept. An entry the board can't draw (another colour letter, a square that is not one, more than 64 on a move) stays in the comment as text. Moving a piece leaves the drawings; **Clear drawings** in the Analysis section takes them off the move shown. A position that is not legal has no game, so its drawings are kept beside it and not written anywhere.
- The board is also the **study board**, with more for studying a position (the Chess panel's tabs are **Diagrams** and **Text**). **Open on the board** in the list or the editor loads a diagram there and brings the board forward, docked or in its own window.
  - **Set up a position**, a tool of the rail beside the board, puts the set-up in the board's place, with pieces from a palette: click a square to put the chosen piece there, click it again or right-click it to empty it. **Standard start position** and **Empty board** start over; set who is to move and the castling rights (a right the pieces no longer allow is taken off). The panel says at once whether the position is legal, and **Done** plays from it (one undo step). When the game on the board has moves, the choice is **New game from this position** (a new game after it in the list; the game keeps its moves) or **Replace this game** (its moves go, told with Undo). **Cancel**, Esc, another tool or another game leaves the set-up.
  - The **Analysis** section under the board, closed until you open it, holds the rest. **Engine analysis** runs a UCI chess engine you installed, such as Stockfish. None comes with PdfCraft. It is found through the `PDFCRAFT_CHESS_ENGINE` environment variable (the program's path) or the program in an `engine` folder beside PdfCraft, or you pick one with **Choose engine…**. The program you pick and the number of lines are remembered in the settings (`chess_engine`) for the next session; analysis itself starts only when you switch it on. A remembered program that is no longer there is ignored, and the engine is looked for again.
    - It analyses the position shown, to depth 30, and starts again whenever the position changes. It stops while the board is not shown (another tab in front of it, the panel closed, Home), docked or in its own window, and goes on when it is back.
    - Back on a position it had analysed this session (one of the last 64), the lines it found are shown at once and it searches on, showing them until it goes deeper; a position already analysed to depth 30 is not analysed again. Changing the number of lines starts afresh.
    - A thin **evaluation bar** beside the board shows who is better, and the best move is drawn as a blue arrow.
    - **Lines** (1–3) shows the engine's best lines with their scores for White (`+0.40`, `#3`). Click a move of a line to play the line up to it; moves already in the game are followed, the others start a variation.
    - The engine runs as a separate program and only on the desktop. No automation tool starts it.
  - The section also shows the **material** each side is ahead by, as pieces and points, clears the drawings and says how they are drawn.

## PGN toolbar

The **PGN toolbar** turns a book's printed moves into games without playing them one by one: you select the text on the page and it goes into the game on the board, as the line, a variation or a comment, with its glyphs. A narrow rail of tools beside the board does the rest: glyphs, comments, the shape of the variations, the games of the document, and the board in its own window. (Its spec is `plan/chess/pgn-toolbar.md`, steps T1–T16; it is local to the fork.)

### The rail

The rail is a column of 28 pt buttons beside the board, in sections. Each button's name says its key, which works while the **chess key scope** is on (below). A button with a small triangle is a **group**: it shows the tool used last, and a long press, a right click, or Enter or ↓ while it has the keyboard focus opens the others; Shift with the group's letter switches the one shown.

| Tool | Key | What it does |
|---|---|---|
| **Navigate** | Esc | A click on a move goes to it. Esc also puts down any other tool. |
| **Set up a position** | | The set-up view in the board's place (see the board, above). |
| **Structure** group | Shift+S switches | Promote one level (Ctrl+↑), Make this the main line (Ctrl+Shift+↑), Demote (Ctrl+↓), Delete variation (Delete), Move variation up (Alt+↑), Move variation down (Alt+↓), Cut after this move (Shift+Delete), Threat (null move). |
| **Capture** group | X; Shift+X switches | Capture (automatic), Capture as the line, as a variation, as a sub-variation: armed, a text selection on the page goes into the game. |
| **Capture as a comment** | C | Armed, a text selection on the page becomes the move's comment. |
| **OCR area** | O | On a scanned page, capture what chess OCR reads in a rectangle (step T9). |
| **Move glyphs**, **Evaluation glyphs**, **Other glyphs** | typed | The 28 common annotation glyphs, in three groups (the palette has all of them). |
| **Glyph brush** | B | Armed, each move clicked in the notation gets the brush's glyph, or loses it. |
| **Move comment** | Enter | Opens the comment editor, with the comment before the move. |
| **Glyph palette** | G | A floating window of figurines, glyphs and `½ – × † ‡`: a glyph goes on the move, the rest into a comment or field. |
| **Follow** | F | The page follows the move shown (below). |
| **Show in PDF** | | Scrolls the page to where the move shown was printed. |
| **Undo**, **Redo** | Ctrl+Z, Ctrl+Shift+Z | The game's own history (below); the names say what they would undo. |
| **Chess in its own window** | | The board and the Chess panel in a window of their own (desktop only). |

On macOS, Ctrl is ⌘ and Alt is ⌥. Every tool is also a command (`chess.tool.*`, `chess.variation.*`, `chess.moves.*`, `chess.nag.1`–`6`, `chess.comment.edit`, `chess.palette.toggle`, `chess.source.*`, `chess.window.detach`), so it can be found with Ctrl+K and run by automation. A move's right-click menu in the notation has the structure tools, each saying what it would act on (or why it can't), and, where one could act on several variations on the way to the move, those variations listed under it, nearest first ("Promote one level: 2…Nc6 · 1…c5"); then the move-quality glyphs and the evaluations, ticked while the move has them, and **Remove all glyphs**, which act on that move without going to it; then **Diagram here**, **Copy line with figurines**, **Copy line (letters)**, **Delete from this move** and **New game from this position**.

**The chess key scope.** A click in the chess panels, or a change to the game, gives the keyboard and Undo to the game; a click on the page or a change to the document gives them back to the document. While the game has them, the keys above go to the board before anything else sees them: Delete deletes a variation, never a page or an annotation, and the arrows walk the game, never the page. They are left alone while a text field has the keyboard (a letter or Delete typed in a comment is the comment's), while a menu or popup is open and under a dialog.

### Capturing text

- **Arm a tool.** **Capture** (X) or **Capture as a comment** (C). The page tool switches to Select and a chip in the page's corner says what is armed ("Capturing · automatic ▾ · Esc to stop"); its menu switches the mode, and Esc stops.
  - **Automatic** reads the text as the book sets it: a bold first move continues the main line, anything else becomes a variation where its move numbers fit; prose becomes comments, brackets variations, and `!`, `±`… glyphs.
  - **Forced** modes (Shift+X steps through them) are for books that use no bold, or text that fits in more than one place: **as the line** continues the line of the move shown (a different move already there becomes a variation), **as a variation** of the main line, **as a sub-variation** of the line of the move shown or one outside it.
  - **Comment** puts the text on the move shown, its moves in English SAN. The chip's switches put it **before** the move instead of after, and **replace** the comment instead of adding to it.
- **Select the text and let go.** The selection grows to whole moves ("20.Nx" takes "20.Nxf6+"). It is read from the page's glyphs, not from copied text: a figurine font's pieces are pieces, and a bold glyph is bold. The language is the document's (as the **Text** tab read the page; English until it has), never guessed from the selection: `Rc7` is a rook in English and a king in Portuguese.
- **Without arming anything:** **Send to PGN ▸** in the bar beside a selection, or in the page's right-click menu, captures it in the mode you pick. **Paste:** Ctrl+V with the chess scope on (and no field typing) captures the clipboard's text; **Paste as ▸** in the board panel picks the mode. Ctrl+C on the page captures too when you turn on **Also capture what is copied** in the chip's menu (off by default).

**Where a capture goes.** The selection's first move decides, by its printed number:

1. The number says the half-move the move is played at, counted from the diagram's move number (a diagram already carries the number printed under it).
2. The candidate lines are the line of the move shown, the line that one branched from, and so on out to the main line. As the line or a sub-variation every one is tried, the innermost first; as a variation, only the main line. Automatic capture takes a bold first move as the main line's continuation and anything else as a sub-variation.
3. On each line the place is the move after which that half-move comes. A first move printed without a number goes after the move shown.
4. As the line, a new move where a move already has replies becomes the first reply, and the old first reply a variation. As a variation, it goes after the replies there. A reply that is already there is followed, not added twice.
5. Each place is tried on a copy of the game: the selection is read from that position and grafted there. The place that plays the most moves wins, then the one with the fewest moves that did not play, then the inner line. Two places that play as many are **ambiguous**.
6. In a game with no moves whose start says move 1 (the number under the diagram was not read), the start renumbered to the first move's number is offered too.

**When it asks first.** A capture goes in at once, as one undo step, only when the whole text was read, it fits one place, that place is on the line of the move shown, no line's end was extended by a variation, no move already there was demoted and nothing was renumbered. Otherwise a **preview** opens beside the selection and says why:

| Reason | Means |
|---|---|
| ambiguous | another place plays as many moves |
| incomplete | part of the text was not read: a move that did not play, a bracket that did not balance, text after a result, a glyph or comment with no place, or a comment too long |
| outer line | the place is on a line outside the one of the move shown |
| tip extension | taken as a variation, it found no move to be an alternative to and extended a line's end |
| renumber | the start is renumbered to the text's first move |
| implied close | a closing bracket the text lacks was assumed |
| demotes | a move already there became a variation |
| no fit | no place plays a move |
| too long | over 64 KiB of text; nothing was read |

The preview shows where the text goes ("Variation of 18…Qxe1"), **Fit at** to pick another place it fits, the **Language** it is read in (changing it reads the text again, and becomes the document's language for captures), and the moves as chips: ✓ played, ✗ where it stopped and why, grey not read, bold for the main line. **Apply** (Enter), **Apply and keep the rest as a comment** (what was not read is never thrown away) or **Cancel** (Esc). It follows the page as it scrolls and closes when the document is edited.

**Undo.** A capture is one step of the game's history: Ctrl+Z takes it back even with the pointer still on the page, and the document is never touched. After a capture a notice says what it did ("Variation of 18…Qxe1 · moves: 4") with **Undo**, which stays until the game changes again (it has no timeout), and the new moves are tinted in the move list for a moment (a plain tint with reduced motion).

**Captured text on the page.** Every captured move remembers where it was printed. **Underline captured text on the page**, in the board panel, underlines them, so you can see what is left to transcribe; the places come back with the games in the next session.

A document that doesn't allow copying doesn't allow capturing either. At most 64 KiB of text is captured at once; a selection over 4 KiB is planned off the interface thread.

### Correcting a move that did not play

In the preview, a ✗ chip (a move that could not be played where it was read, or where the graft stopped) is a button: it opens a panel under the chips (step T10).

- **Legal moves near it**: what the legality repair makes of the move in the position it was to be played in: the one reading it settles on, the moves an ambiguous reading fits (`Rd1`: `Rad1`, `Rhd1`), or the three closest legal moves. They are written in the language the text is read in and are **never applied until you click one** (guessing a move goes wrong). A move after one that stopped was never reached, so it has none: correct the first one first.
- **Correction**: a field holding the move as printed, with the **Glyph palette** (the rail's characters plus the ellipsis) inserting at its cursor; Enter or **Read again** takes it.
- **The OCR's readings** (text from the OCR area of a scan): the move's characters, a doubtful one underlined (its top two readings within a margin of 0.5); a click lists the reader's top three readings, and picking one puts it in place of the character.

Each correction rewrites the move where it was printed (the page's glyphs keep their place and weight) and reads the whole text again, so the preview shows what the corrected text reads as; nothing goes into the game until **Apply**. A correction that fits nowhere is refused and the text stays as it was.

**Collecting corrected characters for retraining** (step T11, opt-in, desktop only). With **Collect corrected characters for retraining** on in the capture chip's menu, and a folder chosen there, each character of an OCR area that you correct (a reading picked, or a corrected move whose characters keep their boxes) is saved as a 32 × 32 gray picture in `<folder>/<character>/<confidence>_<document>_p<page>_<n>.png`, the layout of ChessVisionOFF's review queue (`revisao_ocr`): the folder is the character's class name there (`digit_3`, `upper_N`, `sym_9812` for ♔), and the name starts with the model's confidence in thousandths, so the most doubtful come first. Nothing is overwritten, at most 2,000 are saved a session, an encrypted document is asked about first (once per session; declined, nothing of it is saved), and nothing is trained in the app or leaves the computer.

### Glyphs

PdfCraft knows every glyph of the PGN standard (codes 1–139) and of the extension the public table of non-standard NAGs attributes to the ChessPad editor (140–146 and 238–255): 164 codes, most of them in White/Black pairs, as one table (`pdfcraft_chess::nag::NAGS`). The 28 that books use most are in three groups on the rail, each named for its meaning ("Good move", "White is slightly better"), never read out as a sign:

- **Move glyphs:** `!` `?` `!!` `??` `!?` `?!`, only move `□`, novelty `N`. A move has at most one; another replaces it.
- **Evaluation glyphs:** `=` `∞` `⩲` `⩱` `±` `∓` `+−` `−+` and compensation. At most one; another replaces it.
- **Other glyphs:** zugzwang, development, initiative, attack, counterplay, time trouble, with the idea, aimed against, better is, worse is, editorial comment. These add up.

A glyph goes on the move shown with the code for the side it speaks of (zugzwang: the side to move after it); clicking a glyph the move has takes it off. Each change is one undo step.

- **Typed glyphs:** with the chess scope on and no field typing, type the sign: `!` `?` `!!` `??` `!?` `?!` `=` `+=` `=+` `+/-` `-/+` `+-` `-+` `~`, or `0` to take the move's glyphs off. They come as text, so any keyboard layout works; a two-character sign waits up to 600 ms for its second character.
- **Glyph brush** (B): each move clicked in the notation gets the brush's glyph, or loses it, one undo step a click; the move shown does not change. A glyph chosen in a group while it is armed becomes the brush's. Esc puts it down.
- **Glyph palette** (G, or the rail's button): a small window floating beside the rail, in the main window or the chess window, that can be dragged anywhere within it and opens again where it was left (for the session). It has the figurines and `½ – × † ‡`, then the glyphs in the rail's three groups (the move row with ChessPad's equivalent move `==` too), a row of **Positional glyphs**, and **More assessments**, all ticked while the move shown has them. A glyph clicked goes on the move shown, or comes off it, as on the rail; the other characters go at the end of the move's comment. While a field had the keyboard (a comment, FEN, PGN), any character, a glyph's sign too, is typed at its cursor instead. A click in the palette keeps the keys on the game, as a click in the panel does. Esc, its close button or G closes it.
  - **Positional glyphs**: space `○`, and ChessPad's play along the file `⇔`, along the diagonal `⇗`, in the center `⊞`, on the kingside `⟫` and the queenside `⟪`, weak point `✕`, endgame `⊥`, connected, isolated, doubled and passed pawns `⯺ ⯻ ⯼ ⯽`, the pair of bishops, bishops of opposite colors and of the same color, with `∟` and without `⯾`. ChessPad's space advantage (`$238`) is drawn `○` like the standard's (`$26`), which is the one read in text.
  - **More assessments** (folded at first): the assessments the PGN standard numbers but books write out in words, in a list that scrolls in the room left in the window (the palette never runs past the window's bottom, and moves up when the list opens too low to have room), by name, in sections (move quality, evaluation, initiative and time, space and control, king safety, pawns, pieces, phases of the game), or narrowed by the **Filter assessments** field (by name, in English or the interface language, or by `$n`). One clicked goes on the move with the code of the side that made it, even while a field had the keyboard (as text it would only be `$n`), one undo step. PGN writes them as `$n`, and nothing in a text is read as one.
- **A move's menu** (right-click in the notation) has the move-quality glyphs, the evaluations and **Remove all glyphs** for that move, one undo step each. Only these: the other glyphs are on the rail and in the palette, and a menu with all of them would be a wall.
- **In the notation**, a glyph reads after its move: as its sign where the interface fonts draw it (`⩲` `⩱` as `+=` `=+` and `⨁` as `⊕` where they don't), painted where no bundled font has the sign (`⌓`, `⇗`, `⯺ ⯻ ⯼ ⯽`, `∟ ⯾`, and the bishop signs, which have no Unicode character), and an assessment without a sign by its name, small and muted, with its `$n` as the tooltip: a reader learns nothing from `$24`, and these are rare enough for the words not to crowd the moves. PGN and copied text keep the real character, or `$n`.
- **Diagram here**, in a move's menu, shows a small board after the move in the notation and is kept in the PGN as `$220` (a de-facto code, from the public table of non-standard NAGs; other programs may not read it).

**Reading the signs in books.** The new signs are read in a book's text and after a move as the old ones are: `○` (space, `$26`/`$27` by the side that moved), `⇔ ⇗ ⊞ ⟫ ⟪ ✕ ⊥ ∟ ⯺ ⯻ ⯼ ⯽ ⯾` and `==`. Signs close to others are kept apart on purpose: `=` is still equality (`$10`) and `==` one sign (`$144`), never two `=`, while a promotion (`e8=Q`) is the move's own text; `⇔` (a file) is not counterplay (`⇆`, `⇄`); the weak point's `✕` (U+2715) is not the capture sign `×`; and `⯹`, the Unicode sign for compensation, reads as `©`. The bishop signs have no Unicode character: in text they are the stand-ins the ISChess font's letters are read as (`⧉` the pair, `□■` opposite colors, `■■` the same color), read only glued after a move, as `■` turns up in the text of OCR'd diagrams (62 times in one of the 13 books scanned, never as a sign). Re-reading the ten stored book stretches before and after (2026-10-22, `pdfcraft-cli` `chess_document`, then `examples/walk_bench.rs` and `examples/fragment_bench.rs`) gave the same 2,841 valid moves, every move with the same status, and the same 324 of 333 variations reproduced; the whole text layers of 13 books (about 11.2 million characters) hold none of the new one-character signs, and `==` only in OCR'd rule lines (`====`), which were noise before as well.

### Comments before and after

Under the notation the move shown has its comment, the one after it. **Move comment** (Enter) opens the editor with the **comment before** the move too. Typing in a comment is one undo step per visit to the field, not one per key.

PGN only has a place for a comment before a move that starts a variation: `(18... {Better is} Qd8 …)`. Before a move that continues its line, the PGN writes it into the comment after the previous move (or before the game's first move), and the editor says so. PdfCraft keeps it on its move in the game and in the autosave, so it comes back where it was; only a PGN file loses where it was.

### Structure

Each operation is one undo step, and can be undone exactly: promote one level then demote, or move up then move down, gives back the same tree, comments before included.

- **Promote one level** (Ctrl+↑): the variation continues the line it branches from, and that line's move becomes its first variation; the other variations keep their order. From a move inside a variation, the nearest variation that can be promoted is the one; when several could be, the group's popup lists them by their first move.
- **Make this the main line** (Ctrl+Shift+↑): promote all the way.
- **Demote** (Ctrl+↓): a move that continues its line becomes the first variation, and the variation that was first continues the line instead; the inverse of promoting.
- **Move variation up / down** (Alt+↑ / Alt+↓): among the variations at the same fork. The line itself never moves.
- **Delete variation** (Delete): the variation and its moves; the move shown goes back to the fork.
- **Cut after this move** (Shift+Delete): every move after the move shown, variations included.

Whatever takes moves out of the game (these tools, a move's menu, the button under the notation, a position put in the game's place) is told in a strip at the top of the panel, "5 moves deleted (2 variations)", with **Undo**; the notice stays until the game changes again or it is dismissed (✕). Backspace only goes back a move: it deletes nothing. The game as it was also goes into the document's trash, kept with the autosave (the newest 30, each up to 256 KB), so the moves can come back after Undo is gone, in a later session too: **Deleted moves (n)…** under the list of games lists them, newest first, and **Open as a new game** puts one after the game shown.
- **Threat (null move)**: the side to move passes, and the moves after show what the other side threatens, as books do (`12... --`). Written `--` in PGN and read back (also as `Z0` or `0000`); refused when the side to move is in check.
- **Copy line with figurines / (letters)** in a move's menu copies the line up to it as book text (`18.♘d5! ♕xe1 19.♘xe7+`), from the start of its variation or of the game.

### Undo

The game has its own history, apart from the document's: every change (a move, a capture, a glyph, a comment, a drawing, a structure change, the game's details, a position or a PGN loaded) is one step, and walking the moves is not a change. Undo and Redo follow the scope of the last action: after a click in the chess panels or a change to the game they undo the game; after a click on the page or a document edit, the document. The rail's Undo and Redo always undo the game. The history keeps at most 200 steps and 64 MiB per game, the oldest going first. It is not kept across sessions.

### Games of a document

Every open document has its own games, and the board shows the active one; switching documents switches games. **Games in this document**, in the board panels, lists them by their players, else their event, the diagram they came from, or their number.

- A diagram opened on the board is a **new game**: it never replaces the game there (the same untouched diagram opened twice shows its game again). **New game** adds a blank one; **New game from this position** starts one at the move shown, keeping its diagram.
- Each game's menu, its buttons on the row under the pointer, or the keys of a focused row: **Rename…** (F2; the White, Black and Event tags, so the name is in the PGN), **Duplicate**, **Move up / down** (Alt+↑ / Alt+↓), **Delete…** (Delete or Backspace; it asks first; deleting the last game leaves a blank one), and Enter shows it. A row can also be dragged to another place.
- **Undo** takes back a change to the list too (Ctrl+Z, the Edit menu or the rail's Undo, while Undo is on the chess side): whichever is newer, the list's last change or the last change to the game shown, goes first, and Undo names it (“Undo Delete game”). A deleted game comes back at its place with its own undo history. Games added are steps too: **New game**, **Duplicate**, **New game from this position**, **Open PGN…** (all the games of the file, one step) and **Open as a new game** from Deleted moves; Undo takes them out and shows the game shown before, and Redo puts them back. A game you changed after it was added is also kept in **Deleted moves** when Undo takes it out (the notice says so), so its changes are never lost; Redo takes it back out of there. Undoing **Open as a new game** puts the game back in Deleted moves. The list's undo history lasts for the session.
- **Open PGN…** adds every game of a file (at most 256). **Save PGN…** writes the game shown, **Save all games as PGN…** (Export… in the list) the whole list in its order (a file you pick; a download on the web). Saving is refused for a document that does not allow copying.

**Autosave.** Two seconds after the last change, when a document closes and on exit, each document's games are written to a file of their own in the `chess` folder of the recovery folder: `%LOCALAPPDATA%\PdfCraft\Recovery\chess` on Windows, `~/Library/Application Support/PdfCraft/Recovery/chess` on macOS, `~/.local/share/pdfcraft/recovery/chess` elsewhere (in portable mode, the portable data folder). The file is two lines of JSON: the document's keys (its `/ID`, path, and name and size), then the games: each one's PGN and what PGN cannot hold (the move shown, the board's side, an illegal position, the diagram it came from, where each move was printed, and the comments before a move PGN would merge). Opening the document again finds them by any of its keys, and saving the document takes the new keys at once.

- Files are written atomically, and only by the running instance that holds the recovery folder's lock; a file another running instance wrote is left alone, and you are told.
- A file that can't be read, is too big or has another version is renamed `….games.bad` (never deleted, never written over) and you are told; new games are saved from then on.
- Caps: 256 games and 32 MiB per document. Above them the document's games are not autosaved (you are told) until they are back under; nothing is ever evicted.
- **Protected documents** (encrypted, or not allowing copying): you are asked once per document and session before their games are written to disk.
- On the web the games are kept in the browser's storage (4 MiB for all documents), written when the app saves its state; a value that can't be read is kept aside.

### The chess window

**Chess in its own window** (the rail's last tool, or the Board tab's menu) moves the board, with the Chess panel's Diagrams and Text tabs, into a window of its own, so the PDF can fill the main window, on another monitor if you like. From 760 pt wide the board sits beside the tabs; narrower, a **Board | Diagrams | Text** strip switches them. In the window the board can grow past the dock's 480 pt, up to 1,200 pt. Its keys are its own: the chess keys go to its rail and the app's shortcuts still work there. Closing it, or **Dock**, puts the board back where it was: in its tab group, at its place in it, and in front only if it was in front when it left. **Bring it back** in the Inspector does the same but leaves the Inspector in front (its Chess panel shows the Diagrams and Text again); if the board's group is gone, it goes at the end of the Inspector's group. Its size, place and tab are remembered in the settings (`docking_detached`). The size is what the window's contents get, the same whether the window is a separate one or drawn inside the main window, so going from one to the other keeps it. The place is kept in pixels of the desktop with the monitor it was on, so on monitors with different scaling the window comes back on its monitor (moved with it if that monitor moved); a window whose monitor is gone, or that would be mostly off every monitor, opens beside the main window instead, always wholly on a monitor. A value that isn't usable is ignored. `pdfcraft-cli ui --control FILE state` shows the window's geometry (`chess_window`: its place, the monitors and how it last showed), for checking it across monitors. If the window or its drawing surface can't be made, the board docks back once, with a notice, and no window is tried again in that session. Only on the desktop: on the web nothing offers it (a window remembered in the settings is drawn inside the page, with a Dock button).

### Follow the source

A captured move, and a move of a game opened from the **Text** tab (a double-click on a move there, or **Open this game on the board**), remembers where it was printed. **Show in PDF** scrolls the page to the move shown and outlines it, unless it is on screen already. **Follow** (F, a switch for the session) does that whenever the move shown changes, so walking the game walks the book, with the board docked or in its own window. A move with no place on the page (played on the board, or typed) shows nothing.

### A worked example

The spec's scenario is a page of Grooten's *Chess Strategy for Club Players* (the game up to 17…Bc4 and the diagram under it, `2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 18`):

1. Double-click the diagram: game 1 of the document's collection, at move 18 (the number printed under the diagram), nothing renumbered.
2. Press X and select from `18.Nd5!` to `20.Raxe1`, prose included. In one capture, without the preview: the bold main line `18.Nd5! Qxe1 19.Nxe7+ Kh7 20.Raxe1`, the variation `18…Qd8 19.Bxf6! Bxf6 20.fxg6` with its sub-variation `(20.Nxf6+ exf6 21.Bxc4 Rxc4)` and the return `20…fxg6 21.Qg3 Kh7 22.e5!`, the prose as comments where it was, and `!` on 18.Nd5, 19.Bxf6 and 22.e5.
3. The next paragraph: `(20.Rfxe1)` inside the prose becomes a variation of 20.Raxe1, and the bold `20…Rc7 21.Bxc4 Rxc4` the main line. Captured alone first, the prose's `20…Rc7 21.e5!` extends the main line's end (with the preview); the bold line captured after it follows `20…Rc7`, puts `21.Bxc4` as the first reply and demotes `21.e5` to a variation (with the preview, because a move already there was demoted).
4. The forced modes build the same tree step by step from the empty game: as the line `18.Nd5! Qxe1`; as a variation `18…Qd8 19.Bxf6! Bxf6 20.fxg6`; as a sub-variation `20.Nxf6+`; as the line `20…exf6 21.Bxc4 Rxc4` (it continues the sub-variation); as the line `20…fxg6 21.Qg3`, which can't be played in the sub-variation (after 20.Nxf6+ Black is in check and no White piece is on g6), so it extends the outer variation, with the preview.
5. Click 20.Nxf6+, press C and select the prose after it: its comment. With **before** on the chip, the words before it become its comment-before (it starts a variation, so the PGN keeps it there).
6. Right-click 18…Qd8 ▸ **Promote one level**: it becomes the main line. ▸ **Demote**: back, identical, comment-before included. Ctrl+Z twice gives the same game, and the PDF was never touched.
7. The PGN reads back to itself. Close and reopen the app: the collection comes back, and the underline of captured text with it.
8. With **Follow** on, going to 20…exf6 scrolls the page to it and outlines it.
9. **Chess in its own window**: the board and the notation in a window of their own, the PDF in the whole main window. Closing it docks them back.

Steps 2–4 are tests of the engine (`engine::chess::capture`), of `chess_pgn_edit` and of the app; step 2 is one selection and no Enter, against about forty clicks on the board.

### Keyboard

These work while the chess scope is on (a click in the chess panels), and not while a text field has the keyboard. Ctrl is ⌘ on macOS.

| Key | Does |
|---|---|
| ← → | previous, next move |
| ↑ ↓ | the variation before, after the move shown at its fork |
| Home, End | the start, the end of the line |
| Esc | put the armed tool down (Navigate); cancel a preview |
| Enter | open the comment editor; apply a preview |
| X, Shift+X | arm Capture; switch to the next capture mode |
| C | arm Capture as a comment |
| O | the OCR area on or off |
| B | arm the glyph brush |
| G | the glyph palette |
| F | Follow on or off |
| Shift+S | switch the Structure group's tool |
| Ctrl+↑, Ctrl+Shift+↑, Ctrl+↓ | promote one level, make the main line, demote |
| Alt+↑, Alt+↓ | move the variation up, down |
| Delete | delete the variation |
| Backspace | back a move (like ←) |
| Shift+Delete | cut after the move shown |
| `!` `?` `=` `+` `-` `/` `~` | type a glyph; `0` takes them off |
| Ctrl+Z, Ctrl+Shift+Z | undo, redo the game |
| Ctrl+V | capture the clipboard's text |

In the list of games, a focused row takes F2 (rename), Delete or Backspace (delete), Alt+↑ / Alt+↓ (move) and Enter (show), and nothing else.

### Measurements

Measured on 2026-10-21 (Windows 11, AMD desktop, release builds; the stored readings of the ten stretches of the user's books used throughout this page; spec §8):

| What | How | Result | Target |
|---|---|---|---|
| A selection gives the book's own variation | `examples/fragment_bench.rs`: every variation the walker built is read again as a selection from the position before it and grafted | 275 of 292 (94.2%) on the readings without weights; 294 of 301 (97.7%) on the same stretches read again with their weights, where bold main lines count (Dvoretsky 60/60 and 89/90, Fischer 90/92) | ≥ 95% |
| Whole books don't read worse | `examples/walk_bench.rs` | 2,327 valid moves on the readings without weights (2,304 before the toolbar's rules, none worse); 2,534 with the weights | ≥ the base |
| Gestures for the scenario's paragraph | the worked example, step 2 (`the_paragraph_selected_with_capture_armed_goes_in_at_once`) | one selection, no Enter | 1 selection + at most 1 Enter |
| Time to plan a capture | `plan_cost_on_the_scenario` (`cargo test --release -p pdfcraft-engine --lib -- --ignored plan_cost`) | 0.84 ms from the empty game, 0.86 and 0.69 ms from inside the sub-variation (one candidate each; each candidate is one reading and one graft, so the most a plan can have, 35, stays near 30 ms) | < 50 ms |
| Never panics | `cargo xtask fuzz` with a page of move text to capture (T16), and fuzz-like tests of `chess_pgn_edit`, the games files and the chess window's saved settings | 8,913 runs in 300 s (two jobs, the four synthetic seeds; 9,184 in an earlier run before the OCR area's reading was added): no crash, no hang | no crash |

The differences left in `fragment_bench` are mostly the whole book's misreadings: a numbered move after a prose digression goes back to the variation's own line in a selection, while the whole book keeps it in the digression.

## Headless: tools and CLI

Every feature is also a tool (`pdfcraft-cli tools`; the MCP server is opt-in):

| Tool | Does |
|---|---|
| `chess_status` | which models and piece set are available, and why not |
| `chess_detect`, `chess_read` | find the diagrams; read them (FEN, legality, side to move, weakest square, uncertain squares, repairs, lichess URL) |
| `chess_document` | the book as headings, paragraphs of moves and diagrams, with the games replayed into variation trees; JSON and PGN |
| `chess_replace_diagram`, `content_draw` | redraw a diagram (one undo step); the generic checked-operators edit under it |
| `chess_add_lichess_links` | a label and link under every legal, accepted diagram |
| `chess_game` | the board without a document: a game from a FEN or a PGN, moves in SAN or UCI, the position at a ply with its legal moves, and the game as PGN |
| `chess_pgn_edit` | a game's moves without the board: play, a null move, comment (after or before a move), glyphs, a diagram mark, promote one level, make main line, demote, move a variation up or down, delete a variation, cut, and graft move text as a variation, as the line or as a comment, at a main-line half-move (`main:35`) or a reply path (`path:0.1`), with a report per operation; `capture` plans where a selection of move text goes (see [Capturing text](#capturing-text)) and applies it, reporting every candidate place; inline only (the game in the arguments and the reply, no path) |
| `chess_games` | a collection of games as PGN (inline, or a `.pgn` file): list, show, rename (White, Black, Event), duplicate, delete, move to any place, a new game from a move's position, and export, as the app's list does them; the result written to a file when asked |
| `chess_glyph_sample` | save one corrected character's box as a 32 × 32 sample for retraining the glyph model, in ChessVisionOFF's review-queue layout, in a folder inside the root (an encrypted document only with `allow_encrypted`) |
| `chess_read_area` | read a rectangle of a page with chess OCR (the user's glyph model): text, moves, each character's top readings; with a game, the capture plan and the legal moves near each move that did not play (offered, never applied) |
| `chess_export_html`, `_epub`, `_docx`, `_rtf`, `_markdown`, `_pgn` | the exports |

`pdfcraft-cli chess scan FILE` prints the diagrams found. All paths are confined to `--root`.

The pages to OCR are chosen with `ocr`: `auto` (the default) reads only pages whose text layer is poor or missing, `off` reads none (a born-digital book), and `all` reads every page (a scanned book).

A scan that FineReader, Acrobat or Tesseract already recognised carries their text drawn invisibly over the page image. That layer reads cleanly but is a general OCR's guess at the figurines and annotation signs, so a page whose text is mostly invisible and whose images cover at least half of it is judged poor ("another program's OCR"), and `auto` reads it with chess OCR.

## How well it reads

All figures below are on the user's own models. Each has its command and the machine it ran on in the blueprint's log (§8).

- **Finding diagrams on a page image:** recall 0.9826 (113 of 115), precision 1.0, at 220 dpi on the prototype's field set.
- **Reading diagrams with the piece model:** 555 of 569 exact (0.9754) on the prototype's test split. The target is 0.99.
- **Chess OCR, fused with ocrs:** CER 0.88% and 98.0% of moves exact on the Caïssa benchmark (744 lines). These numbers are in-sample: the Dvoretsky lines are in the model's training data.
  - A scanned line of ten Helvetica moves is read 10 of 10.
  - Outline figurines in fonts the model never saw are still weak, at about 74% of moves for DejaVu Sans.
- Diagrams set in a chess diagram font are read exactly, from the text. A subset font whose codes were renumbered is read by its text map, and glyphs under 3 pt (stray spaces between the squares) are not squares.

**Speed and a real book** (2026-10-18, `pdfcraft-cli run` in release, the user's models, Windows 11 on an AMD desktop; *Dvoretsky's Endgame Manual* (2025), pages 51–60, one process per run, so nothing cached):

| Step | Time | Result |
|---|---|---|
| Find the diagrams | 0.31 s/page | 19 diagrams |
| Find and read them | 0.53 s/page | 19 of 19 accepted, all from the diagram font (before, by the piece model: the subset font's renumbered codes and its 0.7 pt spaces hid the boards; without the models no diagram was read and no move checked) |
| The recognised document (text layer) | 0.86 s/page | 459 moves, 18 games, 381 valid (7 before column order, 61 before subset figurines, 324 before games across diagrams, 369 before bracketed replies) |
| The recognised document with chess OCR | 1.8 s/page | |

That PDF's pages are set in two columns, and its figurines come from a subset font that renumbers its glyphs. Lines are cut at the gutter and read column by column, and a diagram's printed coordinates are left out (valid moves 7 → 61). A figurine is read from the text layer when its code names no piece, so the page is no longer judged poor and handed to OCR, whose words (`Taf7asch` for Tarrasch) replaced a clean layer (61 → 324). Games are found across diagrams: a text after two diagrams printed side by side starts from the one its first move is legal in, a diagram of a game's own position does not end it, and after an alternative written as prose a move is tried on the main line (324 → 369). Brackets after a move that hold the replies to it (`1.Kb1 (1...b3 2.Kb2; 1...a3 …)`) are played after it (369 → 381; with page 49, which holds page 50's diagram, 402 of 478). A diagram whose text gives the solution for each side to move (`Black, if on move, holds by 1...f5`, then `1.f5+`) starts a game for each: on pages 100–119, valid moves went from 790 to 812 of 978. Most of the rest are "not reached" after a move that cannot be played (often a position the prose describes but the book does not print), or belong to a diagram on a page that was not read.

### A game collection: Fischer's *My 60 Memorable Games* (Portuguese edition)

That book prints move numbers with no full stop (`19 ♖xd5`, Black's `15 … ♖xb6`), sets its game text larger than the introductions, and opens each game with a short introduction in a smaller italic. Numbers are now read without the full stop when a move follows; the body size is the book's, not the page's, so a page that opens a game does not make its moves and notes headings; a block with moves is never a heading; and a `1.` that the game from the last diagram cannot play starts the next game from the initial position. On pages 30–59, valid moves went from 142 to 468 of 1179 (and 63 false headings to 8 true ones), and the eight game titles and subtitles are the headings.

### A scanned book with an old OCR layer: Aagaard, *A Matter of Endgame Technique*

That PDF is a scan under an invisible text layer in Courier New, left by an earlier OCR program that read the figurines as stray symbols (`50.&xe5 ®f7 51.^d5`). Every move failed. Such words now make the layer poor (at least three on a page), so chess OCR reads the page again; the old layer's block sizes no longer decide headings on a page read with OCR; and a check written `†` (which OCR returns as a cross, `✝`) or mate `‡` after a move is read. On pages 40–49, with the models, valid moves went from 0 of 102 to 80 of 374. Moves OCR cuts with spaces (`♖ e 1`, `f1 =♕`) are put back together (80 → 87 of 376); the rest are mostly OCR misreadings.

### Two-column scans: Gallagher, *Winning With the King's Gambit*, and Flores Rios, *Chess Structures*

Both are scans with no text layer, read with chess OCR, set in two columns with game scores as tables (`2 f4 ef`). Two things kept the columns apart from being read in order. The OCR's glyph boxes are their ink (an `a` half an `h`), so against those heights every word gap looked like a gutter; the page text now gives each recognised glyph its line's height, as a text layer does. And a column whose score table is cut into narrow pieces fell below the average width a column needs; five wide lines now make it a column too. On pages 40–49 of Gallagher, valid moves went from 1 to 31 of 865; on pages 60–69 of Flores Rios, from 55 to 87 of 786 (Aagaard 87 → 85, the text-layer books unchanged). Both books draw their dark squares hatched, which the piece model misreads: their diagrams start no game until the model is trained on such boards.

### A puzzle book: Karpov, *Chess Combinations – World Champions 1*

The book prints six numbered diagrams a page (`№185. Teichmann – Em.Lasker` above each, which the text layer extracts as `N!!185.`) and the solutions 230 pages on (`№185. … 27 ... ♕e2+?`), on pages whose old OCR layer the chess OCR rereads (it reads `№` as `N9`). A caption above or below a diagram, in its column, that starts with a puzzle number gives the diagram that number (a caption with `№` wins over a bare `185.`, a caption goes to its nearest diagram, and a bare number needs a name after it). A paragraph of solutions that names a puzzle seen earlier closes the open game and plays its moves from that diagram; the game is tagged `PdfPuzzle`. On puzzle pages 40–41 and solution pages 275–276, valid moves went from 0 to 139 of 483, all twelve numbers read right; the other books unchanged.

Polgar's *Chess 5334 Problems, Combinations and Games* prints the number alone above each diagram (`1675`, the diagrams in the `skak` font, read exactly) and its solutions as the number alone before the key move (`1675 1.Rd3+`), 700 pages on. A caption that is the number alone counts (a section number such as `2.2 Combinations` does not), and a number alone just before a move numbered `1.` names a puzzle. For a number no diagram gave, the open game ends and the moves stay unchecked, instead of being read into whatever game was open. On pages 297–298 and 1009, 11 of the 12 key moves whose diagrams were read are valid (the twelfth, `1.O-O-O`, has no rook on its diagram), and no solution of an unread diagram is validated by chance.

### Back to the main line after a prose digression

Annotated books leave the main line in prose (`2...Kd7? and gives away the h-pawn, White wins with 3.Kf4 Kd6 … 9.Kf5`) and come back to it in bold (`3.Kd4 Kf6!`). A numbered move like `3.Kd4` could also be an alternative inside the digression, to its own `3.Kf4`, and it used to be taken that way: the next move (`Kf6`, legal on the main line, not in the digression) then failed and stopped the line. Now a numbered move that is exactly the main line's next move, and legal there, goes back to the main line when the digression could only take it as an alternative to one of its own moves; a move that goes straight on from the digression (`1...c5 2.Nf3`) stays in it. On ten documents read earlier (Aagaard, Dvoretsky in two stretches, Fischer pt-BR, Flores Rios, Gallagher, Karpov, Polgar, Capablanca; replayed with `examples/walk_bench.rs`, no reading or OCR), valid moves went from 2,036 to 2,304 and moves not reached from 2,778 to 2,509; 13 moves got worse, in two games whose first chain of moves was itself a digression. Pages checked against the books: Fischer p. 46 (the Spielmann–Tartakover digression, then `7...♘c6 8.♗e3 ♘xd4?`), Flores Rios p. 60 (`If 14...♗xd6 15.♘xd6 …`, `Slightly better is 15...♕d7 …`), Dvoretsky p. 112.

### The main line by its printed weight

Annotated books print the main line in bold and their analysis in regular type (Dvoretsky, Fischer pt-BR). The text layer says which font each glyph is in, so each move now carries its weight (`bold` in the document's JSON, written only when true; OCR text and documents read before have none). When the book's analysis comes first (`Black wins without any trouble after 1...Be6+ 2.Kg6 …`, then the game's **`1...Bxg7?`**) or goes straight on from the main line's last move (**`4...Kg4`** `Tarrasch had expected 5.Kf6? c4 …` **`5.Kg6!`**), the first chain read used to become the main line. Now, in a game with moves of both weights:

- a bold move after regular ones goes on from the last bold move, when it can be played there (an unnumbered one only after a bold move that was played: after one that failed it would be read a ply early);
- a bold move played on the bold line makes that line the main line, so the analysis stays as variations; one that could only be read inside the analysis does not;
- a game whose moves all have one weight is read as before.

On the same ten stretches read again on 2026-10-20 (release `pdfcraft-cli`, the user's models; Karpov and Capablanca are scans, used as stored) and replayed with `examples/walk_bench.rs` with and without the weights (`--plain`), valid moves went from 2,305 to 2,515 and moves not reached from 2,509 to 2,329: 217 better, 7 worse. Checked against the books: Dvoretsky p. 52 (`1...♗xg7? 2.♗xf5! ♔xf5 3.♔xg7 … 11.♔xa5 ♔xb3`, before the analysis `1...Be6+ 2.Kg6`), p. 51 (`1.♔c8!! b5 … 5.♔d4=` and `1.♔c3? b1=♕ … 4.♔xa4 ♔c3`, which had the analysis `1.c8=Q? …` and `1.Qc4+ Ka3` as main lines), pp. 100–101, 108–110 (the whole of Matanovic's rook ending, `1.♖d6+? … 22...♖c1`, after the book's detour into a pawn ending and its `Let's return to diagram 1-134`), 113, 115 and 119; Fischer pp. 37–38 (`6.♗e2 e5 … 11.♕e1`, past `6 h3` and `6 ♗c4`), 43–44, 47 and 52–54. Not real gains: on Fischer p. 49 (16 moves) the game started from the digression `19...♘e5`, because the centred diagram is ordered after the short line `19 … ♗xd5` beside it, and on p. 55 (14) from a diagram read with White to move; those moves are legal there by chance. The 7 worse moves were valid only on a branch that was already wrong (Fischer pp. 33 and 35, after `51...♔d6` and `57...c3` failed; p. 56, in the game started from the digression `26 b3`).

Tried and not kept: guessing a different move in place of a failed one when the next printed moves fit exactly one move like it. All six moves it would have changed were printed correctly; each had failed because of something earlier (the line on the wrong branch, a short capture such as `hg` not read as a move), so guessing only hides the cause.

### A diagram centred above a short line

Fischer's *My 60 Memorable Games* centres each diagram (its caption printed beside it, `Position after 19 ♘d5!`) and sets the next move under it at the left margin (`19 … ♗xd5`), so the move lies wholly to the left of the diagram. A diagram used to go only before a line under it or in a column to its right, so it came after that move: the game was played on from a position before its last move, closed, and the book's analysis became a new game. A diagram now also goes before a line wholly below it on its side of the gutter, or of the page's middle on a page without one; a diagram or line across that middle is on both sides. On a two-column page, a left-column line below a right-column diagram is still read first. On the ten re-read stretches, valid moves went from 2,515 to 2,948 (all in Fischer pp. 30–59: 706 → 1,139 of 1,179; not reached 414 → 20; 18 games → 9), no move got worse, and the other nine stretches read exactly as before. Pages checked against the book: Fischer pp. 30, 31, 33, 34, 38, 47, 52, 56.

### Reading a selection (fragment mode)

The PGN toolbar captures a selection on the page into the game on the board (its spec, `plan/chess/pgn-toolbar.md`, step T2a for the reader; the capture in the app is steps T3 and T7a, see [Capturing text](#capturing-text) under the [PGN toolbar](#pgn-toolbar)). The reader is the same game walker that reads whole books, in a *fragment mode* (`chessvision::doc::fragment`): it starts from the position it is given (never renumbered from the moves), reads one game (a `1.e4` quoted in prose does not start another, and a move numbered before the start position is prose), and writes the game out as the items `Game::graft` takes, moves with their printed half-move, weight and box. The selection is read in the document's language, never one guessed from a few words (`Rc7` is a rook in English and a king in Portuguese). Rules added for it:

- a bracket lost in the text is assumed when a numbered move fits only outside the variation (flagged);
- a move without a number after prose words stays in the comment (`the break …f5`), unless it is bold;
- inside brackets, `;` before a numbered move and labels `a)`, `(b)` separate sibling variations, and a label is never a closing bracket;
- `1:0`, `1–0` and `(1-0)` end the selection as `1-0` does; what follows is left over, never dropped;
- `20.…`, `20. …Qd8` and `…Qd8` are notation, and a move after a lone ellipsis is Black's;
- a word cut by a hyphen at a line's end is joined only between letters (`posi-` `tion`), never after a square or a digit (`Nd7-` `b6`, `1-` `0`).

Whole books take only the rules that made no move worse on the ten stored documents (`examples/walk_bench.rs --rules=`): the assumed bracket, the separators and the ellipsis, 2,304 → 2,327 valid moves, 23 better and none worse. Moves in prose made 88 worse (books go on after prose without numbers) and the new results 3 (moves that were played into the wrong game before a `0:1` ended the right one), so those stay for selections. Reading every variation the walker built again as a selection, from the position before it, and grafting it (`examples/fragment_bench.rs`) gives the walker's own tree for 100% of Dvoretsky's (59 of 59), 94.1% of the second Dvoretsky stretch, 91.0% of Fischer's (81 of 89) and 94.2% overall (275 of 292). Most of the differences are the whole-book reading's: a numbered move after a prose digression goes back to the variation's own line in the selection, while the whole book keeps it in the digression.

## Limits

- **Moves and text**
  - A line stops at a move that cannot be played (a misprint, a misread diagram, or a line read onto the wrong branch): its later moves are "not reached" until a numbered move finds its place again. A game whose first chain of moves is a digression (the book's analysis of an earlier move printed before the game goes on) has that digression as its main line when the book does not set its main line in bold, or the page is a scan. A diagram centred beside a short line of moves can be read after that line, so a game can start from the analysis that follows it. A game whose diagram is on a page that was not read has no position.
  - Three or more columns are read as two at most.
  - A diagram goes among the lines by its place on the page: before the first line under its middle, in a column to its right, or wholly below it on its side of the gutter (or of the page's middle). On a one-column page, a diagram set entirely on one side of the middle still comes after a line below it on the other side.
  - Umlauts in prose come through only where the glyph reader has them, because ocrs reads ASCII only.
- **Links:** the lichess address is plain text in Word and RTF; it is a working link in PDF, HTML, EPUB and Markdown.
- **Readings and corrections**
  - A reading the decoder repaired is never linked or accepted until a person confirms it.
  - The book's cipher (what a figurine font or a misreading maps to) is carried from page to page and between reads while the document is open; headless runs save and load it with `cipher_out` / `cipher_in`. It is not saved inside the PDF.
- **PGN toolbar**
  - A PGN file keeps a comment before a move only where the move starts a variation; elsewhere it joins the comment after the move before (the app's autosave keeps it apart).
  - Drawing commands typed into a move's comment become drawings. Typed into the comment before a move, they stay text there, unless the move continues its line: the PGN then writes that comment with the one after the move before, and reading it back draws them on that move. `chess_pgn_edit` reads only the first game of a PGN; Open PGN… reads them all.
  - The game's undo history, and the list's, are not kept across sessions; a diagram opened on the board (a new game in the list) is not a step of the list's history. The lines the engine found for a position are kept for the session only.
  - `chess_games` works on PGN text and files: it cannot reach the games the app autosaved for a document.
  - The chess window has not yet been checked on the real executable across two monitors with different scaling (the rules are tested with this machine's 1920 × 1080 at 100% and 3440 × 1440 at 150%). Monitors are known only on the desktop; on the web, and where they can't be read, a window opens inside the main window. A window placed on a monitor of another scale is moved there once after it opens. A place saved before 2026-10-21 (U5) is not used: the window opens beside the main window once.
- **Speed:** reading a page with OCR, or exporting a whole book, runs off the interface thread (the panel shows it is working) but cannot be cancelled once started.
