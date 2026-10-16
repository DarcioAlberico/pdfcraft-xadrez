use std::sync::Arc;

use pdfcraft_testkit::{Pdf, stream_obj};

use super::*;

/// One page whose content is `content`, with `/XObject` resources `xobjects` (for example
/// `"/F1 5 0 R"`), followed by the object bodies in `extra` (numbered from 5).
fn page(content: &str, xobjects: &str, extra: &[Vec<u8>]) -> Document {
    let pdf = Pdf::new()
        .obj("<< /Type /Catalog /Pages 2 0 R >>")
        .obj("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>")
        .obj(format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << {xobjects} >> >> >>"))
        .stream("", content)
        .objs(extra);
    Document::open(Arc::new(pdf.build())).unwrap()
}

fn form(matrix: &str, xobjects: &str, body: &str) -> Vec<u8> {
    stream_obj(&format!("/Type /XObject /Subtype /Form /BBox [0 0 1000 1000] /Matrix [{matrix}] /Resources << /XObject << {xobjects} >> >>"), body)
}

fn close(a: [f64; 4], b: [f64; 4]) {
    assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9), "{a:?} != {b:?}");
}

#[test]
fn an_eight_by_eight_grid_of_filled_squares_is_64_items_in_user_space() {
    // Squares of 10 units, scaled by 2 and moved to (100, 50) by the CTM, alternating colours.
    let mut content = String::from("q 2 0 0 2 100 50 cm\n");
    for row in 0..8 {
        for col in 0..8 {
            let shade = if (row + col) % 2 == 0 { "0.9" } else { "0.2" };
            content.push_str(&format!("{shade} g {} {} 10 10 re f\n", col * 10, row * 10));
        }
    }
    content.push_str("Q\n0 0 m 1 1 l S\n");
    let inventory = page_paths(&page(&content, "", &[]), 0).unwrap();
    assert!(!inventory.truncated && inventory.unreadable == 0);
    // One item per painting operator: 64 squares, then the stroke after Q.
    assert_eq!(inventory.items.len(), 65);
    for row in 0..8 {
        for col in 0..8 {
            let item = &inventory.items[row * 8 + col];
            let (x, y) = (100.0 + 20.0 * col as f64, 50.0 + 20.0 * row as f64);
            close(item.bbox_user, [x, y, x + 20.0, y + 20.0]);
            let shade = if (row + col) % 2 == 0 { 0.9 } else { 0.2 };
            assert_eq!(item.fill, Some(PathColor::Gray(shade)));
            assert_eq!(item.stroke, None);
            // `re` is a move, three lines and a close: the closing edge is implicit.
            let [sub] = item.subpaths.as_slice() else { panic!("{:?}", item.subpaths) };
            assert!(sub.closed);
            assert_eq!(sub.start, [x, y]);
            assert_eq!(
                sub.segments,
                vec![PathSegment::Line([x + 20.0, y]), PathSegment::Line([x + 20.0, y + 20.0]), PathSegment::Line([x, y + 20.0])]
            );
        }
    }
    // Q restored the identity CTM and the initial black stroke colour.
    let last = &inventory.items[64];
    close(last.bbox_user, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!((last.fill.clone(), last.stroke.clone()), (None, Some(PathColor::Gray(0.0))));
    assert_eq!(inventory.segments, 64 * 3 + 1);
}

#[test]
fn rectangles_filled_by_one_operator_are_one_item_with_many_subpaths() {
    let mut content = String::from("1 0 0 rg\n");
    for row in 0..8 {
        for col in 0..8 {
            if (row + col) % 2 == 1 {
                content.push_str(&format!("{} {} 10 10 re\n", col * 10, row * 10));
            }
        }
    }
    content.push_str("f\n");
    let inventory = page_paths(&page(&content, "", &[]), 0).unwrap();
    let [item] = inventory.items.as_slice() else { panic!("{:?}", inventory.items.len()) };
    assert_eq!(item.subpaths.len(), 32);
    assert_eq!(item.fill, Some(PathColor::Rgb([1.0, 0.0, 0.0])));
    close(item.bbox_user, [0.0, 0.0, 80.0, 80.0]);
}

#[test]
fn forms_nested_two_deep_compose_matrices_and_inherit_colour() {
    // Page: translate (100, 200), then F1 scales by 2 and draws F2, which moves by (5, 5).
    let f1 = form("2 0 0 2 0 0", "/F2 6 0 R", "/F2 Do");
    let f2 = form("1 0 0 1 5 5", "", "0 0 10 10 re f 1 0 0 RG 0 0 m 10 0 l S");
    let content = "q 0 0 1 rg 1 0 0 1 100 200 cm /F1 Do Q 0 0 5 5 re f";
    let inventory = page_paths(&page(content, "/F1 5 0 R", &[f1, f2]), 0).unwrap();
    assert!(!inventory.truncated);
    let [square, line, after] = inventory.items.as_slice() else { panic!("{:?}", inventory.items) };
    close(square.bbox_user, [110.0, 210.0, 130.0, 230.0]);
    assert_eq!(square.fill, Some(PathColor::Rgb([0.0, 0.0, 1.0])));
    close(line.bbox_user, [110.0, 210.0, 130.0, 210.0]);
    assert_eq!(line.stroke, Some(PathColor::Rgb([1.0, 0.0, 0.0])));
    // Neither the form's transform nor the colour set before it leaked past Q.
    close(after.bbox_user, [0.0, 0.0, 5.0, 5.0]);
    assert_eq!(after.fill, Some(PathColor::Gray(0.0)));
}

#[test]
fn forms_that_draw_themselves_terminate_without_panicking() {
    // F1 draws itself; F2 and F3 draw each other.
    let f1 = form("1 0 0 1 0 0", "/F1 5 0 R", "0 0 10 10 re f /F1 Do");
    let f2 = form("1 0 0 1 0 0", "/F3 7 0 R", "20 0 10 10 re f /F3 Do");
    let f3 = form("1 0 0 1 0 0", "/F2 6 0 R", "40 0 10 10 re f /F2 Do");
    let inventory = page_paths(&page("/F1 Do /F2 Do", "/F1 5 0 R /F2 6 0 R", &[f1, f2, f3]), 0).unwrap();
    // Each form is drawn once per chain; the recursive invocation is skipped, not a limit hit.
    assert!(!inventory.truncated);
    let boxes: Vec<_> = inventory.items.iter().map(|i| i.bbox_user).collect();
    assert_eq!(boxes, vec![[0.0, 0.0, 10.0, 10.0], [20.0, 0.0, 30.0, 10.0], [40.0, 0.0, 50.0, 10.0]]);
}

#[test]
fn deep_form_chains_stop_at_the_depth_limit() {
    // F5 -> F6 -> ... : more distinct forms than the nesting limit, each drawing a square.
    let count = MAX_FORM_DEPTH + 8;
    let forms: Vec<_> = (0..count)
        .map(|i| {
            let next = if i + 1 < count { format!("/N {} 0 R", i + 6) } else { String::new() };
            form("1 0 0 1 0 0", &next, "0 0 1 1 re f /N Do")
        })
        .collect();
    let inventory = page_paths(&page("/N Do", "/N 5 0 R", &forms), 0).unwrap();
    assert!(inventory.truncated);
    assert_eq!(inventory.items.len(), MAX_FORM_DEPTH);
}

#[test]
fn more_segments_than_the_cap_reports_truncation() {
    let content = format!("0 0 5 5 re f 0 0 m {} S", "1 1 l ".repeat(MAX_SEGMENTS + 1));
    let inventory = page_paths(&page(&content, "", &[]), 0).unwrap();
    assert!(inventory.truncated);
    // What was painted before the limit is kept; the unfinished path is not reported.
    assert_eq!(inventory.items.len(), 1);
    assert!(inventory.segments <= MAX_SEGMENTS);
}

#[test]
fn more_items_than_the_cap_reports_truncation() {
    let content = "0 0 1 1 re f ".repeat(MAX_ITEMS + 1);
    let inventory = page_paths(&page(&content, "", &[]), 0).unwrap();
    assert!(inventory.truncated);
    assert_eq!(inventory.items.len(), MAX_ITEMS);
}

#[test]
fn decoded_bytes_are_capped_across_repeated_forms() {
    // A 1 MiB form drawn 20 times: the 16 MiB budget runs out after 16 drawings.
    let body = format!("{}\n0 0 1 1 re f", "%".repeat(1024 * 1024));
    let inventory = page_paths(&page(&"/F1 Do ".repeat(20), "/F1 5 0 R", &[form("1 0 0 1 0 0", "", &body)]), 0).unwrap();
    assert!(inventory.truncated);
    assert_eq!(inventory.items.len(), MAX_BYTES / (1024 * 1024) - 1);
}

#[test]
fn deep_graphics_state_stops_the_walk_and_keeps_what_was_found() {
    let content = format!("0 0 1 1 re f {} 0 0 1 1 re f", "q ".repeat(MAX_STATE_DEPTH + 1));
    let inventory = page_paths(&page(&content, "", &[]), 0).unwrap();
    assert!(inventory.truncated);
    assert_eq!(inventory.items.len(), 1);
}

#[test]
fn undecodable_streams_are_skipped_and_counted() {
    // An unsupported filter cannot be decoded at all (corrupt Flate is decoded tolerantly).
    let broken = stream_obj("/Filter /NoSuchDecode", "not data");
    let pdf = Pdf::new()
        .obj("<< /Type /Catalog /Pages 2 0 R >>")
        .obj("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>")
        .obj("<< /Type /Page /Parent 2 0 R /Contents [4 0 R 5 0 R] >>")
        .obj(broken)
        .stream("", "0 0 1 1 re f");
    let inventory = page_paths(&Document::open(Arc::new(pdf.build())).unwrap(), 0).unwrap();
    assert_eq!(inventory.unreadable, 1);
    assert_eq!(inventory.items.len(), 1);
    assert!(!inventory.truncated);

    // A broken form drawn many times is decoded and counted once.
    let broken = stream_obj("/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Filter /NoSuchDecode", "not data");
    let inventory = page_paths(&page(&"/F1 Do ".repeat(50), "/F1 5 0 R", &[broken]), 0).unwrap();
    assert_eq!((inventory.unreadable, inventory.truncated), (1, false));

    // Many distinct broken streams stop the walk.
    let parts: Vec<_> = (0..MAX_UNREADABLE + 4).map(|i| format!("{} 0 R", i + 5)).collect();
    let mut pdf = Pdf::new()
        .obj("<< /Type /Catalog /Pages 2 0 R >>")
        .obj("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>")
        .obj(format!("<< /Type /Page /Parent 2 0 R /Contents [{} 4 0 R] >>", parts.join(" ")))
        .stream("", "0 0 1 1 re f");
    for _ in 0..MAX_UNREADABLE + 4 {
        pdf = pdf.obj(stream_obj("/Filter /NoSuchDecode", "not data"));
    }
    let inventory = page_paths(&Document::open(Arc::new(pdf.build())).unwrap(), 0).unwrap();
    assert_eq!((inventory.unreadable, inventory.truncated, inventory.items.len()), (MAX_UNREADABLE, true, 0));
}

#[test]
fn paint_operators_colour_spaces_and_unpainted_paths() {
    let icc = stream_obj("/N 3", "");
    let pdf = Pdf::new()
        .obj("<< /Type /Catalog /Pages 2 0 R >>")
        .obj("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>")
        .obj("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /ColorSpace << /CS0 [/ICCBased 5 0 R] /Sep [/Separation /Gold /DeviceCMYK 6 0 R] >> >> >>")
        .stream(
            "",
            "0 0 10 10 re W n \
             /CS0 cs 0.1 0.2 0.3 scn 0 0 1 RG 0 0 10 10 re B \
             /Sep CS 0.5 SCN 0 0 m 10 10 20 10 30 0 c S \
             0 0 0 1 k 0 0 1 1 re f* \
             0 0 m n",
        )
        .obj(icc)
        .obj("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0.2 1 0] /N 1 >>");
    let inventory = page_paths(&Document::open(Arc::new(pdf.build())).unwrap(), 0).unwrap();
    // The clip and the `n` path are not painted.
    let [both, curve, cmyk] = inventory.items.as_slice() else { panic!("{:?}", inventory.items) };
    assert_eq!(both.fill, Some(PathColor::Rgb([0.1, 0.2, 0.3])));
    assert_eq!(both.stroke, Some(PathColor::Rgb([0.0, 0.0, 1.0])));
    assert_eq!(curve.fill, None);
    assert_eq!(curve.stroke, Some(PathColor::Other(vec![0.5])));
    // The curve's box is its real extent (y = 7.5 at the top), not its control points'.
    close(curve.bbox_user, [0.0, 0.0, 30.0, 7.5]);
    assert!(matches!(curve.subpaths[0].segments[..], [PathSegment::Cubic(..)]));
    assert_eq!(cmyk.fill, Some(PathColor::Cmyk([0.0, 0.0, 0.0, 1.0])));
}

#[test]
fn a_missing_page_is_an_error_and_an_empty_page_is_empty() {
    let doc = page("", "", &[]);
    assert_eq!(page_paths(&doc, 1), Err(EditError::NoSuchPage(1)));
    assert_eq!(page_paths(&doc, 0).unwrap(), PathInventory::default());
}
