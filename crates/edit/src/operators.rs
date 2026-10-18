//! Draw arbitrary content on a page: a fragment of content-stream operators, checked first.
//!
//! [`add_operators`] appends the operators to the page as their own content stream, tagged
//! `/PCMark /Content`, on top of the page's content (which is wrapped in `q … Q` first, as for
//! every mark; see the crate docs). They draw in the page's default user space. The fragment is
//! wrapped in its own `q … Q`, so nothing it sets leaks into content added after it.
//!
//! The operators come from callers that may be agents, so [`check_operators`] refuses what could
//! break the page or reach outside the fragment: anything that is not an operator or operand,
//! unbalanced `q`/`Q`, `BT`/`ET` or marked-content scopes, path operators inside a text object,
//! wrong operand counts or non-finite numbers, and every operator that needs a page resource
//! (`Do`, `gs`, `sh`, inline images, patterns, named colour spaces, property lists). Text uses
//! the standard fonts the stamp itself provides (`/PCHelv`, `/PCTimes`, `/PCCour`, WinAnsi).

use pdfcraft_content::{Op, parse};
use pdfcraft_cos::{Document, Object};

use crate::EditError;

/// Largest operator fragment accepted, in bytes.
pub const MAX_OPERATOR_BYTES: usize = 4 * 1024 * 1024;
/// Deepest `q` nesting accepted. ISO 32000-1 Annex C lists 28 as a conforming reader's limit.
pub const MAX_SAVE_DEPTH: usize = 28;
/// Deepest marked-content nesting accepted.
pub const MAX_MARKED_DEPTH: usize = 64;
/// The `/PCMark` tag of the stream [`add_operators`] adds.
pub const TAG: &str = "Content";

/// The standard fonts a fragment may select with `Tf` (see `add_resources_with`).
const FONTS: &[&[u8]] = &[b"PCHelv", b"PCTimes", b"PCCour"];

/// What an operator's operands must be.
#[derive(Clone, Copy)]
enum Operands {
    /// Exactly this many finite numbers.
    Numbers(usize),
    /// From one to four finite numbers (a colour in the current space).
    Colour,
    /// One name.
    Name,
    /// A name and a finite number (`Tf`).
    Font,
    /// A string (`Tj`, `'`).
    Text,
    /// An array of strings and finite numbers (`TJ`).
    TextArray,
    /// Two finite numbers and a string (`"`).
    SpacedText,
    /// An array of finite numbers and a finite number (`d`).
    Dash,
    /// A name and an inline dictionary (`BDC`, `DP`).
    Tagged,
}

/// Where an operator may appear.
#[derive(Clone, Copy, PartialEq)]
enum Place {
    /// Anywhere: outside a text object or inside one.
    Anywhere,
    /// Only outside a text object (paths, `q`/`Q`, `cm`, `BT`).
    Outside,
    /// Only inside a text object.
    Inside,
}

/// The operators a fragment may use (ISO 32000-2 Table 50), with their operands and place.
fn rule(op: &[u8]) -> Option<(Operands, Place)> {
    use Operands::*;
    use Place::*;
    Some(match op {
        // General graphics state.
        b"w" | b"J" | b"j" | b"M" | b"i" => (Numbers(1), Anywhere),
        b"d" => (Dash, Anywhere),
        b"ri" => (Name, Anywhere),
        // Special graphics state.
        b"q" | b"Q" => (Numbers(0), Outside),
        b"cm" => (Numbers(6), Outside),
        // Path construction, painting and clipping.
        b"m" | b"l" => (Numbers(2), Outside),
        b"c" => (Numbers(6), Outside),
        b"v" | b"y" | b"re" => (Numbers(4), Outside),
        b"h" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n" | b"W" | b"W*" => (Numbers(0), Outside),
        // Text objects, state, positioning and showing.
        b"BT" => (Numbers(0), Outside),
        b"ET" => (Numbers(0), Inside),
        b"Tc" | b"Tw" | b"Tz" | b"TL" | b"Tr" | b"Ts" => (Numbers(1), Anywhere),
        b"Tf" => (Font, Anywhere),
        b"Td" | b"TD" => (Numbers(2), Inside),
        b"Tm" => (Numbers(6), Inside),
        b"T*" => (Numbers(0), Inside),
        b"Tj" | b"'" => (Text, Inside),
        b"TJ" => (TextArray, Inside),
        b"\"" => (SpacedText, Inside),
        // Colour: device spaces only.
        b"CS" | b"cs" => (Name, Anywhere),
        b"SC" | b"sc" | b"SCN" | b"scn" => (Colour, Anywhere),
        b"G" | b"g" => (Numbers(1), Anywhere),
        b"RG" | b"rg" => (Numbers(3), Anywhere),
        b"K" | b"k" => (Numbers(4), Anywhere),
        // Marked content, with inline property lists only.
        b"MP" | b"BMC" => (Name, Anywhere),
        b"DP" | b"BDC" => (Tagged, Anywhere),
        b"EMC" => (Numbers(0), Anywhere),
        _ => return None,
    })
}

fn finite(o: &Object) -> bool {
    o.as_f64().is_some_and(f64::is_finite)
}

fn operands_fit(kind: Operands, operands: &[Object]) -> bool {
    match (kind, operands) {
        (Operands::Numbers(n), all) => all.len() == n && all.iter().all(finite),
        (Operands::Colour, all) => (1..=4).contains(&all.len()) && all.iter().all(finite),
        (Operands::Name, [Object::Name(_)]) => true,
        (Operands::Font, [Object::Name(name), size]) => FONTS.contains(&name.as_slice()) && finite(size),
        (Operands::Text, [Object::String(_)]) => true,
        (Operands::TextArray, [Object::Array(items)]) => items.iter().all(|o| matches!(o, Object::String(_)) || finite(o)),
        (Operands::SpacedText, [w, c, Object::String(_)]) => finite(w) && finite(c),
        (Operands::Dash, [Object::Array(dashes), phase]) => dashes.iter().all(finite) && finite(phase),
        (Operands::Tagged, [Object::Name(_), Object::Dict(_)]) => true,
        _ => false,
    }
}

fn refuse(at: usize, op: &Op, why: &str) -> EditError {
    EditError::Invalid(format!("operator {} ({}): {why}", at + 1, String::from_utf8_lossy(&op.op)))
}

/// Check that `ops` is a self-contained fragment [`add_operators`] may draw (see the module docs).
/// Returns the number of operators.
pub fn check_operators(ops: &[u8]) -> Result<usize, EditError> {
    if ops.len() > MAX_OPERATOR_BYTES {
        return Err(EditError::Invalid(format!("the content is {} bytes; at most {MAX_OPERATOR_BYTES} are accepted", ops.len())));
    }
    let parsed = parse(ops);
    if parsed.skipped > 0 {
        return Err(EditError::Invalid(format!("the content has {} bytes or operands that are not part of an operator", parsed.skipped)));
    }
    if parsed.ops.is_empty() {
        return Err(EditError::Invalid("the content has no operators".into()));
    }
    let (mut saves, mut marked, mut in_text) = (0usize, 0usize, false);
    for (at, op) in parsed.ops.iter().enumerate() {
        let Some((operands, place)) = rule(&op.op) else {
            return Err(refuse(at, op, "not allowed: only path, colour, text and marked-content operators that need no page resource"));
        };
        if !operands_fit(operands, &op.operands) {
            return Err(refuse(at, op, "wrong operands"));
        }
        match (place, in_text) {
            (Place::Outside, true) => return Err(refuse(at, op, "not allowed inside a text object (BT … ET)")),
            (Place::Inside, false) => return Err(refuse(at, op, "only allowed inside a text object (BT … ET)")),
            _ => {}
        }
        match op.op.as_slice() {
            b"q" => {
                saves += 1;
                if saves > MAX_SAVE_DEPTH {
                    return Err(refuse(at, op, &format!("q is nested more than {MAX_SAVE_DEPTH} deep")));
                }
            }
            b"Q" => saves = saves.checked_sub(1).ok_or_else(|| refuse(at, op, "Q without a q before it"))?,
            b"BT" => in_text = true,
            b"ET" => in_text = false,
            b"BMC" | b"BDC" => {
                marked += 1;
                if marked > MAX_MARKED_DEPTH {
                    return Err(refuse(at, op, &format!("marked content is nested more than {MAX_MARKED_DEPTH} deep")));
                }
            }
            b"EMC" => marked = marked.checked_sub(1).ok_or_else(|| refuse(at, op, "EMC without a BMC or BDC before it"))?,
            b"CS" | b"cs" if !matches!(op.name(0), Some(b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK")) => {
                return Err(refuse(at, op, "only DeviceGray, DeviceRGB and DeviceCMYK"));
            }
            _ => {}
        }
    }
    if saves > 0 {
        return Err(EditError::Invalid(format!("{saves} q without a matching Q")));
    }
    if in_text {
        return Err(EditError::Invalid("a text object (BT) is not closed with ET".into()));
    }
    if marked > 0 {
        return Err(EditError::Invalid(format!("{marked} marked-content scope(s) not closed with EMC")));
    }
    Ok(parsed.ops.len())
}

/// Draw `ops` on top of `page` (0-based), in the page's default user space, after checking them
/// ([`check_operators`]). The stream is tagged [`TAG`]: page content from then on, which Remove
/// header, watermark or background never takes away.
pub fn add_operators(doc: &mut Document, page: usize, ops: &[u8]) -> Result<(), EditError> {
    check_operators(ops)?;
    let mut content = Vec::with_capacity(ops.len().saturating_add(8));
    content.extend_from_slice(b"q\n");
    content.extend_from_slice(ops);
    content.extend_from_slice(b"\nQ\n");
    crate::stamp(doc, page, TAG, content)
}

#[cfg(test)]
mod tests;
