use std::sync::Arc;

use pdfcraft_cos::{Document, Object, SaveOptions, write_incremental};
use proptest::prelude::*;

use super::*;

/// One 600 × 800 page whose content leaves the graphics state changed (an unbalanced `cm`).
fn page() -> Document {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>",
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        "<< /Length 37 >>\nstream\n2 0 0 2 0 0 cm BT /F1 9 Tf (Hi) Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

/// The page's content streams: (`/PCMark` tag, decoded bytes).
fn streams(doc: &Document) -> Vec<(Option<String>, String)> {
    let p = &pdfcraft_model::pages(doc)[0];
    let list = match p.dict.get(b"Contents").map(|c| doc.resolve(c)).as_deref() {
        Some(Object::Array(a)) => a.clone(),
        _ => vec![p.dict.get(b"Contents").cloned().unwrap()],
    };
    list.iter()
        .map(|o| match &*doc.resolve(o) {
            Object::Stream(s) => {
                (s.dict.name(b"PCMark").map(|n| String::from_utf8_lossy(n).into_owned()), String::from_utf8_lossy(&s.decoded().unwrap()).into_owned())
            }
            other => panic!("not a stream: {other:?}"),
        })
        .collect()
}

const BOARD: &str = "0.5 0.25 0 rg 10 20 30 40 re f\n1 1 1 RG 2 w 1 j 10 20 m 40 60 l 50 60 55 70 60 80 c h S\n\
    0 g BT /PCHelv 8 Tf 1 0 0 1 12 22 Tm (a) Tj [(b) -20 (c)] TJ ET\n/Span <</ActualText (x)>> BDC 0 0 1 1 re W n EMC\n";

/// A checked fragment goes on top of the page, after the original content (wrapped in q … Q so
/// its unbalanced `cm` cannot scale the new drawing), in a stream of its own tagged `Content`,
/// wrapped in its own q … Q. Helvetica is there for its text.
#[test]
fn a_fragment_is_drawn_on_top_in_its_own_tagged_stream() {
    let mut doc = page();
    assert_eq!(check_operators(BOARD.as_bytes()).unwrap(), 23);
    add_operators(&mut doc, 0, BOARD.as_bytes()).unwrap();
    let bytes = write_incremental(&doc, &SaveOptions::default()).unwrap();
    let doc = Document::open(Arc::new(bytes)).unwrap();
    let s = streams(&doc);
    let tags: Vec<Option<&str>> = s.iter().map(|(t, _)| t.as_deref()).collect();
    assert_eq!(tags, [Some("Wrap"), None, Some("Wrap"), Some(TAG)]);
    assert_eq!(s[3].1, format!("q\n{BOARD}\nQ\n"));
    let p = &pdfcraft_model::pages(&doc)[0];
    let res = doc.resolve(p.dict.get(b"Resources").unwrap()).as_dict().cloned().unwrap();
    let fonts = doc.resolve(res.get(b"Font").unwrap()).as_dict().cloned().unwrap();
    assert!(fonts.get(b"PCHelv").is_some() && fonts.get(b"F1").is_some(), "the page keeps its own font and gains Helvetica");
}

#[test]
fn a_page_that_does_not_exist_is_refused() {
    let mut doc = page();
    assert_eq!(add_operators(&mut doc, 1, b"0 0 1 1 re f"), Err(EditError::NoSuchPage(1)));
}

/// Everything that could break the page or reach outside the fragment is refused, with a reason.
#[test]
fn unsafe_or_malformed_fragments_are_refused() {
    let refused = [
        ("q 0 0 1 1 re f", "without a matching Q"),
        ("Q", "Q without a q"),
        ("q Q Q q", "Q without a q"),
        ("BT /PCHelv 9 Tf (a) Tj", "not closed with ET"),
        ("ET", "only allowed inside a text object"),
        ("BT 0 0 1 1 re ET", "not allowed inside a text object"),
        ("BT q ET", "not allowed inside a text object"),
        ("(a) Tj", "only allowed inside a text object"),
        ("/BMC1 BMC", "not closed with EMC"),
        ("EMC", "EMC without"),
        ("/Im1 Do", "not allowed"),
        ("/GS1 gs", "not allowed"),
        ("/Sh1 sh", "not allowed"),
        ("BI /W 1 /H 1 /BPC 8 /CS /G ID \x00 EI", "not allowed"),
        ("0 0 1 1 re W n d0", "not allowed"),
        ("BT /F1 9 Tf ET", "wrong operands"),
        ("/Pattern cs", "only DeviceGray"),
        ("/CS0 CS", "only DeviceGray"),
        ("/P1 scn", "wrong operands"),
        ("0.5 0.5 0.5 0.5 0.5 sc", "wrong operands"),
        ("/OC /MC0 BDC EMC", "wrong operands"),
        ("1 2 3 re", "wrong operands"),
        ("1 2 3 4 5 re", "wrong operands"),
        ("(1) 2 m", "wrong operands"),
        ("1 2 3 4 5 6 7 cm", "wrong operands"),
        ("0 0 m 1 1 l S 3", "not part of an operator"),
        ("0 0 m ) 1 1 l", "not part of an operator"),
        ("", "no operators"),
        ("% only a comment", "no operators"),
    ];
    for (ops, why) in refused {
        let err = check_operators(ops.as_bytes()).expect_err(ops);
        assert!(err.to_string().contains(why), "{ops:?}: {err}");
        let mut doc = page();
        assert!(add_operators(&mut doc, 0, ops.as_bytes()).is_err(), "{ops:?}");
        assert!(!doc.is_modified(), "{ops:?}: a refused fragment changes nothing");
    }
    // Numbers too large to be finite.
    let huge = format!("{} 0 m", "9".repeat(400));
    assert!(check_operators(huge.as_bytes()).is_err());
    // Nesting is capped.
    let deep = format!("{}{}", "q ".repeat(MAX_SAVE_DEPTH + 1), "Q ".repeat(MAX_SAVE_DEPTH + 1));
    assert!(check_operators(deep.as_bytes()).unwrap_err().to_string().contains("nested"));
    let ok = format!("{}{}", "q ".repeat(MAX_SAVE_DEPTH), "Q ".repeat(MAX_SAVE_DEPTH));
    assert!(check_operators(ok.as_bytes()).is_ok());
    let marked = format!("{}{}", "/A BMC ".repeat(MAX_MARKED_DEPTH + 1), "EMC ".repeat(MAX_MARKED_DEPTH + 1));
    assert!(check_operators(marked.as_bytes()).unwrap_err().to_string().contains("nested"));
}

#[test]
fn the_size_is_capped() {
    let one = "0 0 1 1 re f\n";
    let mut big = one.repeat(MAX_OPERATOR_BYTES / one.len());
    assert!(check_operators(big.as_bytes()).is_ok());
    big.push_str(one);
    assert!(check_operators(big.as_bytes()).unwrap_err().to_string().contains("at most"));
}

#[test]
fn device_colour_spaces_and_the_standard_fonts_are_allowed() {
    for ops in
        ["/DeviceRGB cs 1 0 0 sc 0 0 1 1 re f", "/DeviceCMYK CS 0 0 0 1 SCN", "BT /PCTimes 9 Tf (a) ' 1 2 (b) \" ET", "[3 2] 0 d 0 0 m 1 1 l S"]
    {
        check_operators(ops.as_bytes()).unwrap_or_else(|e| panic!("{ops}: {e}"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Arbitrary bytes are checked without panicking, and whatever is accepted stays balanced
    /// once drawn: the page's content still parses with every q closed.
    #[test]
    fn arbitrary_fragments_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256), words in proptest::collection::vec(prop::sample::select(vec!["q", "Q", "BT", "ET", "1", "0", "/PCHelv", "(a)", "Tf", "Tj", "re", "f", "m", "l", "cm", "BMC", "/A", "EMC", "W", "n"]), 0..40)) {
        let _ = check_operators(&bytes);
        let joined = words.join(" ");
        let mut doc = page();
        if add_operators(&mut doc, 0, joined.as_bytes()).is_ok() {
            let mut depth = 0i64;
            for op in pdfcraft_content::parse(joined.as_bytes()).ops {
                depth += i64::from(op.is("q")) - i64::from(op.is("Q"));
                prop_assert!(depth >= 0);
            }
            prop_assert_eq!(depth, 0);
        }
    }
}
