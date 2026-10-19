# Chess books

**Status:** current to 2026-10-18 (blueprint steps S1–S18; see [`chess/blueprint.md`](chess/blueprint.md)).
**Revision history:** 2026-10-18 — first version, after S18.

PdfCraft can read a chess book in PDF. It finds the diagrams, reads each one as a position (FEN), and reads the moves in the text, including figurines and annotation signs. It can redraw a diagram, put a lichess link under it, and export the book as HTML, EPUB, Word, RTF, Markdown or PGN. Everything runs on your machine; nothing is sent anywhere.

## What you need

None of these files ship with PdfCraft (fork decisions D2 and D4). Without them, the rest still works.

| File | What it does | Without it |
|---|---|---|
| `chess-pieces.onnx` + `.json` | reads drawn and scanned diagrams | only diagrams set in a chess diagram font are read; the others are found and marked |
| `chess-glyphs.onnx` + `.json` | chess OCR: reads moves, figurines and signs from page images | pages without a usable text layer are listed as needing OCR |
| `text-detection.rten`, `text-recognition.rten` (ocrs) | reads the prose around the notation | the glyph reader reads the prose too, less well |
| a chess diagram font (Chess Merida) | draws redrawn diagrams with real pieces | plain geometric pieces |

PdfCraft looks for the models in the folder named by `PDFCRAFT_MODELS`, then beside the executable. For the pieces it uses the font named by `PDFCRAFT_CHESS_PIECES`, then an installed Chess Merida. A diagram drawn with Merida or cburnett pieces carries those pieces into what you export. That is fine for personal use, but not for publishing.

## In the app

Open the **Chess diagrams** panel (`chess.panel`), or run **Chess ▸ Scan** (`chess.scan`).

- **Scan this page** or **Scan all pages** find the diagrams. Pages are scanned in batches in the background, and each batch shows as soon as it is done.
  - Boxes on the page show each diagram's state: read, needs review, not read, or failed.
  - Any edit makes the scan stale. The boxes leave the page until you scan again.
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
- **Export…** writes the whole book as HTML, EPUB, Word, RTF, Markdown or PGN. Markdown's diagram images go in a folder beside the file.

## Headless: tools and CLI

Every feature is also a tool (`pdfcraft-cli tools`; the MCP server is opt-in):

| Tool | Does |
|---|---|
| `chess_status` | which models and piece set are available, and why not |
| `chess_detect`, `chess_read` | find the diagrams; read them (FEN, legality, side to move, weakest square, uncertain squares, repairs, lichess URL) |
| `chess_document` | the book as headings, paragraphs of moves and diagrams, with the games replayed into variation trees; JSON and PGN |
| `chess_replace_diagram`, `content_draw` | redraw a diagram (one undo step); the generic checked-operators edit under it |
| `chess_add_lichess_links` | a label and link under every legal, accepted diagram |
| `chess_export_html`, `_epub`, `_docx`, `_rtf`, `_markdown`, `_pgn` | the exports |

`pdfcraft-cli chess scan FILE` prints the diagrams found. All paths are confined to `--root`.

The pages to OCR are chosen with `ocr`: `auto` (the default) reads only pages whose text layer is poor or missing, `off` reads none, and `all` reads every page.

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

## Limits

- **Moves and text**
  - A line stops at a move that cannot be played (a misprint, or a misread diagram): its later moves are "not reached" until a numbered move finds its place again. A game whose diagram is on a page that was not read has no position.
  - Three or more columns are read as two at most.
  - Umlauts in prose come through only where the glyph reader has them, because ocrs reads ASCII only.
- **Links:** the lichess address is plain text in Word and RTF; it is a working link in PDF, HTML, EPUB and Markdown.
- **Readings and corrections**
  - A reading the decoder repaired is never linked or accepted until a person confirms it.
  - The book's cipher (what a figurine font or a misreading maps to) is carried from page to page and between reads while the document is open; headless runs save and load it with `cipher_out` / `cipher_in`. It is not saved inside the PDF.
- **Speed:** reading a page with OCR, or exporting a whole book, runs off the interface thread (the panel shows it is working) but cannot be cancelled once started.
