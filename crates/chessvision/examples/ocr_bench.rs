//! Chess OCR benchmark on the Caïssa ground truth (blueprint S13b). Not run in CI.
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example ocr_bench -- \
//!     --gt <ground_truth folder> --glyphs <models folder> [--ocrs <ocrs models folder>] \
//!     [--mode fuse|glyphs|ocrs] [--no-case] [--no-join] [--no-merge] [--no-glued] [--no-capture] \
//!     [--out hyp.jsonl] [--fails N] [--limit N] [--gate]
//! ```
//!
//! Reads every `<name>.png` line crop with its `<name>.gt.txt` truth (and `index.jsonl` for the
//! document and partition of each), reads it with
//!
//! - `glyphs`: [`glyphs::read`] alone;
//! - `ocrs`: the general OCR alone (`text-detection.rten` / `text-recognition.rten`, the models
//!   `pdfcraft-ocr` uses, with its alphabet);
//! - `fuse` (the default, needs `--ocrs`): both, through [`fuse::fuse_line`];
//!
//! and scores it with Caïssa's instrument (`caissa.ocr.metrics`, re-derived in Rust by the S13a
//! spike and checked number for number against the Python): NFKC and folds, weighted CER, and
//! SAN move tokens kept as a multiset intersection per line. `--gate` exits non-zero unless the
//! whole set reaches CER ≤ 2.12% and moves ≥ 0.95.
//!
//! The scorer is a copy of the spike's, kept apart from the crate's own notation rules on
//! purpose: the instrument must not move when the reader changes. The line crops and the models
//! are the user's own and stay outside the repository; nothing derived from them is written
//! anywhere but `--out`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pdfcraft_chessvision::coords::PixelRect;
use pdfcraft_chessvision::glyphs::fuse::{OcrWord, fuse_line, glyphs_only};
use pdfcraft_chessvision::glyphs::{self, GlyphOptions};
use pdfcraft_chessvision::models::{ChessModels, GlyphModel, ModelState};
use pdfcraft_chessvision::raster::GrayImage;

type Error = Box<dyn std::error::Error>;

/// The gates of blueprint S13b.
const MAX_CER: f64 = 0.0212;
const MIN_MOVES: f64 = 0.95;
/// `pdfcraft-ocr`'s model files and alphabet (ASCII and €).
const DETECTION_MODEL: &str = "text-detection.rten";
const RECOGNITION_MODEL: &str = "text-recognition.rten";
const ALPHABET: &str = " 0123456789!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~€ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

struct Args {
    kv: BTreeMap<String, String>,
    flags: Vec<String>,
}

impl Args {
    fn parse() -> Args {
        let rest: Vec<String> = std::env::args().skip(1).collect();
        let (mut kv, mut flags) = (BTreeMap::new(), Vec::new());
        let mut i = 0;
        while i < rest.len() {
            let a = rest[i].trim_start_matches('-').to_owned();
            let valued = matches!(a.as_str(), "gt" | "glyphs" | "ocrs" | "mode" | "out" | "fails" | "limit");
            if valued && i + 1 < rest.len() {
                kv.insert(a, rest[i + 1].clone());
                i += 2;
            } else {
                flags.push(a);
                i += 1;
            }
        }
        Args { kv, flags }
    }
    fn path(&self, k: &str) -> Result<PathBuf, Error> {
        self.kv.get(k).map(PathBuf::from).ok_or_else(|| format!("missing --{k}").into())
    }
    fn flag(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }
}

fn load_png(path: &Path) -> Result<GrayImage, Error> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(std::fs::read(path)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf)?;
    let bytes = buf.get(..info.buffer_size()).ok_or("short frame")?;
    let luma = |r: u8, g: u8, b: u8| ((u32::from(r) * 4899 + u32::from(g) * 9617 + u32::from(b) * 1868 + (1 << 13)) >> 14) as u8;
    let data: Vec<u8> = match info.color_type {
        png::ColorType::Grayscale => bytes.to_vec(),
        png::ColorType::GrayscaleAlpha => bytes.as_chunks::<2>().0.iter().map(|&[g, _]| g).collect(),
        png::ColorType::Rgb => bytes.as_chunks::<3>().0.iter().map(|&[r, g, b]| luma(r, g, b)).collect(),
        png::ColorType::Rgba => bytes.as_chunks::<4>().0.iter().map(|&[r, g, b, _]| luma(r, g, b)).collect(),
        png::ColorType::Indexed => return Err("palette not expanded".into()),
    };
    Ok(GrayImage::new(info.width, info.height, data)?)
}

fn ocrs_engine(dir: &Path) -> Result<ocrs::OcrEngine, Error> {
    let load = |f: &str| rten::Model::load_file(dir.join(f)).map_err(|e| format!("{f}: {e}"));
    Ok(ocrs::OcrEngine::new(ocrs::OcrEngineParams {
        detection_model: Some(load(DETECTION_MODEL)?),
        recognition_model: Some(load(RECOGNITION_MODEL)?),
        alphabet: Some(ALPHABET.into()),
        ..Default::default()
    })?)
}

/// The general OCR's words, with their boxes, in reading order.
fn ocrs_words(engine: &ocrs::OcrEngine, image: &GrayImage) -> Result<Vec<OcrWord>, Error> {
    use ocrs::TextItem;
    let src = ocrs::ImageSource::from_bytes(image.data(), (image.width(), image.height()))?;
    let input = engine.prepare_input(src)?;
    let found = engine.detect_words(&input)?;
    let lines = engine.find_text_lines(&input, &found);
    let read = engine.recognize_text(&input, &lines)?;
    let px = |v: i32| u32::try_from(v.max(0)).unwrap_or(0);
    let mut out = Vec::new();
    for line in read.into_iter().flatten() {
        for w in line.words() {
            let text = w.to_string();
            if text.trim().is_empty() {
                continue;
            }
            let r = w.bounding_rect();
            out.push(OcrWord { text, rect: PixelRect { x0: px(r.left()), y0: px(r.top()), x1: px(r.right()), y1: px(r.bottom()) } });
        }
    }
    Ok(out)
}

fn glyph_model(dir: &Path) -> Result<GlyphModel, Error> {
    match ChessModels::load_from(dir).glyphs {
        ModelState::Loaded(m) => Ok(m),
        ModelState::Missing => Err(format!("no chess-glyphs.onnx in {}", dir.display()).into()),
        ModelState::Invalid(e) => Err(e.into()),
    }
}

fn main() -> Result<(), Error> {
    let a = Args::parse();
    let gt = a.path("gt")?;
    let mode = a.kv.get("mode").map_or("fuse", String::as_str).to_owned();
    let limit: Option<usize> = a.kv.get("limit").and_then(|v| v.parse().ok());
    let fails: usize = a.kv.get("fails").and_then(|v| v.parse().ok()).unwrap_or(0);
    let options = GlyphOptions {
        split_glued: !a.flag("no-glued"),
        merge_broken: !a.flag("no-merge"),
        case_by_height: !a.flag("no-case"),
        join_move_numbers: !a.flag("no-join"),
        capture_x: !a.flag("no-capture"),
    };
    let model = if mode == "ocrs" { None } else { Some(glyph_model(&a.path("glyphs")?)?) };
    let engine = if mode == "glyphs" { None } else { Some(ocrs_engine(&a.path("ocrs")?)?) };
    if !matches!(mode.as_str(), "fuse" | "glyphs" | "ocrs") {
        return Err(format!("unknown mode {mode}").into());
    }

    let mut meta: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Ok(index) = std::fs::read_to_string(gt.join("index.jsonl")) {
        for line in index.lines() {
            let v: serde_json::Value = serde_json::from_str(line)?;
            let doc = v["document"].as_str().unwrap_or("").chars().take(12).collect::<String>();
            meta.insert(v["name"].as_str().unwrap_or("").to_owned(), (doc, v["partition"].as_str().unwrap_or("").to_owned()));
        }
    }
    let mut names: Vec<String> = std::fs::read_dir(&gt)?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".png")).map(str::to_owned))
        .collect();
    names.sort();
    if let Some(l) = limit {
        names.truncate(l);
    }
    let mut out = match a.kv.get("out") {
        Some(p) => Some(std::fs::File::create(p)?),
        None => None,
    };
    let mut groups: BTreeMap<String, score::Tally> = BTreeMap::new();
    let mut shown = 0;
    let mut warmed = false;
    for name in &names {
        let image = load_png(&gt.join(format!("{name}.png")))?;
        let truth = std::fs::read_to_string(gt.join(format!("{name}.gt.txt")))?.trim().to_owned();
        if !warmed {
            // Load both models' kernels once so the first line does not carry the start-up.
            if let Some(m) = &model {
                let _ = glyphs::read(&image, None, None, m, &options);
            }
            if let Some(e) = &engine {
                let _ = ocrs_words(e, &image);
            }
            warmed = true;
        }
        let t0 = Instant::now();
        let lines = match &model {
            Some(m) => glyphs::read(&image, None, None, m, &options)?,
            None => Vec::new(),
        };
        let ocr = match &engine {
            Some(e) => ocrs_words(e, &image)?,
            None => Vec::new(),
        };
        let hyp = match mode.as_str() {
            "glyphs" => glyphs_only(&lines).iter().map(|l| l.text()).collect::<Vec<_>>().join(" "),
            "ocrs" => ocr.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
            _ => fuse_line(&ocr, &lines).text(),
        };
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if let Some(f) = &mut out {
            writeln!(f, "{}", serde_json::json!({"name": name, "truth": truth, "hyp": hyp, "ms": ms}))?;
        }
        let (doc, part) = meta.get(name).cloned().unwrap_or_default();
        for key in ["all".to_owned(), format!("doc:{doc}"), format!("part:{part}")] {
            groups.entry(key).or_default().add(&truth, &hyp, ms);
        }
        if shown < fails && score::normalise(&truth) != score::normalise(&hyp) {
            shown += 1;
            println!("{name}\n  T: {truth}\n  H: {hyp}");
        }
    }
    println!(
        "| group | lines | CER | CER without spaces | figurine CER (err/truth) | NAG-symbol CER (err/truth) | moves kept | moves invented | exact lines | figurines invented | ms/line |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (k, t) in &groups {
        println!("{}", t.row(k));
    }
    let all = groups.get("all").cloned().unwrap_or_default();
    let cer = all.edits as f64 / all.chars.max(1) as f64;
    let moves = all.mv_kept as f64 / all.mv_truth.max(1) as f64;
    let pass = cer <= MAX_CER && moves >= MIN_MOVES;
    println!(
        "gate (CER <= {:.2}%, moves >= {MIN_MOVES}): CER {:.2}%, moves {moves:.4} -> {}",
        MAX_CER * 100.0,
        cer * 100.0,
        if pass { "PASS" } else { "FAIL" }
    );
    if a.flag("gate") && !pass {
        std::process::exit(1);
    }
    Ok(())
}

/// The Caïssa scoring instrument (`caissa.ocr.metrics`), as the S13a spike re-derived it:
/// `normalise`, character edit distance, and multiset accounting of SAN move tokens.
mod score {
    use unicode_normalization::UnicodeNormalization;

    const FOLD: &[(char, &str)] = &[
        ('\u{ad}', "-"),
        ('‐', "-"),
        ('‑', "-"),
        ('‒', "-"),
        ('–', "-"),
        ('—', "-"),
        ('−', "-"),
        ('‘', "'"),
        ('’', "'"),
        ('‚', "'"),
        ('“', "\""),
        ('”', "\""),
        ('„', "\""),
        ('…', "..."),
        ('×', "x"),
    ];

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn replace_castle(chars: &[char], pat: &str, rep: &str) -> Vec<char> {
        let p: Vec<char> = pat.chars().collect();
        let mut out = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let fits = chars.get(i..i + p.len()).is_some_and(|s| s == p.as_slice());
            let before_ok = i == 0 || !is_word(chars[i - 1]);
            let after_ok = chars.get(i + p.len()).is_none_or(|&c| !is_word(c));
            if fits && before_ok && after_ok {
                out.extend(rep.chars());
                i += p.len();
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    /// `caissa.ocr.metrics.normalise`.
    pub fn normalise(text: &str) -> String {
        let mut s: String = text.nfkc().collect();
        for (from, to) in FOLD {
            if s.contains(*from) {
                s = s.replace(*from, to);
            }
        }
        let c: Vec<char> = s.chars().collect();
        let c = replace_castle(&c, "0-0-0", "O-O-O");
        let c = replace_castle(&c, "0-0", "O-O");
        let s: String = c.into_iter().collect();
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub fn is_figurine(c: char) -> bool {
        ('♔'..='♟').contains(&c)
    }

    fn is_piece(c: char) -> bool {
        "KQRBNPDTCSAFLГЛСКФП".contains(c) || is_figurine(c)
    }

    fn is_promo(c: char) -> bool {
        "QRBNDTCSAFL".contains(c) || is_figurine(c)
    }

    const WORDLIKE_SUFFIX: &[char] = &['²', '³', 'µ', 'μ', 'ʘ', 'ƒ', 'Δ', '¹'];

    fn tail_ok(c: &[char], end: usize) -> bool {
        if let Some(&n) = c.get(end)
            && !WORDLIKE_SUFFIX.contains(&n)
            && is_word(n)
        {
            return false;
        }
        let two = |i: usize| c.get(i).is_some_and(|x| x.is_ascii_alphabetic()) && c.get(i + 1).is_some_and(|x| x.is_ascii_alphabetic());
        !(two(end) || (c.get(end) == Some(&'-') && two(end + 1)))
    }

    fn file(c: Option<&char>) -> bool {
        c.is_some_and(|c| ('a'..='h').contains(c))
    }
    fn rank(c: Option<&char>) -> bool {
        c.is_some_and(|c| ('1'..='8').contains(c))
    }

    fn match_at(c: &[char], i: usize) -> Option<usize> {
        let lit = |at: usize, s: &str| -> Option<usize> {
            let p: Vec<char> = s.chars().collect();
            (c.get(at..at + p.len()) == Some(p.as_slice())).then_some(at + p.len())
        };
        for castle in ["O-O-O", "O-O"] {
            if let Some(e) = lit(i, castle)
                && tail_ok(c, e)
            {
                return Some(e);
            }
        }
        for piece in [true, false] {
            if piece && !c.get(i).is_some_and(|&x| is_piece(x)) {
                continue;
            }
            let p = i + usize::from(piece);
            for f_opt in [true, false] {
                if f_opt && !file(c.get(p)) {
                    continue;
                }
                let q = p + usize::from(f_opt);
                for r_opt in [true, false] {
                    if r_opt && !rank(c.get(q)) {
                        continue;
                    }
                    let r = q + usize::from(r_opt);
                    for x_opt in [true, false] {
                        if x_opt && c.get(r) != Some(&'x') {
                            continue;
                        }
                        let s = r + usize::from(x_opt);
                        if !(file(c.get(s)) && rank(c.get(s + 1))) {
                            continue;
                        }
                        let sq = s + 2;
                        let mut promos = Vec::new();
                        if c.get(sq) == Some(&'=') && c.get(sq + 1).is_some_and(|&x| is_promo(x)) {
                            promos.push(sq + 2);
                        }
                        if c.get(sq).is_some_and(|&x| is_promo(x)) {
                            promos.push(sq + 1);
                        }
                        promos.push(sq);
                        for pe in promos {
                            let mut ends = Vec::new();
                            if c.get(pe).is_some_and(|&x| x == '+' || x == '#') {
                                ends.push(pe + 1);
                            }
                            ends.push(pe);
                            for e in ends {
                                if tail_ok(c, e) {
                                    return Some(e);
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn homoglyph(c: char) -> char {
        match c {
            'К' => 'K',
            'С' => 'C',
            'Р' => 'P',
            'В' => 'B',
            'А' => 'A',
            'Е' => 'E',
            'М' => 'M',
            'Т' => 'T',
            'Н' => 'H',
            'О' => 'O',
            'Х' => 'X',
            o => o,
        }
    }

    /// `caissa.ocr.metrics.move_tokens`.
    pub fn move_tokens(text: &str) -> Vec<String> {
        let c: Vec<char> = normalise(text).chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < c.len() {
            if i > 0 && is_word(c[i - 1]) {
                i += 1;
                continue;
            }
            if let Some(e) = match_at(&c, i) {
                out.push(c[i..e].iter().map(|&x| homoglyph(x)).collect());
                i = e;
            } else {
                i += 1;
            }
        }
        out
    }

    pub fn levenshtein(a: &[char], b: &[char]) -> usize {
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        let mut cur = vec![0; b.len() + 1];
        for (i, ca) in a.iter().enumerate() {
            cur[0] = i + 1;
            for (j, cb) in b.iter().enumerate() {
                cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
            }
            std::mem::swap(&mut prev, &mut cur);
        }
        prev[b.len()]
    }

    pub fn is_nag_symbol(c: char) -> bool {
        "±∓⩲⩱∞□△⇆→↑⇄⨀○⟳⊥⌓Δ∆".contains(c)
    }

    /// Errors touching a class of glyph on an optimal alignment: truth glyphs substituted or
    /// deleted, plus glyphs inserted or substituted for another character.
    pub fn glyph_errors(t: &[char], h: &[char], class: fn(char) -> bool) -> (usize, usize) {
        let (n, m) = (t.len(), h.len());
        let mut d = vec![vec![0usize; m + 1]; n + 1];
        for (i, row) in d.iter_mut().enumerate() {
            row[0] = i;
        }
        for (j, v) in d[0].iter_mut().enumerate() {
            *v = j;
        }
        for i in 1..=n {
            for j in 1..=m {
                d[i][j] = (d[i - 1][j - 1] + usize::from(t[i - 1] != h[j - 1])).min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
            }
        }
        let (mut i, mut j, mut err) = (n, m, 0);
        while i > 0 || j > 0 {
            if i > 0 && j > 0 && d[i][j] == d[i - 1][j - 1] + usize::from(t[i - 1] != h[j - 1]) {
                if t[i - 1] != h[j - 1] && (class(t[i - 1]) || class(h[j - 1])) {
                    err += 1;
                }
                i -= 1;
                j -= 1;
            } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
                if class(t[i - 1]) {
                    err += 1;
                }
                i -= 1;
            } else {
                if class(h[j - 1]) {
                    err += 1;
                }
                j -= 1;
            }
        }
        (err, t.iter().filter(|&&c| class(c)).count())
    }

    #[derive(Default, Clone, Debug)]
    pub struct Tally {
        pub lines: usize,
        pub exact: usize,
        pub edits: usize,
        pub chars: usize,
        pub fig_err: usize,
        pub sym_err: usize,
        pub edits_ns: usize,
        pub chars_ns: usize,
        pub sym_truth: usize,
        pub fig_truth: usize,
        pub fig_inv: usize,
        pub mv_truth: usize,
        pub mv_kept: usize,
        pub mv_inv: usize,
        pub ms: f64,
    }

    fn multiset_kept(a: &[String], b: &[String]) -> usize {
        let mut pool: Vec<&String> = b.iter().collect();
        let mut kept = 0;
        for x in a {
            if let Some(p) = pool.iter().position(|y| *y == x) {
                pool.swap_remove(p);
                kept += 1;
            }
        }
        kept
    }

    impl Tally {
        pub fn add(&mut self, truth: &str, hyp: &str, ms: f64) {
            let t: Vec<char> = normalise(truth).chars().collect();
            let h: Vec<char> = normalise(hyp).chars().collect();
            self.lines += 1;
            self.exact += usize::from(t == h);
            self.edits += levenshtein(&h, &t);
            self.chars += t.len();
            let tn: Vec<char> = t.iter().copied().filter(|c| *c != ' ').collect();
            let hn: Vec<char> = h.iter().copied().filter(|c| *c != ' ').collect();
            self.edits_ns += levenshtein(&hn, &tn);
            self.chars_ns += tn.len();
            let (fe, ft) = glyph_errors(&t, &h, is_figurine);
            self.fig_err += fe;
            self.fig_truth += ft;
            let (se, st) = glyph_errors(&t, &h, is_nag_symbol);
            self.sym_err += se;
            self.sym_truth += st;
            let tf: Vec<String> = t.iter().filter(|c| is_figurine(**c)).map(|c| c.to_string()).collect();
            let hf: Vec<String> = h.iter().filter(|c| is_figurine(**c)).map(|c| c.to_string()).collect();
            self.fig_inv += hf.len() - multiset_kept(&tf, &hf);
            let tm = move_tokens(truth);
            let hm = move_tokens(hyp);
            let k = multiset_kept(&tm, &hm);
            self.mv_truth += tm.len();
            self.mv_kept += k;
            self.mv_inv += hm.len() - k;
            self.ms += ms;
        }

        pub fn row(&self, label: &str) -> String {
            let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
            format!(
                "| {label} | {} | {:.2} % | {:.2} % | {:.2} % ({}/{}) | {:.2} % ({}/{}) | {}/{} ({:.1} %) | {} | {}/{} | {} | {:.1} |",
                self.lines,
                pct(self.edits, self.chars),
                pct(self.edits_ns, self.chars_ns),
                pct(self.fig_err, self.fig_truth),
                self.fig_err,
                self.fig_truth,
                pct(self.sym_err, self.sym_truth),
                self.sym_err,
                self.sym_truth,
                self.mv_kept,
                self.mv_truth,
                pct(self.mv_kept, self.mv_truth),
                self.mv_inv,
                self.exact,
                self.lines,
                self.fig_inv,
                self.ms / self.lines.max(1) as f64,
            )
        }
    }

    #[test]
    fn tokens_like_caissa() {
        assert_eq!(move_tokens("If 37 ♖b7+ there follows not 37...♔f6? 38"), vec!["♖b7+", "♔f6"]);
        assert_eq!(move_tokens("15 0-0?! exd5 Nf3 e8=Q+ g1♕ Bl"), vec!["O-O", "exd5", "Nf3", "e8=Q+", "g1♕"]);
        assert_eq!(move_tokens("the e4-e5 and a4 b4bc"), vec!["e4", "e5", "a4"]);
        assert_eq!(normalise("a  b—c ’"), "a b-c '");
    }
}
