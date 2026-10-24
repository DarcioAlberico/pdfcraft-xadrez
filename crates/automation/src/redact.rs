//! Redaction tools: mark areas, text, patterns or whole pages for redaction, apply the marks
//! (removing what they cover for good) or clear them.

use pdfcraft_engine::{
    Edit, EraseOptions, GlyphRule, Hidden, MAX_ERASE_AREAS, NewAnnotation, REDACT_MAX_WORDS, REDACTION_CODE_SETS, RedactPattern, RedactionCodeSet,
    Shape, Style, find_pattern, rect_quad, redact_word_list,
};
use serde_json::{Value, json};

use crate::comments::{DEFAULT_AUTHOR, parse_color};
use crate::{Args, Automation, Result, ToolError, failed};

impl Automation {
    pub(crate) fn redact_mark(&mut self, a: &Args) -> Result<Value> {
        let doc = self.doc(a)?;
        let (id, n) = (doc.id, doc.info.pages.len());
        let pages: Vec<usize> = match a.opt_ints("pages")? {
            Some(_) => self.pages(a, "pages")?,
            None => (0..n).collect(),
        };
        let overlay = match (a.opt_str("overlay")?, a.opt_str("code_set")?) {
            (Some(_), Some(_)) => return Err(ToolError::InvalidArgs("pass overlay or code_set with codes, not both".into())),
            (Some(o), None) => o.to_string(),
            (None, Some(id)) => {
                let set = RedactionCodeSet::from_id(id).ok_or_else(|| {
                    let ids: Vec<_> = REDACTION_CODE_SETS.iter().map(|s| s.id).collect();
                    ToolError::InvalidArgs(format!("unknown code_set {id:?}; allowed: {}", ids.join(", ")))
                })?;
                let picked = a.strs("codes")?;
                let allowed = || format!("allowed in {}: {}", set.id, set.codes.join(", "));
                match set.overlay(&picked) {
                    Ok(o) if !o.is_empty() => o,
                    Ok(_) => return Err(ToolError::InvalidArgs(format!("codes is empty; {}", allowed()))),
                    Err(bad) => return Err(ToolError::InvalidArgs(format!("unknown code {bad:?}; {}", allowed()))),
                }
            }
            (None, None) if a.get("codes").is_some() => return Err(ToolError::InvalidArgs("codes needs code_set".into())),
            (None, None) => String::new(),
        };
        let author = a.opt_str("author")?.unwrap_or(DEFAULT_AUTHOR).to_string();
        let mut style = Style::default_for(&Shape::Redact { quads: Vec::new(), overlay: String::new(), look: Default::default() });
        if let Some(c) = a.opt_str("fill")? {
            style.fill = Some(parse_color(c)?);
        }
        // (page, quads) per mark.
        let mut marks: Vec<(usize, Vec<[f64; 8]>)> = Vec::new();
        let chosen = [
            a.get("rect").is_some(),
            a.get("find").is_some(),
            a.get("words").is_some(),
            a.get("pattern").is_some(),
            a.opt_bool("whole_pages")?.unwrap_or(false),
        ];
        if chosen.iter().filter(|c| **c).count() != 1 {
            return Err(ToolError::InvalidArgs("pass exactly one of rect (with page), find, words, pattern or whole_pages: true".into()));
        }
        let case_sensitive = a.opt_bool("case_sensitive")?.unwrap_or(false);
        let whole_words = a.opt_bool("whole_words")?.unwrap_or(false);
        if (case_sensitive || whole_words) && a.get("find").is_none() && a.get("words").is_none() {
            return Err(ToolError::InvalidArgs("case_sensitive and whole_words apply only to find or words".into()));
        }
        // Per searched word or phrase: how many marks it made.
        let mut matched_words: Vec<(String, usize)> = Vec::new();
        if a.get("rect").is_some() {
            let page = self.page(a)?;
            let info = &self.doc(a)?.info.pages[page];
            let r: Vec<f64> = a.get("rect").and_then(Value::as_array).map(|x| x.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let r = <[f64; 4]>::try_from(r).map_err(|_| ToolError::InvalidArgs("rect must be 4 numbers".into()))?;
            let q = info.view_rect_to_quad([r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32]);
            marks.push((page, vec![q]));
        } else if a.opt_bool("whole_pages")?.unwrap_or(false) {
            let info = &self.doc(a)?.info;
            for &p in &pages {
                let c = info.pages[p].crop;
                marks.push((p, vec![rect_quad([c[0] as f64, c[1] as f64, c[2] as f64, c[3] as f64])]));
            }
        } else {
            let pattern = match a.opt_str("pattern")? {
                Some(p) => Some(RedactPattern::from_id(p).ok_or_else(|| ToolError::InvalidArgs(format!("unknown pattern {p:?}")))?),
                None => None,
            };
            let needles: Vec<String> = match (a.opt_str("find")?, a.get("words")) {
                (Some(f), _) => vec![f.to_owned()],
                (None, Some(_)) => {
                    let raw = a.strs("words")?;
                    if raw.len() > REDACT_MAX_WORDS {
                        return Err(ToolError::InvalidArgs(format!("words has {} entries; at most {REDACT_MAX_WORDS} are allowed", raw.len())));
                    }
                    let list = redact_word_list(&raw.join("\n"));
                    if list.is_empty() {
                        return Err(ToolError::InvalidArgs("words must hold at least one non-empty word or phrase".into()));
                    }
                    list
                }
                (None, None) => Vec::new(),
            };
            matched_words = needles.iter().map(|w| (w.clone(), 0)).collect();
            let texts = self.page_texts(id, &pages)?;
            let info = self.doc(a)?.info.clone();
            for (&p, text) in pages.iter().zip(&texts) {
                let hits: Vec<(Option<usize>, _)> = match pattern {
                    Some(pat) => text.find_with(|chars| find_pattern(pat, chars)).into_iter().map(|h| (None, h)).collect(),
                    None => needles
                        .iter()
                        .enumerate()
                        .flat_map(|(i, w)| text.find_opts(w, case_sensitive, whole_words).into_iter().map(move |h| (Some(i), h)))
                        .collect(),
                };
                for (word, h) in hits {
                    let quads: Vec<[f64; 8]> = text.line_rects(h).into_iter().map(|r| info.pages[p].view_rect_to_quad(r)).collect();
                    if !quads.is_empty() {
                        marks.push((p, quads));
                        if let Some(m) = word.and_then(|i| matched_words.get_mut(i)) {
                            m.1 += 1;
                        }
                    }
                }
            }
            if marks.is_empty() {
                return Err(failed("nothing matched, so nothing was marked"));
            }
        }
        let count = marks.len();
        let edits: Vec<Edit> = marks
            .iter()
            .map(|(page, quads)| {
                let shape = Shape::Redact { quads: quads.clone(), overlay: overlay.clone(), look: Default::default() };
                Edit::AddAnnotation(NewAnnotation { page: *page, shape, style: style.clone(), contents: String::new(), author: author.clone() })
            })
            .collect();
        let edit = match <[Edit; 1]>::try_from(edits) {
            Ok([one]) => one,
            Err(edits) => Edit::Batch { label: "Mark for redaction".into(), edits },
        };
        let mut out = self.apply(a, edit)?;
        out["marked"] = json!(count);
        out["pages_marked"] = json!(marks.iter().map(|m| m.0 + 1).collect::<std::collections::BTreeSet<_>>());
        out["marks_pending"] = json!(self.doc(a)?.redaction_marks());
        if a.get("words").is_some() {
            out["matched_words"] = json!(matched_words.iter().map(|(w, n)| json!({ "word": w, "marked": n })).collect::<Vec<_>>());
        }
        Ok(out)
    }

    pub(crate) fn redact_apply(&mut self, a: &Args) -> Result<Value> {
        let pages = match a.opt_ints("pages")? {
            Some(_) => Some(self.pages(a, "pages")?),
            None => None,
        };
        let before = self.doc(a)?.redaction_marks();
        if before == 0 {
            return Err(failed("there are no redaction marks to apply (see redact_mark)"));
        }
        let mut out = self.apply(a, Edit::ApplyRedactions { pages })?;
        let after = self.doc(a)?.redaction_marks();
        out["applied"] = json!(before - after);
        out["marks_pending"] = json!(after);
        Ok(out)
    }

    /// `page_erase`: remove the page's content under rectangles of the displayed page, as an
    /// ordinary edit (no marks, no boxes).
    pub(crate) fn page_erase(&mut self, a: &Args) -> Result<Value> {
        let page = self.page(a)?;
        let rects =
            a.get("rects").and_then(Value::as_array).ok_or_else(|| ToolError::InvalidArgs("rects must be a list of [x0, y0, x1, y1]".into()))?;
        if rects.is_empty() || rects.len() > MAX_ERASE_AREAS {
            return Err(ToolError::InvalidArgs(format!("give between 1 and {MAX_ERASE_AREAS} rects, not {}", rects.len())));
        }
        let info = self.doc(a)?.info.pages.get(page).cloned().ok_or_else(|| ToolError::InvalidArgs(format!("no page {}", page + 1)))?;
        let mut areas = Vec::with_capacity(rects.len());
        for r in rects {
            let v: Vec<f64> = r.as_array().map(|x| x.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let r = <[f64; 4]>::try_from(v).map_err(|_| ToolError::InvalidArgs("each rect must be 4 numbers".into()))?;
            if !r.iter().all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX)) {
                return Err(ToolError::InvalidArgs(format!("the rect {r:?} is not finite")));
            }
            let u = info.view_rect_to_user([r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32]);
            areas.push(u.map(f64::from));
        }
        let mut options = EraseOptions::default();
        if let Some(kinds) = a.get("kinds") {
            let kinds = kinds.as_array().ok_or_else(|| ToolError::InvalidArgs("kinds must be a list".into()))?;
            options = EraseOptions { text: false, images: false, vectors: false, annotations: false, ..options };
            for k in kinds {
                match k.as_str() {
                    Some("text") => options.text = true,
                    Some("images") => options.images = true,
                    Some("vectors") => options.vectors = true,
                    Some("annotations") => options.annotations = true,
                    _ => return Err(ToolError::InvalidArgs(format!("unknown kind {k}; allowed: text, images, vectors, annotations"))),
                }
            }
        }
        options.rule = match a.opt_str("rule")? {
            None | Some("centre") => GlyphRule::Centre,
            Some("overlap") => GlyphRule::Overlap,
            Some(other) => return Err(ToolError::InvalidArgs(format!("unknown rule {other:?}; allowed: centre, overlap"))),
        };
        let found = self.doc(a)?.erase_preview(page, &areas, &options).map_err(ToolError::Failed)?.len();
        let mut out = self.apply(a, Edit::EraseContent { page, areas, options })?;
        out["erased"] = json!(found);
        Ok(out)
    }

    /// `page_cut`: the Knife. Each page is cut at the same places, in points or as fractions of
    /// the page as shown.
    pub(crate) fn page_cut(&mut self, a: &Args) -> Result<Value> {
        let pages = self.pages(a, "pages")?;
        let axis = match a.str("axis")? {
            "vertical" => pdfcraft_engine::CutAxis::Vertical,
            "horizontal" => pdfcraft_engine::CutAxis::Horizontal,
            other => return Err(ToolError::InvalidArgs(format!("unknown axis {other:?}; allowed: vertical, horizontal"))),
        };
        let at: Vec<f64> = a.get("at").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
        let count = a.get("at").and_then(Value::as_array).map_or(0, Vec::len);
        if at.is_empty() || at.len() != count || at.len() >= pdfcraft_engine::MAX_CUT_PARTS {
            return Err(ToolError::InvalidArgs(format!("at must be 1 to {} numbers", pdfcraft_engine::MAX_CUT_PARTS - 1)));
        }
        let fraction = match a.opt_str("units")? {
            None | Some("pt") => false,
            Some("fraction") => true,
            Some(other) => return Err(ToolError::InvalidArgs(format!("unknown units {other:?}; allowed: pt, fraction"))),
        };
        if fraction && !at.iter().all(|v| *v > 0.0 && *v < 1.0) {
            return Err(ToolError::InvalidArgs("fractions must lie between 0 and 1".into()));
        }
        let info = self.doc(a)?.info.clone();
        let cuts: Vec<pdfcraft_engine::Cut> = pages
            .iter()
            .map(|&page| {
                let p = &info.pages[page];
                // The page as shown, in points.
                let extent = f64::from(if axis == pdfcraft_engine::CutAxis::Vertical { p.width } else { p.height });
                let at = if fraction { at.iter().map(|f| f * extent).collect() } else { at.clone() };
                pdfcraft_engine::Cut { page, axis, at }
            })
            .collect();
        let before = info.pages.len();
        let right_to_left = a.opt_bool("right_to_left")?.unwrap_or(false);
        let trim_outside = a.opt_bool("trim_outside")?.unwrap_or(false);
        let mut out = self.apply(a, Edit::CutPages { cuts: cuts.clone(), right_to_left, trim_outside })?;
        // Where each page's parts went (1-based, in the order given).
        let mut sorted: Vec<usize> = pages.clone();
        sorted.sort_unstable();
        let mut offset = 0usize;
        let mut parts = Vec::new();
        for page in sorted {
            let n = cuts.iter().find(|c| c.page == page).map_or(1, |c| c.at.len() + 1);
            parts.push(json!({ "page": page + 1, "parts": (0..n).map(|k| page + offset + k + 1).collect::<Vec<_>>() }));
            offset += n - 1;
        }
        out["cut"] = json!(parts);
        out["pages_before"] = json!(before);
        Ok(out)
    }

    pub(crate) fn redact_clear(&mut self, a: &Args) -> Result<Value> {
        if self.doc(a)?.redaction_marks() == 0 {
            return Err(failed("there are no redaction marks"));
        }
        self.apply(a, Edit::ClearRedactions)
    }

    pub(crate) fn doc_hidden_info(&self, a: &Args) -> Result<Value> {
        let doc = self.doc(a)?;
        let items: Vec<Value> = doc.hidden_info().into_iter().map(|(h, n)| json!({ "category": h.id(), "label": h.label(), "count": n })).collect();
        let total: usize = doc.hidden_info().iter().map(|c| c.1).sum();
        Ok(json!({ "total": total, "categories": items }))
    }

    pub(crate) fn doc_audit_redactions(&self, a: &Args) -> Result<Value> {
        let d = self.doc(a)?;
        let findings = d.audit_redactions().map_err(ToolError::Failed)?;
        let items: Vec<Value> = findings
            .into_iter()
            .map(|f| {
                json!({
                    "page": f.page + 1,
                    "kind": f.kind.id(),
                    // Points from the top-left of the displayed page, like `comment_list` and `doc_info`.
                    "rect": crate::view_rect(&d.info, f.page, f.rect.map(|v| v as f32)),
                    "covered_text": f.covered_text,
                })
            })
            .collect();
        Ok(json!({ "findings": items, "count": items.len() }))
    }

    pub(crate) fn doc_remove_hidden(&mut self, a: &Args) -> Result<Value> {
        let edit = match a.get("categories") {
            None => Edit::Sanitize,
            Some(_) => {
                let which = a
                    .strs("categories")?
                    .into_iter()
                    .map(|c| Hidden::from_id(c).ok_or_else(|| ToolError::InvalidArgs(format!("unknown category {c:?} (see doc_hidden_info)"))))
                    .collect::<Result<Vec<_>>>()?;
                if which.is_empty() {
                    return Err(ToolError::InvalidArgs("categories is empty".into()));
                }
                Edit::RemoveHidden { which }
            }
        };
        let before: usize = self.doc(a)?.hidden_info().iter().map(|c| c.1).sum();
        let mut out = self.apply(a, edit)?;
        let after: usize = self.doc(a)?.hidden_info().iter().map(|c| c.1).sum();
        out["removed"] = json!(before.saturating_sub(after));
        Ok(out)
    }
}
