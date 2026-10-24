//! Field benchmark for the contour detector (blueprint S7b). Not run in CI.
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example contour_bench -- \
//!     <field_set.jsonl> <folder with the books> [dpi=220] [--misses]
//!     [--read=<models folder> [--steps=0,d,f,h,c,...] [--dump=<folder>]]
//! ```
//!
//! Reads the prototype's hand-annotated field set (one JSON object per page: `pdf`, 0-based
//! `page`, `reviewed`, `diagrams[].bbox` in displayed-page points, `regime`), renders each
//! reviewed page with `pdfcraft-render`, runs [`contour::candidates`] on it and matches the
//! boxes as the prototype's `field_eval` does: greedily by IoU, at least 0.5, one detection per
//! annotation. Prints recall and precision per regime and in total.
//!
//! The books are the user's own and stay outside the repository; nothing derived from them is
//! written anywhere. This measures the contour path alone: the prototype's numbers were for its
//! hybrid detector (embedded images plus contours).
//!
//! With `--read`, every annotated diagram with a `placement` that a detection matched is also read
//! through [`board::read_with`] from that detection, once per preprocessing variant of `--steps`
//! (letters as [`board::Preprocessing::from_letters`] takes them, comma-separated: `0` is
//! none, `0>h` hatch suppression on a retry), and the
//! exact and accepted readings are counted per regime. `--dump` writes each variant's board, as
//! the squares are cut from it, as a PNG named after the page (for looking at, outside the
//! repository).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use pdfcraft_chess::fen::{format_placement, parse_placement};
use pdfcraft_chessvision::board;
use pdfcraft_chessvision::coords::{PageGeometry, PageRect, Rotation};
use pdfcraft_chessvision::detect::contour;
use pdfcraft_chessvision::models::{ChessModels, ModelState};
use pdfcraft_chessvision::raster::GrayImage;
use pdfcraft_render::{DocInfo, PageRenderer, RenderConfig, RenderRequest};

type Bbox = [f64; 4];

#[derive(Default)]
struct Tally {
    pages: usize,
    annotated: usize,
    detected: usize,
    matched: usize,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.pages += other.pages;
        self.annotated += other.annotated;
        self.detected += other.detected;
        self.matched += other.matched;
    }

    fn line(&self, name: &str) -> String {
        let ratio = |a: usize, b: usize| if b == 0 { f64::NAN } else { a as f64 / b as f64 };
        format!(
            "{name:<16} pages {:>3}  annotated {:>3}  detected {:>3}  matched {:>3}  recall {:.4}  precision {:.4}",
            self.pages,
            self.annotated,
            self.detected,
            self.matched,
            ratio(self.matched, self.annotated),
            ratio(self.matched, self.detected)
        )
    }
}

fn iou(a: &Bbox, b: &Bbox) -> f64 {
    let iw = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let ih = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = iw * ih;
    let union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter;
    if inter <= 0.0 || union <= 0.0 { 0.0 } else { inter / union }
}

/// Readings of the labelled diagrams under one preprocessing variant.
#[derive(Default)]
struct ReadTally {
    labelled: usize,
    detected: usize,
    exact: usize,
    accepted: usize,
    accepted_wrong: usize,
    confidence: f64,
}

impl ReadTally {
    fn add(&mut self, other: &ReadTally) {
        self.labelled += other.labelled;
        self.detected += other.detected;
        self.exact += other.exact;
        self.accepted += other.accepted;
        self.accepted_wrong += other.accepted_wrong;
        self.confidence += other.confidence;
    }

    fn line(&self, name: &str) -> String {
        format!(
            "{name:<28} labelled {:>3}  detected {:>3}  exact {:>3}  accepted {:>3}  accepted but wrong {:>2}  mean min confidence {:.3}",
            self.labelled,
            self.detected,
            self.exact,
            self.accepted,
            self.accepted_wrong,
            self.confidence / self.detected.max(1) as f64
        )
    }
}

/// A gray image written as an 8-bit grayscale PNG.
fn write_png(path: &Path, image: &GrayImage) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.width(), image.height());
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(image.data())?;
    Ok(())
}

/// Greedy matching by IoU, as `field_eval._match`: the number of annotations matched.
fn matched(annotated: &[Bbox], found: &[Bbox]) -> usize {
    pairs(annotated, found).len()
}

/// Greedy matching by IoU: `(annotation, detection)` index pairs.
fn pairs(annotated: &[Bbox], found: &[Bbox]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(f64, usize, usize)> =
        annotated.iter().enumerate().flat_map(|(i, a)| found.iter().enumerate().map(move |(j, f)| (iou(a, f), i, j))).collect();
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (mut used_a, mut used_f) = (vec![false; annotated.len()], vec![false; found.len()]);
    let mut out = Vec::new();
    for (score, i, j) in pairs {
        if score < 0.5 || used_a[i] || used_f[j] {
            continue;
        }
        used_a[i] = true;
        used_f[j] = true;
        out.push((i, j));
    }
    out
}

/// Premultiplied RGBA over white, to gray with the BT.601 weights OpenCV's `RGB2GRAY` uses.
fn to_gray(width: u32, height: u32, rgba: &[u8]) -> Option<GrayImage> {
    let data = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| {
            let over_white = |c: u8| u32::from(c) + (255 - u32::from(a));
            let (r, g, b) = (over_white(r), over_white(g), over_white(b));
            ((299 * r + 587 * g + 114 * b + 500) / 1000).min(255) as u8
        })
        .collect();
    GrayImage::new(width, height, data).ok()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: contour_bench <field_set.jsonl> <pdf folder> [dpi] [--misses]";
    let field = args.first().ok_or(usage)?;
    let folder = Path::new(args.get(1).ok_or(usage)?);
    let dpi: f32 = args.get(2).filter(|a| !a.starts_with("--")).map_or(Ok(220.0), |a| a.parse())?;
    let show_misses = args.iter().any(|a| a == "--misses");
    let option = |name: &str| args.iter().find_map(|a| a.strip_prefix(name)).map(str::to_owned);
    let models = option("--read=").map(|dir| ChessModels::load_from(Path::new(&dir)));
    let model = match &models {
        None => None,
        Some(m) => match &m.pieces {
            ModelState::Loaded(model) => Some(model),
            ModelState::Missing => return Err("no chess-pieces.onnx in the models folder".into()),
            ModelState::Invalid(why) => return Err(why.clone().into()),
        },
    };
    let variants: Vec<(String, board::Preprocessing)> = option("--steps=")
        .unwrap_or_else(|| "0".to_owned())
        .split(',')
        .map(|v| board::Preprocessing::from_letters(v).map(|s| (v.to_owned(), s)).ok_or_else(|| format!("unknown steps {v:?}")))
        .collect::<Result<_, _>>()?;
    let dump = option("--dump=").map(std::path::PathBuf::from);
    let mut reads: BTreeMap<(String, String), ReadTally> = BTreeMap::new();

    let mut documents: HashMap<String, (PageRenderer, DocInfo)> = HashMap::new();
    let mut by_regime: BTreeMap<String, Tally> = BTreeMap::new();
    let mut seconds = 0.0_f64;
    for line in std::fs::read_to_string(field)?.lines().filter(|l| !l.trim().is_empty()) {
        let page: serde_json::Value = serde_json::from_str(line)?;
        if !page["reviewed"].as_bool().unwrap_or(false) {
            continue;
        }
        let pdf = page["pdf"].as_str().ok_or("page without pdf")?.to_string();
        let index = page["page"].as_u64().ok_or("page without index")? as usize;
        let regime = page["regime"].as_str().unwrap_or("?").to_string();
        let annotated: Vec<Bbox> = page["diagrams"]
            .as_array()
            .map(|d| d.iter().filter_map(|x| serde_json::from_value::<Bbox>(x["bbox"].clone()).ok()).collect())
            .unwrap_or_default();
        // The labelled placement of each annotation, in the same order.
        let labels: Vec<Option<String>> = page["diagrams"]
            .as_array()
            .map(|d| {
                d.iter()
                    .filter(|x| serde_json::from_value::<Bbox>(x["bbox"].clone()).is_ok())
                    .map(|x| x["placement"].as_str().and_then(|p| parse_placement(p).ok()).map(|p| format_placement(&p)))
                    .collect()
            })
            .unwrap_or_default();

        if !documents.contains_key(&pdf) {
            let bytes = Arc::new(std::fs::read(folder.join(&pdf))?);
            let info = pdfcraft_render::inspect(bytes.clone(), None).map_err(|e| format!("{pdf}: {e:?}"))?;
            documents.insert(pdf.clone(), (PageRenderer::new(bytes, RenderConfig::default()), info));
        }
        let (renderer, info) = documents.get_mut(&pdf).ok_or("document cache")?;
        let page_info = info.pages.get(index).ok_or_else(|| format!("{pdf}: no page {index}"))?.clone();
        let [x0, y0, x1, y1] = page_info.crop;
        let geometry =
            PageGeometry::new(PageRect::new(x0, y0, x1, y1), Rotation::from_degrees(i64::from(page_info.rotation))).ok_or("degenerate page")?;

        let rendered = renderer.render(RenderRequest { page: index, scale: dpi / 72.0, ..Default::default() });
        if let Some(error) = rendered.error {
            return Err(format!("{pdf} page {index}: {error}").into());
        }
        let gray = to_gray(rendered.width, rendered.height, &rendered.rgba).ok_or("raster size")?;
        // The DPI actually rendered at: the renderer may cap the scale.
        let actual_dpi = rendered.width as f32 / page_info.width * 72.0;

        let start = Instant::now();
        let found = contour::candidates(&gray, &geometry, actual_dpi);
        seconds += start.elapsed().as_secs_f64();
        let boxes: Vec<Bbox> = found
            .iter()
            .filter_map(|c| geometry.user_to_view(c.bbox_user))
            .map(|v| [f64::from(v.x0), f64::from(v.y0), f64::from(v.x1), f64::from(v.y1)])
            .collect();

        let hits = matched(&annotated, &boxes);
        if let Some(model) = model {
            let matches = pairs(&annotated, &boxes);
            for (name, steps) in &variants {
                let tally = reads.entry((name.clone(), regime.clone())).or_default();
                for (i, label) in labels.iter().enumerate() {
                    let Some(label) = label else { continue };
                    tally.labelled += 1;
                    let Some(candidate) = matches.iter().find(|(a, _)| *a == i).and_then(|&(_, j)| found.get(j)) else { continue };
                    tally.detected += 1;
                    let reading = board::read_with(&gray, &geometry, actual_dpi, std::slice::from_ref(candidate), Some(model), *steps).pop();
                    let Some(position) = reading.as_ref().and_then(board::BoardReading::position) else { continue };
                    let exact = format_placement(&position.placement) == *label;
                    tally.exact += usize::from(exact);
                    tally.accepted += usize::from(position.accepted);
                    tally.accepted_wrong += usize::from(position.accepted && !exact);
                    tally.confidence += f64::from(position.min_confidence);
                    if show_misses && !exact {
                        println!(
                            "  [{name}] {pdf} p{index} #{i}: label {label}  read {}  min {:.3}",
                            format_placement(&position.placement),
                            position.min_confidence
                        );
                    }
                    if let Some(dir) = &dump
                        && let Ok(image) = board::board_image(&gray, &geometry, actual_dpi, candidate)
                    {
                        let stem: String = pdf.chars().filter(char::is_ascii_alphanumeric).take(24).collect();
                        let file = format!("{stem}_p{index}_{i}_{}_{}.png", name.replace('>', "-"), if exact { "ok" } else { "miss" });
                        let first = board::preprocess(&image, steps.first);
                        write_png(&dir.join(&file), &first)?;
                        let retried = board::preprocess(&first, steps.retry);
                        if retried != first {
                            write_png(&dir.join(file.replace(".png", "_retry.png")), &retried)?;
                        }
                    }
                }
            }
        }
        let tally = by_regime.entry(regime.clone()).or_default();
        tally.pages += 1;
        tally.annotated += annotated.len();
        tally.detected += boxes.len();
        tally.matched += hits;
        if show_misses && (hits < annotated.len() || hits < boxes.len()) {
            println!("{pdf} p{index} [{regime}]: annotated {annotated:?}");
            println!("    found {:?}", boxes.iter().map(|b| b.map(|v| (v * 10.0).round() / 10.0)).collect::<Vec<_>>());
        }
    }

    let mut total = Tally::default();
    for (regime, tally) in &by_regime {
        println!("{}", tally.line(regime));
        total.add(tally);
    }
    println!("{}", total.line("total"));
    if model.is_some() {
        for (name, _) in &variants {
            let mut total = ReadTally::default();
            for ((variant, regime), tally) in &reads {
                if variant == name {
                    println!("{}", tally.line(&format!("read [{name}] {regime}")));
                    total.add(tally);
                }
            }
            println!("{}", total.line(&format!("read [{name}] total")));
        }
    }
    println!("dpi {dpi}, detector time {:.2} s ({:.0} ms per page)", seconds, 1000.0 * seconds / total.pages.max(1) as f64);
    Ok(())
}
