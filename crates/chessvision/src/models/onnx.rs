//! An ONNX model's graph, read without rten and checked before rten loads it (plan A9-V8).
//!
//! rten folds the constant part of a graph when it loads it and computes the rest when it runs,
//! and a tensor it cannot allocate aborts the process: a valid 1.3 MB model whose
//! `ConstantOfShape` asks for 65536 × 65536 × 64 floats closed the app as soon as its folder was
//! chosen (R7-01), and two small constants added together broadcast just as far. So the file's
//! protobuf is read here first, by a reader of the few fields that matter, and refused unless
//! every operator is one a classifier of these shapes is built from ([`OPERATORS`]), no node holds
//! a subgraph, every initializer and constant holds at most [`MAX_CONSTANT_ELEMENTS`] values, and
//! every value the graph makes, its shape worked out from the input the crate feeds
//! ([`MAX_BATCH`] images), holds at most [`MAX_TENSOR_ELEMENTS`].

use std::collections::HashMap;

use super::{MAX_BATCH, ModelError};

/// The operators a model may use: what a small convolutional classifier is made of. The two
/// models trained for PdfCraft use Constant, Conv, Gemm, MaxPool, Relu, Reshape and Flatten.
const OPERATORS: [&str; 16] = [
    "Conv",
    "Relu",
    "MaxPool",
    "Gemm",
    "MatMul",
    "Add",
    "Flatten",
    "Reshape",
    "Constant",
    "BatchNormalization",
    "GlobalAveragePool",
    "Dropout",
    "Identity",
    "Softmax",
    "Clip",
    "Sigmoid",
];

/// Most values in one initializer or constant (64 MiB of floats; the largest weight of the
/// models trained for PdfCraft, the piece model's first dense layer, holds 2^21).
const MAX_CONSTANT_ELEMENTS: u64 = 1 << 24;

/// Most values in any tensor the graph makes, with [`MAX_BATCH`] images in (1 GiB of floats). The
/// largest the models trained for PdfCraft make is half of it: the piece model's first
/// convolution, 1024 × 32 × 64 × 64.
const MAX_TENSOR_ELEMENTS: u64 = 1 << 28;

/// Most things read from a graph and kept: nodes, initializers, inputs, each node's input and
/// output names and attributes (the models trained for PdfCraft have about 150).
const MAX_GRAPH_ITEMS: usize = 1 << 18;

/// Most numbers in one attribute's list (a convolution's padding has four).
const MAX_ATTRIBUTE_INTS: usize = 64;

/// Most dimensions a tensor may have.
const MAX_RANK: usize = 8;

/// Most values of an integer constant kept to read a `Reshape`'s target shape.
const MAX_SHAPE_VALUES: usize = MAX_RANK;

/// ONNX's element type for 64-bit integers.
const INT64: u64 = 7;

/// Refuses the model in `bytes` unless its graph is one rten can load and run without asking for
/// a tensor too large: see the module's documentation. `input` is the input the crate feeds,
/// `[batch, 1, side, side]`.
pub(super) fn check(bytes: &[u8], input: &str, side: usize) -> Result<(), ModelError> {
    if bytes.starts_with(b"RTEN") {
        return Err(unsupported("the .rten format: export the model as ONNX"));
    }
    let graph = read_model(bytes).map_err(|e| ModelError::Load(format!("its graph could not be read: {e}")))?;
    let mut shapes = Shapes::default();
    let side = u64::try_from(side).unwrap_or(u64::MAX);
    let batch = u64::try_from(MAX_BATCH).unwrap_or(u64::MAX);
    for tensor in &graph.initializers {
        let dims = constant_dims(tensor)?;
        shapes.set(tensor.name, dims.clone())?;
        if let Some(values) = tensor.int64s() {
            shapes.ints.insert(tensor.name, values);
        }
    }
    if !graph.inputs.contains(&input) {
        return Err(ModelError::NoSuchNode(input.to_owned()));
    }
    for name in &graph.inputs {
        if shapes.dims.contains_key(name) {
            continue; // An initializer listed as an input too (IR version 3).
        }
        if *name != input {
            return Err(unsupported(&format!("an input {name:?} besides {input:?}")));
        }
        shapes.set(name, vec![batch, 1, side, side])?;
    }
    for node in &graph.nodes {
        if !matches!(node.domain, "" | "ai.onnx") {
            return Err(unsupported(&format!("the operator {} of the domain {:?}", node.op, node.domain)));
        }
        if !OPERATORS.contains(&node.op) {
            return Err(unsupported(&format!("the operator {}", node.op)));
        }
        if node.attributes.iter().any(|a| a.graph) {
            return Err(unsupported(&format!("a subgraph in {}", node.op)));
        }
        let out = shapes.infer(node)?;
        for name in node.outputs.iter().filter(|n| !n.is_empty()) {
            shapes.set(name, out.clone())?;
        }
    }
    Ok(())
}

fn unsupported(what: &str) -> ModelError {
    ModelError::Unsupported(what.to_owned())
}

/// The values' shapes worked out so far, and the integer constants a `Reshape` may read.
#[derive(Default)]
struct Shapes<'a> {
    dims: HashMap<&'a str, Vec<u64>>,
    ints: HashMap<&'a str, Vec<i64>>,
}

impl<'a> Shapes<'a> {
    /// Records `name`'s shape, refused when it holds more than [`MAX_TENSOR_ELEMENTS`].
    fn set(&mut self, name: &'a str, dims: Vec<u64>) -> Result<(), ModelError> {
        let n = elements(&dims);
        if n > MAX_TENSOR_ELEMENTS || dims.len() > MAX_RANK {
            let shown = dims.iter().map(u64::to_string).collect::<Vec<_>>().join(" × ");
            return Err(unsupported(&format!("a tensor {name:?} of {shown} values, more than {MAX_TENSOR_ELEMENTS}")));
        }
        self.dims.insert(name, dims);
        Ok(())
    }

    /// The shape of the node's `i`th input.
    fn input(&self, node: &Node, i: usize) -> Result<&[u64], ModelError> {
        let name = node.inputs.get(i).copied().unwrap_or_default();
        self.dims.get(name).map(Vec::as_slice).ok_or_else(|| unsupported(&format!("a {} reading {name:?}, which nothing before it makes", node.op)))
    }

    /// The shape of the node's outputs (an upper bound for the extra ones, such as a pool's
    /// indices or a normalization's statistics).
    fn infer(&mut self, node: &Node<'a>) -> Result<Vec<u64>, ModelError> {
        let x = |i| self.input(node, i);
        match node.op {
            "Constant" => {
                let Some(attr) = node.attributes.iter().find(|a| a.name.starts_with("value") || a.name == "sparse_value") else {
                    return Err(unsupported("a Constant with no value"));
                };
                match (attr.name, &attr.t) {
                    ("value", Some(t)) => {
                        let dims = constant_dims(t)?;
                        if let (Some(values), Some(out)) = (t.int64s(), node.outputs.first()) {
                            self.ints.insert(*out, values);
                        }
                        Ok(dims)
                    }
                    ("value_float" | "value_int", _) => Ok(Vec::new()),
                    ("value_floats" | "value_ints", _) => {
                        if attr.name == "value_ints"
                            && let Some(out) = node.outputs.first()
                            && attr.ints.len() <= MAX_SHAPE_VALUES
                        {
                            self.ints.insert(*out, attr.ints.clone());
                        }
                        let n = u64::try_from(attr.count).unwrap_or(u64::MAX);
                        if n > MAX_CONSTANT_ELEMENTS {
                            return Err(unsupported(&format!("a Constant of {n} values, more than {MAX_CONSTANT_ELEMENTS}")));
                        }
                        Ok(vec![n])
                    }
                    (other, _) => Err(unsupported(&format!("a Constant given as {other}"))),
                }
            }
            "Relu" | "Sigmoid" | "Softmax" | "Clip" | "Dropout" | "Identity" | "BatchNormalization" => Ok(x(0)?.to_vec()),
            "GlobalAveragePool" => {
                let d = x(0)?;
                Ok(d.iter().enumerate().map(|(i, v)| if i < 2 { *v } else { 1 }).collect())
            }
            "Flatten" => {
                let d = x(0)?;
                let rank = i64::try_from(d.len()).unwrap_or(i64::MAX);
                let axis = node.int("axis").unwrap_or(1);
                let axis = if axis < 0 { axis.saturating_add(rank) } else { axis };
                let at = usize::try_from(axis.clamp(0, rank)).unwrap_or(0);
                let (before, after) = d.split_at(at.min(d.len()));
                Ok(vec![elements(before), elements(after)])
            }
            "Reshape" => self.reshape(node),
            "Add" => broadcast(x(0)?, x(1)?, node),
            "MatMul" => matmul(x(0)?, x(1)?, node),
            "Gemm" => {
                let (a, b) = (x(0)?, x(1)?);
                let ([m, k], [k2, n]) = (two(a, node)?, two(b, node)?);
                let (m, _) = if node.int("transA").unwrap_or(0) != 0 { (k, m) } else { (m, k) };
                let n = if node.int("transB").unwrap_or(0) != 0 { k2 } else { n };
                Ok(vec![m, n])
            }
            "Conv" => {
                let (data, weights) = (x(0)?, x(1)?);
                let kernel = node.ints("kernel_shape").map(|k| k.iter().map(|v| u64::try_from(*v).unwrap_or(u64::MAX)).collect::<Vec<_>>());
                let kernel = kernel.unwrap_or_else(|| weights.get(2..).unwrap_or_default().to_vec());
                let channels = weights.first().copied().ok_or_else(|| unsupported("a Conv whose weights have no shape"))?;
                window(data, channels, &kernel, node, false)
            }
            "MaxPool" => {
                let data = x(0)?;
                let kernel: Vec<u64> = node.ints("kernel_shape").unwrap_or_default().iter().map(|v| u64::try_from(*v).unwrap_or(u64::MAX)).collect();
                let channels = data.get(1).copied().unwrap_or(1);
                window(data, channels, &kernel, node, node.int("ceil_mode").unwrap_or(0) != 0)
            }
            other => Err(unsupported(&format!("the operator {other}"))),
        }
    }

    /// A `Reshape`'s output: its target shape from a constant (or, before opset 5, an attribute),
    /// a 0 copying the input's dimension and one -1 taking what is left.
    fn reshape(&self, node: &Node) -> Result<Vec<u64>, ModelError> {
        let data = self.input(node, 0)?;
        let target = match node.ints("shape") {
            Some(shape) => shape.to_vec(),
            None => {
                let name = node.inputs.get(1).copied().unwrap_or_default();
                self.ints.get(name).cloned().ok_or_else(|| unsupported("a Reshape to a shape worked out while the model runs"))?
            }
        };
        let copy_zero = node.int("allowzero").unwrap_or(0) == 0;
        let mut dims = Vec::with_capacity(target.len());
        let mut free = None;
        for (i, v) in target.iter().enumerate() {
            let d = match *v {
                -1 if free.is_none() => {
                    free = Some(i);
                    1
                }
                0 if copy_zero => data.get(i).copied().unwrap_or(0),
                v => u64::try_from(v).map_err(|_| unsupported(&format!("a Reshape to the dimension {v}")))?,
            };
            dims.push(d);
        }
        if let Some(i) = free {
            let known = elements(&dims);
            let left = elements(data).checked_div(known).unwrap_or(0);
            if let Some(d) = dims.get_mut(i) {
                *d = left;
            }
        }
        Ok(dims)
    }
}

/// The values a tensor of shape `dims` holds, saturating.
fn elements(dims: &[u64]) -> u64 {
    dims.iter().fold(1_u64, |n, d| n.saturating_mul(*d))
}

/// An initializer's or constant's shape, refused when it holds more than
/// [`MAX_CONSTANT_ELEMENTS`] or names a dimension below zero.
fn constant_dims(t: &Tensor) -> Result<Vec<u64>, ModelError> {
    if t.external {
        return Err(unsupported(&format!("the initializer {:?} kept in another file", t.name)));
    }
    let dims: Vec<u64> = t
        .dims
        .iter()
        .map(|d| u64::try_from(*d))
        .collect::<Result<_, _>>()
        .map_err(|_| unsupported(&format!("the initializer {:?} of a negative size", t.name)))?;
    let n = elements(&dims);
    if n > MAX_CONSTANT_ELEMENTS || dims.len() > MAX_RANK {
        return Err(unsupported(&format!("the initializer {:?} of {n} values, more than {MAX_CONSTANT_ELEMENTS}", t.name)));
    }
    Ok(dims)
}

/// A 2-D shape, as `Gemm` takes.
fn two(d: &[u64], node: &Node) -> Result<[u64; 2], ModelError> {
    match d {
        [a, b] => Ok([*a, *b]),
        _ => Err(unsupported(&format!("a {} on a tensor of {} dimensions", node.op, d.len()))),
    }
}

/// The shape two inputs broadcast to (numpy rules; a pair that does not broadcast takes the
/// larger, an upper bound for an operation rten would refuse).
fn broadcast(a: &[u64], b: &[u64], node: &Node) -> Result<Vec<u64>, ModelError> {
    let rank = a.len().max(b.len());
    if rank > MAX_RANK {
        return Err(unsupported(&format!("a {} on a tensor of {rank} dimensions", node.op)));
    }
    let at = |d: &[u64], i: usize| -> u64 {
        let pad = rank - d.len();
        if i < pad { 1 } else { d.get(i - pad).copied().unwrap_or(1) }
    };
    Ok((0..rank)
        .map(|i| {
            let (x, y) = (at(a, i), at(b, i));
            if x == 1 {
                y
            } else if y == 1 {
                x
            } else {
                x.max(y)
            }
        })
        .collect())
}

/// `MatMul`'s output shape (numpy rules).
fn matmul(a: &[u64], b: &[u64], node: &Node) -> Result<Vec<u64>, ModelError> {
    let (a2, b2): (Vec<u64>, Vec<u64>) =
        (if a.len() == 1 { [&[1_u64][..], a].concat() } else { a.to_vec() }, if b.len() == 1 { [b, &[1_u64][..]].concat() } else { b.to_vec() });
    let (Some((&m, a_batch)), Some((&n, b_batch))) = (a2.split_last().and_then(|(_, r)| r.split_last()), b2.split_last()) else {
        return Err(unsupported(&format!("a {} on a scalar", node.op)));
    };
    let b_batch = b_batch.split_last().map_or(&[][..], |(_, r)| r);
    let mut out = broadcast(a_batch, b_batch, node)?;
    if a.len() != 1 {
        out.push(m);
    }
    if b.len() != 1 {
        out.push(n);
    }
    Ok(out)
}

/// A convolution's or pooling's output shape: `[batch, channels, spatial…]` with each spatial
/// dimension worked out from the kernel, strides, dilations and padding.
fn window(data: &[u64], channels: u64, kernel: &[u64], node: &Node, ceil: bool) -> Result<Vec<u64>, ModelError> {
    let spatial = data.get(2..).unwrap_or_default();
    if kernel.len() != spatial.len() || data.len() < 3 {
        return Err(unsupported(&format!("a {} whose kernel does not match its input", node.op)));
    }
    let attr = |name: &str, default: u64, len: usize| -> Result<Vec<u64>, ModelError> {
        match node.ints(name) {
            None => Ok(vec![default; len]),
            Some(v) if v.len() == len => {
                v.iter().map(|x| u64::try_from(*x).map_err(|_| unsupported(&format!("a {} with {name} below zero", node.op)))).collect()
            }
            Some(_) => Err(unsupported(&format!("a {} whose {name} does not match its input", node.op))),
        }
    };
    let rank = spatial.len();
    let strides = attr("strides", 1, rank)?;
    let dilations = attr("dilations", 1, rank)?;
    let pads = attr("pads", 0, rank * 2)?;
    let auto = node.string("auto_pad").unwrap_or("NOTSET");
    let mut out = vec![data.first().copied().unwrap_or(1), channels];
    for i in 0..rank {
        let (d, k, s, dil) = (at(spatial, i), at(kernel, i), at(&strides, i), at(&dilations, i));
        if s == 0 {
            return Err(unsupported(&format!("a {} with a stride of 0", node.op)));
        }
        let size = match auto {
            "SAME_UPPER" | "SAME_LOWER" => d.div_ceil(s),
            _ => {
                let padded = if auto == "VALID" { d } else { d.saturating_add(at(&pads, i)).saturating_add(at(&pads, i + rank)) };
                let span = padded.saturating_sub(dil.saturating_mul(k.saturating_sub(1))).saturating_sub(1);
                (if ceil { span.div_ceil(s) } else { span / s }).saturating_add(1)
            }
        };
        out.push(size);
    }
    Ok(out)
}

fn at(v: &[u64], i: usize) -> u64 {
    v.get(i).copied().unwrap_or(1)
}

// ---- The protobuf reader -------------------------------------------------------------------

/// The parts of a `GraphProto` checked here.
#[derive(Default)]
struct Graph<'a> {
    nodes: Vec<Node<'a>>,
    initializers: Vec<Tensor<'a>>,
    inputs: Vec<&'a str>,
}

/// The parts of a `NodeProto` checked here.
#[derive(Default)]
struct Node<'a> {
    op: &'a str,
    domain: &'a str,
    inputs: Vec<&'a str>,
    outputs: Vec<&'a str>,
    attributes: Vec<Attribute<'a>>,
}

impl Node<'_> {
    fn attribute(&self, name: &str) -> Option<&Attribute<'_>> {
        self.attributes.iter().find(|a| a.name == name)
    }

    fn int(&self, name: &str) -> Option<i64> {
        self.attribute(name).and_then(|a| a.i)
    }

    fn ints(&self, name: &str) -> Option<&[i64]> {
        self.attribute(name).map(|a| a.ints.as_slice())
    }

    fn string(&self, name: &str) -> Option<&str> {
        self.attribute(name).and_then(|a| std::str::from_utf8(a.s).ok())
    }
}

/// The parts of an `AttributeProto` checked here.
#[derive(Default)]
struct Attribute<'a> {
    name: &'a str,
    i: Option<i64>,
    ints: Vec<i64>,
    s: &'a [u8],
    t: Option<Tensor<'a>>,
    /// How many values a list attribute holds (`floats`, `ints`).
    count: usize,
    /// It holds a graph (`g` or `graphs`): `If`, `Loop` and `Scan` run one.
    graph: bool,
}

/// The parts of a `TensorProto` checked here.
#[derive(Default)]
struct Tensor<'a> {
    name: &'a str,
    dims: Vec<i64>,
    data_type: u64,
    raw: &'a [u8],
    int64_data: Vec<i64>,
    external: bool,
}

impl Tensor<'_> {
    /// The values of a small 64-bit integer tensor (a shape a `Reshape` reads).
    fn int64s(&self) -> Option<Vec<i64>> {
        if self.data_type != INT64 {
            return None;
        }
        let values: Vec<i64> =
            if self.raw.is_empty() { self.int64_data.clone() } else { self.raw.as_chunks::<8>().0.iter().map(|c| i64::from_le_bytes(*c)).collect() };
        // Every value present, or none is used (a long list was not kept).
        let dims: Vec<u64> = self.dims.iter().map(|d| u64::try_from(*d).unwrap_or(u64::MAX)).collect();
        (u64::try_from(values.len()).is_ok_and(|n| n == elements(&dims)) && values.len() <= MAX_SHAPE_VALUES).then_some(values)
    }
}

/// One field's value, by wire type.
enum Wire<'a> {
    Varint(u64),
    Fixed,
    Bytes(&'a [u8]),
}

/// The fields of a message, in order; an error ends them.
struct Fields<'a> {
    rest: &'a [u8],
}

fn fields(bytes: &[u8]) -> Fields<'_> {
    Fields { rest: bytes }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Result<(u64, Wire<'a>), String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let field = self.field();
        if field.is_err() {
            self.rest = &[];
        }
        Some(field)
    }
}

impl<'a> Fields<'a> {
    fn field(&mut self) -> Result<(u64, Wire<'a>), String> {
        let key = varint(&mut self.rest)?;
        let value = match key & 7 {
            0 => Wire::Varint(varint(&mut self.rest)?),
            1 => self.skip(8)?,
            5 => self.skip(4)?,
            2 => {
                let len = usize::try_from(varint(&mut self.rest)?).map_err(|_| "a field longer than memory")?;
                let (bytes, rest) = self.rest.split_at_checked(len).ok_or("a field past the end of the file")?;
                self.rest = rest;
                Wire::Bytes(bytes)
            }
            other => return Err(format!("the wire type {other}")),
        };
        Ok((key >> 3, value))
    }

    fn skip(&mut self, n: usize) -> Result<Wire<'a>, String> {
        self.rest = self.rest.get(n..).ok_or("a field past the end of the file")?;
        Ok(Wire::Fixed)
    }
}

fn varint(buf: &mut &[u8]) -> Result<u64, String> {
    let mut value = 0_u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = buf.split_first().ok_or("a number past the end of the file")?;
        *buf = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("a number longer than ten bytes".into())
}

fn text(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|_| "a name that is not UTF-8".into())
}

/// The 64-bit integers of a repeated field, packed or not, appended to `out`.
fn int64s(value: Wire, out: &mut Vec<i64>, max: usize) -> Result<(), String> {
    // `as`: protobuf writes an int64 as its two's complement bits, which this cast reads back.
    match value {
        Wire::Varint(v) => out.push(v as i64),
        Wire::Bytes(mut packed) => {
            while !packed.is_empty() {
                out.push(varint(&mut packed)? as i64);
                if out.len() > max {
                    return Err(format!("more than {max} numbers in one list"));
                }
            }
        }
        Wire::Fixed => return Err("a list of numbers in the wrong form".into()),
    }
    if out.len() > max { Err(format!("more than {max} numbers in one list")) } else { Ok(()) }
}

/// `ModelProto.graph`, read; a model with functions of its own is refused.
fn read_model(bytes: &[u8]) -> Result<Graph<'_>, String> {
    let mut graph = None;
    let mut budget = MAX_GRAPH_ITEMS;
    for field in fields(bytes) {
        match field? {
            (7, Wire::Bytes(g)) => graph = Some(read_graph(g, &mut budget)?),
            (25, _) => return Err("functions of its own".into()),
            _ => {}
        }
    }
    let graph = graph.ok_or("no graph")?;
    if graph.nodes.is_empty() {
        return Err("a graph with no node".into());
    }
    Ok(graph)
}

/// Takes one thing from the graph's budget of [`MAX_GRAPH_ITEMS`].
fn spend(budget: &mut usize) -> Result<(), String> {
    *budget = budget.checked_sub(1).ok_or_else(|| format!("more than {MAX_GRAPH_ITEMS} nodes, names and attributes"))?;
    Ok(())
}

fn read_graph<'a>(bytes: &'a [u8], budget: &mut usize) -> Result<Graph<'a>, String> {
    let mut graph = Graph::default();
    for field in fields(bytes) {
        match field? {
            (1, Wire::Bytes(n)) => {
                spend(budget)?;
                graph.nodes.push(read_node(n, budget)?);
            }
            (5, Wire::Bytes(t)) => {
                spend(budget)?;
                graph.initializers.push(read_tensor(t)?);
            }
            (15, _) => return Err("a sparse initializer".into()),
            (11, Wire::Bytes(v)) => {
                spend(budget)?;
                graph.inputs.push(read_name(v)?);
            }
            _ => {}
        }
    }
    Ok(graph)
}

/// `ValueInfoProto.name`.
fn read_name(bytes: &[u8]) -> Result<&str, String> {
    let mut name = "";
    for field in fields(bytes) {
        if let (1, Wire::Bytes(n)) = field? {
            name = text(n)?;
        }
    }
    Ok(name)
}

fn read_node<'a>(bytes: &'a [u8], budget: &mut usize) -> Result<Node<'a>, String> {
    let mut node = Node::default();
    for field in fields(bytes) {
        match field? {
            (1, Wire::Bytes(s)) => {
                spend(budget)?;
                node.inputs.push(text(s)?);
            }
            (2, Wire::Bytes(s)) => {
                spend(budget)?;
                node.outputs.push(text(s)?);
            }
            (4, Wire::Bytes(s)) => node.op = text(s)?,
            (7, Wire::Bytes(s)) => node.domain = text(s)?,
            (5, Wire::Bytes(a)) => {
                spend(budget)?;
                node.attributes.push(read_attribute(a)?);
            }
            _ => {}
        }
    }
    Ok(node)
}

fn read_attribute(bytes: &[u8]) -> Result<Attribute<'_>, String> {
    let mut attr = Attribute::default();
    for field in fields(bytes) {
        match field? {
            (1, Wire::Bytes(s)) => attr.name = text(s)?,
            // Two's complement bits, as in `int64s`.
            (3, Wire::Varint(v)) => attr.i = Some(v as i64),
            (4, Wire::Bytes(s)) => attr.s = s,
            (5, Wire::Bytes(t)) => attr.t = Some(read_tensor(t)?),
            (6 | 11, _) => attr.graph = true,
            (22 | 23, _) => return Err("a sparse tensor in an attribute".into()),
            (7, Wire::Bytes(floats)) => attr.count = attr.count.saturating_add(floats.len() / 4),
            (7, _) => attr.count = attr.count.saturating_add(1),
            (8, value) => {
                int64s(value, &mut attr.ints, MAX_ATTRIBUTE_INTS)?;
                attr.count = attr.ints.len();
            }
            _ => {}
        }
    }
    Ok(attr)
}

fn read_tensor(bytes: &[u8]) -> Result<Tensor<'_>, String> {
    let mut tensor = Tensor::default();
    for field in fields(bytes) {
        match field? {
            (1, value) => int64s(value, &mut tensor.dims, MAX_RANK)?,
            (2, Wire::Varint(v)) => tensor.data_type = v,
            (8, Wire::Bytes(s)) => tensor.name = text(s)?,
            (9, Wire::Bytes(raw)) => tensor.raw = raw,
            (7, value) => {
                // Kept only while short: a shape is a handful of numbers.
                let mut values = Vec::new();
                if int64s(value, &mut values, MAX_SHAPE_VALUES).is_ok() && tensor.int64_data.len().saturating_add(values.len()) <= MAX_SHAPE_VALUES {
                    tensor.int64_data.extend(values);
                }
            }
            (13, _) => tensor.external = true,
            (14, Wire::Varint(1)) => tensor.external = true,
            _ => {}
        }
    }
    Ok(tensor)
}

/// A writer of the ONNX protobuf fields the tests need, for synthetic models (no weights are
/// committed, decision D2).
#[cfg(test)]
pub(super) mod build {
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

    /// Where the raw data of the initializer `name` starts in `model`'s bytes.
    pub fn raw_offset(model: &[u8], name: &str) -> Option<usize> {
        let graph = super::read_model(model).ok()?;
        let t = graph.initializers.iter().find(|t| t.name == name)?;
        (t.raw.as_ptr() as usize).checked_sub(model.as_ptr() as usize)
    }

    /// The description of [`glyph_model`].
    pub fn glyph_meta(classes: usize) -> String {
        let names: Vec<String> = (0..classes).map(|c| format!("\"g{c}\"")).collect();
        format!(r#"{{"kind":"chess-glyphs","input":{{"name":"glyphs"}},"output":{{"name":"logits","classes":[{}]}}}}"#, names.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::build::{Attr, floats, int64s, model, node, tensor, value};
    use super::*;

    fn glyphs() -> Vec<u8> {
        value("glyphs", &[None, Some(1), Some(32), Some(32)])
    }

    fn logits() -> Vec<u8> {
        value("logits", &[None, Some(2)])
    }

    /// The shapes a small convolutional classifier makes, as the models trained for PdfCraft
    /// are built: convolution, pooling, a reshape by a constant, dense layers.
    #[test]
    fn a_convolutional_classifier_is_worked_out_layer_by_layer() {
        let bytes = model(
            &[
                node("Conv", &["glyphs", "cw", "cb"], &["c"], &[Attr::Ints("kernel_shape", &[3, 3]), Attr::Ints("pads", &[1, 1, 1, 1])]),
                node("Relu", &["c"], &["r"], &[]),
                node("MaxPool", &["r"], &["p"], &[Attr::Ints("kernel_shape", &[2, 2]), Attr::Ints("strides", &[2, 2])]),
                node("Constant", &[], &["shape"], &[Attr::Tensor("value", int64s("v", &[-1, 8 * 16 * 16]))]),
                node("Reshape", &["p", "shape"], &["flat"], &[]),
                node("Gemm", &["flat", "fw", "fb"], &["logits"], &[Attr::Int("transB", 1)]),
            ],
            &[
                floats("cw", &[8, 1, 3, 3], &[0.0; 72]),
                floats("cb", &[8], &[0.0; 8]),
                floats("fw", &[2, 8 * 16 * 16], &vec![0.0; 2 * 8 * 16 * 16]),
                floats("fb", &[2], &[0.0; 2]),
            ],
            glyphs(),
            logits(),
        );
        assert!(check(&bytes, "glyphs", 32).is_ok());
        let graph = read_model(&bytes).unwrap();
        let mut shapes = Shapes::default();
        for t in &graph.initializers {
            shapes.set(t.name, constant_dims(t).unwrap()).unwrap();
        }
        shapes.set("glyphs", vec![1024, 1, 32, 32]).unwrap();
        let mut seen = Vec::new();
        for n in &graph.nodes {
            let out = shapes.infer(n).unwrap();
            shapes.set(n.outputs[0], out.clone()).unwrap();
            seen.push(out);
        }
        assert_eq!(seen, [vec![1024, 8, 32, 32], vec![1024, 8, 32, 32], vec![1024, 8, 16, 16], vec![2], vec![1024, 2048], vec![1024, 2]]);
    }

    #[test]
    fn shapes_follow_onnx_rules() {
        let n = Node { op: "Add", ..Node::default() };
        assert_eq!(broadcast(&[1024, 256], &[256], &n).unwrap(), [1024, 256]);
        assert_eq!(broadcast(&[524_288, 1], &[1, 524_288], &n).unwrap(), [524_288, 524_288]);
        assert_eq!(matmul(&[1024, 2048], &[2048, 256], &n).unwrap(), [1024, 256]);
        assert_eq!(matmul(&[5, 3, 4], &[4, 6], &n).unwrap(), [5, 3, 6]);
        assert_eq!(matmul(&[4], &[4, 6], &n).unwrap(), [6]);
        assert_eq!(matmul(&[3, 4], &[4], &n).unwrap(), [3]);
        assert!(matmul(&[], &[4], &n).is_err());
        let pool = Node { op: "MaxPool", attributes: vec![], ..Node::default() };
        // Ceil mode rounds a partial window up.
        assert_eq!(window(&[1, 3, 7, 7], 3, &[2, 2], &pool, false).unwrap(), [1, 3, 6, 6]);
        assert_eq!(window(&[1, 3, 7, 7], 3, &[2, 2], &pool, true).unwrap(), [1, 3, 6, 6]);
        let strided =
            Node { op: "MaxPool", attributes: vec![Attribute { name: "strides", ints: vec![2, 2], ..Attribute::default() }], ..Node::default() };
        assert_eq!(window(&[1, 3, 7, 7], 3, &[2, 2], &strided, false).unwrap(), [1, 3, 3, 3]);
        assert_eq!(window(&[1, 3, 7, 7], 3, &[2, 2], &strided, true).unwrap(), [1, 3, 4, 4]);
        let zero = Node { op: "Conv", attributes: vec![Attribute { name: "strides", ints: vec![0, 1], ..Attribute::default() }], ..Node::default() };
        assert!(window(&[1, 1, 8, 8], 1, &[1, 1], &zero, false).is_err());
    }

    /// Plan A9-V8 (R7-01): graphs rten would load or run until a tensor it cannot allocate
    /// aborts the process, refused before rten sees them, each naming why.
    #[test]
    fn graphs_that_ask_for_too_much_are_refused() {
        let flat = || node("Flatten", &["glyphs"], &["flat"], &[Attr::Int("axis", 1)]);
        let gemm = || node("Gemm", &["flat", "W"], &["l0"], &[]);
        let w = || floats("W", &[1024, 2], &[0.0; 2048]);
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            (
                "ConstantOfShape",
                model(
                    &[flat(), gemm(), node("ConstantOfShape", &["big_shape"], &["big"], &[]), node("Add", &["l0", "big"], &["logits"], &[])],
                    &[w(), int64s("big_shape", &[65536, 65536, 64])],
                    glyphs(),
                    logits(),
                ),
                "ConstantOfShape",
            ),
            (
                "two constants broadcast",
                model(
                    &[flat(), gemm(), node("Add", &["A", "B"], &["big"], &[]), node("Add", &["l0", "big"], &["logits"], &[])],
                    &[w(), floats("A", &[524_288, 1], &[]), floats("B", &[1, 524_288], &[])],
                    glyphs(),
                    logits(),
                ),
                "more than 268435456",
            ),
            (
                "padding",
                model(
                    &[node("Conv", &["glyphs", "K"], &["logits"], &[Attr::Ints("pads", &[1 << 20, 1 << 20, 1 << 20, 1 << 20])])],
                    &[floats("K", &[1, 1, 1, 1], &[1.0])],
                    glyphs(),
                    logits(),
                ),
                "more than 268435456",
            ),
            ("an initializer", model(&[flat(), gemm()], &[tensor("W", &[1 << 31, 1 << 31], 1, &[0; 4])], glyphs(), logits()), "initializer"),
            ("a subgraph", model(&[node("Identity", &["glyphs"], &["logits"], &[Attr::Graph("then_branch")])], &[], glyphs(), logits()), "subgraph"),
            (
                "a domain",
                model(
                    &[{
                        let mut n = node("Relu", &["glyphs"], &["logits"], &[]);
                        n.extend_from_slice(b"\x3a\x03com");
                        n
                    }],
                    &[],
                    glyphs(),
                    logits(),
                ),
                "domain",
            ),
            (
                "a computed shape",
                model(&[flat(), node("Reshape", &["flat", "flat"], &["logits"], &[])], &[], glyphs(), logits()),
                "while the model runs",
            ),
        ];
        for (name, bytes, why) in cases {
            let err = check(&bytes, "glyphs", 32).expect_err(name).to_string();
            assert!(err.contains(why), "{name}: {err}");
        }
    }

    #[test]
    fn what_is_not_an_onnx_graph_is_refused() {
        for bytes in [&b""[..], b"RTEN\x01", b"definitely not a model", b"\x3a\x05ab", b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff"] {
            assert!(check(bytes, "glyphs", 32).is_err(), "{bytes:?}");
        }
        // What the reader keeps is bounded: names, nodes and attributes, and each list of numbers.
        let many: Vec<Vec<u8>> = (0..MAX_GRAPH_ITEMS / 2).map(|_| node("Relu", &["glyphs"], &["logits"], &[])).collect();
        let err = check(&model(&many, &[], glyphs(), logits()), "glyphs", 32).unwrap_err().to_string();
        assert!(err.contains("could not be read") && err.contains("more than"), "{err}");
        let long = node("MaxPool", &["glyphs"], &["logits"], &[Attr::Ints("pads", &[0; MAX_ATTRIBUTE_INTS + 1])]);
        let err = check(&model(&[long], &[], glyphs(), logits()), "glyphs", 32).unwrap_err().to_string();
        assert!(err.contains("numbers in one list"), "{err}");
        // A model whose input is not the one fed.
        let other = model(&[node("Relu", &["x"], &["logits"], &[])], &[], value("x", &[None, Some(1), Some(32), Some(32)]), logits());
        assert!(matches!(check(&other, "glyphs", 32), Err(ModelError::NoSuchNode(_))));
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_checked_or_refused(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1024)) {
            let _ = check(&bytes, "glyphs", 32);
        }

        /// A real graph with bytes changed is still checked or refused, never a panic.
        #[test]
        fn a_mutated_graph_is_checked_or_refused(at in 0usize..4096, byte in proptest::prelude::any::<u8>()) {
            let mut bytes = super::build::glyph_model(3);
            let len = bytes.len();
            if let Some(b) = bytes.get_mut(at % len) {
                *b = byte;
            }
            let _ = check(&bytes, "glyphs", 32);
        }
    }
}
