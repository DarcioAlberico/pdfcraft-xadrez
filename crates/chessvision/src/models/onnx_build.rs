//! A writer of the ONNX protobuf fields synthetic models need (no weights are committed,
//! decision D2), and the hostile models built from it.
//!
//! The file has no dependency, so it is included twice: by this crate's tests (`onnx::build`) and
//! by `cargo xtask fuzz`, which writes [`hostile_models`] into model folders and runs the fuzzing
//! child on them (plan A9-Q3a). Mutating the bytes of a model almost never makes a valid graph
//! that asks for too much: the 4,600 mutations of A3 could not find R7-01, a valid 1.3 MB model
//! whose `ConstantOfShape` aborted the process. These graphs are built to ask.

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn bytes(out: &mut Vec<u8>, field: u64, b: &[u8]) {
    varint(out, field << 3 | 2);
    varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn int(out: &mut Vec<u8>, field: u64, v: i64) {
    varint(out, field << 3);
    varint(out, v as u64);
}

/// A `TensorProto` of `data_type` (1 float, 7 int64) holding `raw`.
pub fn tensor(name: &str, dims: &[i64], data_type: i64, raw: &[u8]) -> Vec<u8> {
    let mut t = Vec::new();
    for d in dims {
        int(&mut t, 1, *d);
    }
    int(&mut t, 2, data_type);
    bytes(&mut t, 8, name.as_bytes());
    bytes(&mut t, 9, raw);
    t
}

pub fn floats(name: &str, dims: &[i64], values: &[f32]) -> Vec<u8> {
    tensor(name, dims, 1, &values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>())
}

pub fn int64s(name: &str, values: &[i64]) -> Vec<u8> {
    tensor(name, &[values.len() as i64], 7, &values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>())
}

/// A 64-bit integer scalar (no dimensions).
pub fn int64_scalar(name: &str, value: i64) -> Vec<u8> {
    tensor(name, &[], 7, &value.to_le_bytes())
}

/// An attribute of a node.
pub enum Attr<'a> {
    Int(&'a str, i64),
    Ints(&'a str, &'a [i64]),
    Tensor(&'a str, Vec<u8>),
    /// A subgraph (`If`'s `then_branch`), empty.
    Graph(&'a str),
}

pub fn node(op: &str, inputs: &[&str], outputs: &[&str], attrs: &[Attr]) -> Vec<u8> {
    let mut n = Vec::new();
    for i in inputs {
        bytes(&mut n, 1, i.as_bytes());
    }
    for o in outputs {
        bytes(&mut n, 2, o.as_bytes());
    }
    bytes(&mut n, 4, op.as_bytes());
    for attr in attrs {
        let mut a = Vec::new();
        match attr {
            Attr::Int(name, v) => {
                bytes(&mut a, 1, name.as_bytes());
                int(&mut a, 20, 2);
                int(&mut a, 3, *v);
            }
            Attr::Ints(name, vs) => {
                bytes(&mut a, 1, name.as_bytes());
                int(&mut a, 20, 7);
                for v in *vs {
                    int(&mut a, 8, *v);
                }
            }
            Attr::Tensor(name, t) => {
                bytes(&mut a, 1, name.as_bytes());
                int(&mut a, 20, 4);
                bytes(&mut a, 5, t);
            }
            Attr::Graph(name) => {
                bytes(&mut a, 1, name.as_bytes());
                int(&mut a, 20, 5);
                bytes(&mut a, 6, &[]);
            }
        }
        bytes(&mut n, 5, &a);
    }
    n
}

/// A `ValueInfoProto` for a float tensor; `None` is a free dimension named `batch`.
pub fn value(name: &str, dims: &[Option<i64>]) -> Vec<u8> {
    let mut shape = Vec::new();
    for d in dims {
        let mut dim = Vec::new();
        match d {
            Some(v) => int(&mut dim, 1, *v),
            None => bytes(&mut dim, 2, b"batch"),
        }
        bytes(&mut shape, 1, &dim);
    }
    let mut tensor_type = Vec::new();
    int(&mut tensor_type, 1, 1);
    bytes(&mut tensor_type, 2, &shape);
    let mut ty = Vec::new();
    bytes(&mut ty, 1, &tensor_type);
    let mut v = Vec::new();
    bytes(&mut v, 1, name.as_bytes());
    bytes(&mut v, 2, &ty);
    v
}

/// A `ModelProto` (IR 7, opset 13) whose graph is `nodes` on `initializers`, from `input` to
/// `output`.
pub fn model(nodes: &[Vec<u8>], initializers: &[Vec<u8>], input: Vec<u8>, output: Vec<u8>) -> Vec<u8> {
    let mut g = Vec::new();
    for n in nodes {
        bytes(&mut g, 1, n);
    }
    bytes(&mut g, 2, b"test");
    for t in initializers {
        bytes(&mut g, 5, t);
    }
    bytes(&mut g, 11, &input);
    bytes(&mut g, 12, &output);
    let mut opset = Vec::new();
    bytes(&mut opset, 1, b"");
    int(&mut opset, 2, 13);
    let mut m = Vec::new();
    int(&mut m, 1, 7);
    bytes(&mut m, 8, &opset);
    bytes(&mut m, 7, &g);
    m
}

/// A glyph classifier with `classes` outputs: the 32 × 32 cell flattened, then one dense
/// layer of fixed weights.
pub fn glyph_model(classes: usize) -> Vec<u8> {
    let k = 32 * 32;
    let weights = glyph_weights(classes);
    let bias: Vec<f32> = (0..classes).map(|c| c as f32 / 100.0).collect();
    model(
        &[node("Flatten", &["glyphs"], &["flat"], &[Attr::Int("axis", 1)]), node("Gemm", &["flat", "W", "B"], &["logits"], &[])],
        &[floats("W", &[k as i64, classes as i64], &weights), floats("B", &[classes as i64], &bias)],
        value("glyphs", &[None, Some(1), Some(32), Some(32)]),
        value("logits", &[None, Some(classes as i64)]),
    )
}

/// The dense layer's weights of [`glyph_model`], `1024 × classes`, row-major.
pub fn glyph_weights(classes: usize) -> Vec<f32> {
    (0..32 * 32 * classes).map(|i| ((i * 37 % 101) as f32 - 50.0) / 5000.0).collect()
}

/// The description of [`glyph_model`].
pub fn glyph_meta(classes: usize) -> String {
    let names: Vec<String> = (0..classes).map(|c| format!("\"g{c}\"")).collect();
    format!(r#"{{"kind":"chess-glyphs","input":{{"name":"glyphs"}},"output":{{"name":"logits","classes":[{}]}}}}"#, names.join(","))
}

/// Nodes in [`hostile_models`]' long chain: under the checker's cap on what it reads (three items
/// a node: itself, its input and its output), so the chain reaches rten.
pub const CHAIN_NODES: usize = 60_000;

/// Models that feed `input` (`[batch, 1, side, side]`) and say they give `logits`
/// (`[batch, classes]`), each built to break the loader or the run: a tensor too large to
/// allocate made by each operator that can make one from a few bytes (`ConstantOfShape`,
/// `Expand`, `Tile`, `Range`, `Resize`, `Pad`, two constants broadcast, a convolution's padding,
/// a reshape), weights whose stated size their bytes do not have, attributes out of range, a
/// chain of [`CHAIN_NODES`] nodes, a cycle, an output nobody makes or two nodes make, NaN weights
/// and an output of the wrong shape. Each is named for the fuzzer's findings and the tests.
pub fn hostile_models(input: &str, side: i64, classes: i64) -> Vec<(&'static str, Vec<u8>)> {
    let k = side * side;
    let x = || value(input, &[None, Some(1), Some(side), Some(side)]);
    let y = || value("logits", &[None, Some(classes)]);
    let flat = || node("Flatten", &[input], &["flat"], &[Attr::Int("axis", 1)]);
    let w = |out: i64, v: f32| floats("W", &[k, out], &vec![v; usize::try_from(k * out).unwrap_or(0)]);
    // The classifier, then `logits = l0 + big`, `big` made by `nodes` from `inits`.
    let tail = |nodes: Vec<Vec<u8>>, inits: Vec<Vec<u8>>| {
        let mut all = vec![flat(), node("Gemm", &["flat", "W"], &["l0"], &[])];
        all.extend(nodes);
        all.push(node("Add", &["l0", "big"], &["logits"], &[]));
        let mut weights = vec![w(classes, 0.0)];
        weights.extend(inits);
        model(&all, &weights, x(), y())
    };
    let huge = 1i64 << 20;
    let chain: Vec<Vec<u8>> = (0..CHAIN_NODES)
        .map(|i| {
            let from = if i == 0 { "l0".to_owned() } else { format!("c{i}") };
            let to = if i + 1 == CHAIN_NODES { "big".to_owned() } else { format!("c{}", i + 1) };
            node("Identity", &[&from], &[&to], &[])
        })
        .collect();
    vec![
        ("constant-of-shape", tail(vec![node("ConstantOfShape", &["shape"], &["big"], &[])], vec![int64s("shape", &[65536, 65536, 64])])),
        ("expand", tail(vec![node("Expand", &["l0", "shape"], &["big"], &[])], vec![int64s("shape", &[65536, 65536, classes])])),
        ("tile", tail(vec![node("Tile", &["l0", "reps"], &["big"], &[])], vec![int64s("reps", &[65536, 65536])])),
        (
            "range",
            tail(
                vec![node("Range", &["start", "limit", "delta"], &["big"], &[])],
                vec![int64_scalar("start", 0), int64_scalar("limit", 1 << 40), int64_scalar("delta", 1)],
            ),
        ),
        ("resize", tail(vec![node("Resize", &[input, "", "scales"], &["big"], &[])], vec![floats("scales", &[4], &[1.0, 1.0, 65536.0, 65536.0])])),
        ("pad", tail(vec![node("Pad", &[input, "pads"], &["big"], &[])], vec![int64s("pads", &[0, 0, huge, huge, 0, 0, huge, huge])])),
        ("broadcast", tail(vec![node("Add", &["A", "B"], &["big"], &[])], vec![floats("A", &[524_288, 1], &[]), floats("B", &[1, 524_288], &[])])),
        (
            "conv-padding",
            model(
                &[node("Conv", &[input, "K"], &["logits"], &[Attr::Ints("pads", &[huge, huge, huge, huge])])],
                &[floats("K", &[1, 1, 1, 1], &[1.0])],
                x(),
                y(),
            ),
        ),
        ("conv-kernel", model(&[node("Conv", &[input, "K"], &["logits"], &[])], &[tensor("K", &[1, 1, huge, huge], 1, &[0; 4])], x(), y())),
        (
            "reshape",
            tail(
                vec![
                    node("Constant", &[], &["shape"], &[Attr::Tensor("value", int64s("v", &[65536, 65536]))]),
                    node("Reshape", &["l0", "shape"], &["big"], &[]),
                ],
                vec![],
            ),
        ),
        ("weights-short", model(&[flat(), node("Gemm", &["flat", "W"], &["logits"], &[])], &[tensor("W", &[k, 1 << 31], 1, &[0; 8])], x(), y())),
        (
            "pool-zero",
            model(&[node("MaxPool", &[input], &["logits"], &[Attr::Ints("kernel_shape", &[0, 0]), Attr::Ints("strides", &[0, 0])])], &[], x(), y()),
        ),
        ("flatten-axis", model(&[node("Flatten", &[input], &["logits"], &[Attr::Int("axis", 99)])], &[], x(), y())),
        ("softmax-axis", tail(vec![node("Softmax", &["l0"], &["big"], &[Attr::Int("axis", -99)])], vec![])),
        ("chain", tail(chain, vec![])),
        ("cycle", tail(vec![node("Relu", &["b"], &["a"], &[]), node("Relu", &["a"], &["b"], &[]), node("Identity", &["a"], &["big"], &[])], vec![])),
        ("no-output", model(&[flat(), node("Gemm", &["flat", "W"], &["l0"], &[])], &[w(classes, 0.0)], x(), y())),
        (
            "two-outputs",
            model(&[flat(), node("Gemm", &["flat", "W"], &["logits"], &[]), node("Relu", &["flat"], &["logits"], &[])], &[w(classes, 0.0)], x(), y()),
        ),
        ("nan", model(&[flat(), node("Gemm", &["flat", "W"], &["logits"], &[])], &[w(classes, f32::NAN)], x(), y())),
        ("wrong-output", model(&[flat(), node("Gemm", &["flat", "W"], &["logits"], &[])], &[w(classes + 7, 0.5)], x(), y())),
        (
            "batch-norm",
            model(
                &[node("BatchNormalization", &[input, "s", "b", "m", "v"], &["logits"], &[])],
                &[floats("s", &[huge], &[]), floats("b", &[1], &[0.0]), floats("m", &[3], &[0.0; 3]), floats("v", &[1], &[1.0])],
                x(),
                y(),
            ),
        ),
    ]
}
