# pdfcraft-edit

Layer L4: page content editing. Page marks (header and footer, watermark, background, Bates numbering), added text and images, editing existing text, images and mixed objects, and flattening. The crate documentation (`src/lib.rs` and each module) describes how each is stored.

## Vector path inventory (`paths`)

A read-only list of the vector paths a page paints, for detectors that look for drawn shapes (the chess-diagram detector finds boards drawn as 64 alternating squares).

```rust
let inventory = edit::paths::page_paths(&doc, page)?;   // only a missing page is an error
for item in &inventory.items {
    item.bbox_user;          // [x0, y0, x1, y1] in user space, after the CTM
    item.fill;               // Option<PathColor>: Gray, Rgb, Cmyk or Other(components)
    item.stroke;
    item.subpaths;           // start point, Line / Cubic segments, closed flag
}
inventory.truncated;         // a limit stopped the walk; items holds what came before it
inventory.unreadable;        // content streams skipped because they could not be decoded
```

There is one item per painting operator (`S`, `s`, `f`, `F`, `f*`, `B`, `B*`, `b`, `b*`). A board drawn as 64 `re f` squares is 64 items; 32 `re` followed by one `f` is one item with 32 subpaths. A path ended by `n`, such as a clipping path, is not reported. `re` is a move, three lines and a close: the closing edge is implicit (`closed`), not a segment.

The walk follows `q`/`Q`, `cm`, colour operators (`g`, `rg`, `k`, `cs`/`sc`/`scn` and their stroking forms) and Form XObjects with their `/Matrix`. A form starts from the graphics state at its `Do`. A form that draws itself, directly or through other forms, is drawn once per chain and the recursive `Do` is skipped. Boxes are the exact extent of the segments, curve extrema included.

Limits per page, each reported as `truncated`: 20,000 items, 100,000 segments, 16 MiB of decoded content (a single stream larger than that counts as unreadable), 4,096 streams, 16 undecodable streams, 32 nested forms and 256 nested `q`. A form drawn many times is decoded each time and counts against the byte and stream budgets each time. A broken one is decoded and counted once.

Not interpreted: clipping, line width, caps and joins, optional-content visibility, annotation appearances, Type 3 glyphs, the content of patterns, and shadings. Colours are reported as set, not converted. ICCBased spaces with 1, 3 or 4 components and CalGray/CalRGB are reported as the matching device family, and other spaces as raw components.

The design follows the snapping extractor in `pdfcraft-measure`. That code stays in `measure` because `measure → edit` is a same-layer edge the layering does not allow, and measure's walker is built around snap targets.
