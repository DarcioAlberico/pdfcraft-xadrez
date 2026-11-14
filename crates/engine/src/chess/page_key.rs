//! Content keys for the scan cache (plan A8 P3a2): what lets a page's scan outlive an edit to
//! another page.
//!
//! A page's key hashes everything its scan reads from the document: the page dictionary and every
//! object it reaches (content streams, images, forms, fonts, annotations, each stream's bytes in
//! full), the attributes it inherits (`/Resources`, `/MediaBox`, `/CropBox`, `/Rotate`), and the
//! catalog entries that change how any page renders (optional content, form appearances). Two
//! states of a document that give a page the same key draw that page the same way, so a scan made
//! in one answers for the other, at whatever index the page has now: an edit elsewhere, an undo,
//! inserting or removing pages before it keep its scan.
//!
//! A key is only ever compared within a session (the cache is never written anywhere), so the
//! standard library's hasher is enough; two of them seeded apart make it 128 bits. A page nested
//! deeper than [`MAX_DEPTH`], or reached once [`MAX_OBJECTS`] objects were hashed for the document
//! state, gets no key, and its scan lasts only as long as the document state it was made in, as
//! every scan did before.
//!
//! **What is hashed once outlives an edit** (plan A9-I3b, §2.4): the memo keeps, for each object
//! it hashed, the objects it refers to, so an edit that changed some objects
//! ([`pdfcraft_cos::ReadState::changed`]) forgets the hashes of those and of whatever reaches them,
//! and nothing else ([`KeyMemo::forget`]): a new Title hashes no page again, an edit to one page
//! hashes that page.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{DefaultHasher, Hasher};

use pdfcraft_cos::{Dict, Document as Cos, ObjRef, Object};

/// Most indirect objects hashed for one document state; past it the pages left have no key.
const MAX_OBJECTS: usize = 200_000;
/// Deepest nesting of objects followed from a page; a page nested deeper has no key.
const MAX_DEPTH: usize = 96;
/// Most parents looked through for an inherited attribute.
const MAX_ANCESTORS: usize = 64;
/// The page attributes a page may inherit from the page tree (ISO 32000-2 §7.7.3.4).
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
/// The catalog's `/AcroForm` entries that may change how a field's widget is drawn.
const FORM_DRAWING: [&[u8]; 4] = [b"NeedAppearances", b"DA", b"DR", b"Q"];

/// A page's content key.
pub(crate) type PageKey = u128;

/// Two hashers seeded apart, fed the same bytes: a 128-bit hash.
struct Wide(DefaultHasher, DefaultHasher);

impl Wide {
    fn new(seed: &[u8]) -> Wide {
        let mut w = Wide(DefaultHasher::new(), DefaultHasher::new());
        w.0.write(b"pdfcraft-chess-page-key-a");
        w.1.write(b"pdfcraft-chess-page-key-b");
        w.bytes(seed);
        w
    }

    fn tag(&mut self, t: u8) {
        self.0.write_u8(t);
        self.1.write_u8(t);
    }

    fn u64(&mut self, v: u64) {
        self.0.write_u64(v);
        self.1.write_u64(v);
    }

    fn u128(&mut self, v: u128) {
        self.0.write_u128(v);
        self.1.write_u128(v);
    }

    /// `b` with its length first, so no two sequences of byte strings feed the same input.
    fn bytes(&mut self, b: &[u8]) {
        self.u64(u64::try_from(b.len()).unwrap_or(u64::MAX));
        self.0.write(b);
        self.1.write(b);
    }

    fn finish(&self) -> u128 {
        (u128::from(self.0.finish()) << 64) | u128::from(self.1.finish())
    }
}

/// What hashing a document's pages has learned so far: every indirect object's hash is computed
/// once and shared by the pages that use it (a font on every page is hashed once), and each
/// page's key. Valid for the document state it was made in, and, once told what an edit changed
/// ([`KeyMemo::forget`]), for the next.
#[derive(Default)]
pub(crate) struct KeyMemo {
    objects: HashMap<u32, Option<u128>>,
    /// Objects being hashed: a reference back to one of them is a cycle, hashed by number.
    open: HashSet<u32>,
    /// The objects (a page, the catalog, an object) being hashed, innermost last: the one a
    /// reference found now is in.
    path: Vec<u32>,
    /// What each object hashed refers to (and a page, the page-tree nodes it inherits from): what
    /// is hashed again when one of those changes.
    uses: HashMap<u32, HashSet<u32>>,
    hashed: usize,
    /// The catalog's hash, with the catalog it was made from.
    catalog: Option<(Option<ObjRef>, Option<u128>)>,
    /// Each page's key, by its object, all made with the catalog's hash `pages_catalog`.
    pages: HashMap<u32, Option<PageKey>>,
    pages_catalog: Option<u128>,
    /// Pages whose key was worked out rather than taken from here (plan A9-I3b's measure).
    pub(crate) pages_hashed: usize,
}

impl KeyMemo {
    /// The key of the page whose object is `page`, or `None` when it can't be told (not a page,
    /// too many or too deep objects).
    pub(crate) fn page(&mut self, cos: &Cos, page: ObjRef) -> Option<PageKey> {
        let catalog = self.catalog(cos)?;
        if self.pages_catalog != Some(catalog) {
            self.pages.clear();
            self.pages_catalog = Some(catalog);
        }
        if let Some(known) = self.pages.get(&page.num) {
            return *known;
        }
        self.pages_hashed = self.pages_hashed.saturating_add(1);
        self.uses.remove(&page.num);
        let key = self.page_key(cos, page, catalog);
        self.pages.insert(page.num, key);
        key
    }

    fn page_key(&mut self, cos: &Cos, page: ObjRef, catalog: u128) -> Option<PageKey> {
        let object = cos.get(page);
        let dict = object.as_dict()?;
        let mut h = Wide::new(b"page");
        self.open.insert(page.num);
        self.path.push(page.num);
        let hashed = self.page_dict(cos, page.num, dict, &mut h);
        self.path.pop();
        self.open.remove(&page.num);
        hashed?;
        h.u128(catalog);
        Some(h.finish())
    }

    /// The document changed: `changed` (by number, [`pdfcraft_cos::ReadState::changed`]) are the
    /// objects it reads differently now. The hashes of those, and of every object, page and
    /// catalog that refers to one of them however deep, are forgotten; the rest stand. A page
    /// that had no key is worked out again.
    pub(crate) fn forget(&mut self, changed: &BTreeSet<u32>) {
        let mut users: HashMap<u32, Vec<u32>> = HashMap::new();
        for (from, to) in &self.uses {
            for t in to {
                users.entry(*t).or_default().push(*from);
            }
        }
        let mut gone: HashSet<u32> = changed.iter().copied().collect();
        let mut queue: Vec<u32> = changed.iter().copied().collect();
        while let Some(n) = queue.pop() {
            for u in users.get(&n).into_iter().flatten() {
                if gone.insert(*u) {
                    queue.push(*u);
                }
            }
        }
        self.objects.retain(|n, _| !gone.contains(n));
        self.uses.retain(|n, _| !gone.contains(n));
        self.pages.retain(|n, key| key.is_some() && !gone.contains(n));
        if self.catalog.is_some_and(|(root, _)| root.is_none_or(|r| gone.contains(&r.num))) {
            self.catalog = None;
        }
        self.hashed = self.objects.len();
    }

    /// `to` is used by the object being hashed (the innermost of [`Self::path`]).
    fn used(&mut self, to: u32) {
        if let Some(&from) = self.path.last() {
            self.uses.entry(from).or_default().insert(to);
        }
    }

    /// The page's entries in order, except its `/Parent` (the page tree reaches every page) and
    /// the inheritable attributes, then each of those as the page has it, its own or the nearest
    /// ancestor's: restructuring the page tree copies inherited attributes onto the pages, which
    /// changes nothing they draw. The page-tree nodes looked through are the page's to hash again
    /// when one changes.
    fn page_dict(&mut self, cos: &Cos, num: u32, page: &Dict, h: &mut Wide) -> Option<()> {
        for (k, v) in page.iter() {
            if k.as_slice() == b"Parent" || INHERITED.contains(&k.as_slice()) {
                continue;
            }
            h.bytes(k);
            self.object(cos, v, h, 1)?;
        }
        let mut ancestors = Vec::new();
        for key in INHERITED {
            h.bytes(key);
            match effective(cos, page, key, &mut ancestors) {
                Some(v) => {
                    h.tag(20);
                    self.object(cos, &v, h, 1)?;
                }
                None => h.tag(21),
            }
        }
        self.uses.entry(num).or_default().extend(ancestors);
        Some(())
    }

    /// The catalog entries every page's drawing depends on.
    fn catalog(&mut self, cos: &Cos) -> Option<u128> {
        let root = cos.root();
        if let Some((made_from, known)) = self.catalog
            && made_from == root
        {
            return known;
        }
        if let Some(r) = root {
            self.uses.remove(&r.num);
            self.path.push(r.num);
        }
        let mut h = Wide::new(b"catalog");
        let found = (|| {
            let Some(root) = root else { return Some(()) };
            let catalog = cos.get(root);
            let Some(dict) = catalog.as_dict() else { return Some(()) };
            if let Some(oc) = dict.get(b"OCProperties") {
                h.tag(22);
                self.object(cos, oc, &mut h, 1)?;
            }
            let form = dict.get(b"AcroForm").map(|f| cos.resolve(f));
            if let Some(form) = form.as_deref().and_then(Object::as_dict) {
                for key in FORM_DRAWING {
                    if let Some(v) = form.get(key) {
                        h.tag(23);
                        h.bytes(key);
                        self.object(cos, v, &mut h, 1)?;
                    }
                }
            }
            Some(())
        })();
        if root.is_some() {
            self.path.pop();
        }
        let key = found.map(|()| h.finish());
        self.catalog = Some((root, key));
        key
    }

    /// `o` into `h`. An array, dictionary or stream goes in as the hash of its content, and a
    /// reference as the hash of what it refers to, so the same dictionary given directly or by
    /// reference (as copying an inherited attribute may give it) hashes the same.
    fn object(&mut self, cos: &Cos, o: &Object, h: &mut Wide, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        match o {
            Object::Null => h.tag(0),
            Object::Bool(b) => {
                h.tag(1);
                h.u64(u64::from(*b));
            }
            Object::Int(i) => {
                h.tag(2);
                h.u64(i.cast_unsigned());
            }
            Object::Real(r) => {
                h.tag(3);
                h.u64(r.to_bits());
            }
            Object::String(s) => {
                h.tag(if s.hex { 4 } else { 5 });
                h.bytes(&s.bytes);
            }
            Object::Name(n) => {
                h.tag(6);
                h.bytes(n);
            }
            Object::Array(_) | Object::Dict(_) | Object::Stream(_) => {
                h.tag(7);
                h.u128(self.composite(cos, o, depth)?);
            }
            Object::Ref(r) => {
                h.tag(7);
                h.u128(self.indirect(cos, *r, depth.saturating_add(1))?);
            }
        }
        Some(())
    }

    /// The hash of an array's, dictionary's or stream's content (each stream's bytes in full),
    /// or of any other object as [`Self::object`] puts it.
    fn composite(&mut self, cos: &Cos, o: &Object, depth: usize) -> Option<u128> {
        let mut h = Wide::new(b"value");
        let inner = depth.saturating_add(1);
        match o {
            Object::Array(items) => {
                h.tag(8);
                h.u64(u64::try_from(items.len()).unwrap_or(u64::MAX));
                for item in items {
                    self.object(cos, item, &mut h, inner)?;
                }
            }
            Object::Dict(d) => {
                h.tag(9);
                self.dict(cos, d, &mut h, inner)?;
            }
            Object::Stream(s) => {
                h.tag(10);
                self.dict(cos, &s.dict, &mut h, inner)?;
                h.bytes(s.raw.as_ref());
            }
            other => self.object(cos, other, &mut h, inner)?,
        }
        Some(h.finish())
    }

    /// `d`'s entries in order.
    fn dict(&mut self, cos: &Cos, d: &Dict, h: &mut Wide, depth: usize) -> Option<()> {
        h.u64(u64::try_from(d.iter().count()).unwrap_or(u64::MAX));
        for (k, v) in d.iter() {
            h.bytes(k);
            self.object(cos, v, h, depth)?;
        }
        Some(())
    }

    /// The hash of what `r` refers to ([`Self::composite`]), computed once per document state.
    fn indirect(&mut self, cos: &Cos, r: ObjRef, depth: usize) -> Option<u128> {
        self.used(r.num);
        if let Some(known) = self.objects.get(&r.num) {
            return *known;
        }
        if self.open.contains(&r.num) {
            // A cycle back to an object being hashed: its content is in the hash already.
            let mut h = Wide::new(b"cycle");
            h.u64(u64::from(r.num));
            return Some(h.finish());
        }
        self.hashed = self.hashed.saturating_add(1);
        if self.hashed > MAX_OBJECTS {
            return None;
        }
        let object = cos.get(r);
        // Another page (a link's destination, a field's widget elsewhere) or the page tree does
        // not change how this page is drawn: it stands as its number.
        if object.as_dict().is_some_and(|d| matches!(d.get(b"Type"), Some(Object::Name(t)) if t == b"Page" || t == b"Pages")) {
            let mut h = Wide::new(b"page-ref");
            h.u64(u64::from(r.num));
            let key = Some(h.finish());
            self.objects.insert(r.num, key);
            return key;
        }
        self.open.insert(r.num);
        self.path.push(r.num);
        self.uses.remove(&r.num);
        let key = self.composite(cos, &object, depth);
        self.path.pop();
        self.open.remove(&r.num);
        // A key cut short by the depth limit depends on the path that reached the object, so
        // only finished ones are kept (a missing one makes the page keyless anyway).
        if key.is_some() {
            self.objects.insert(r.num, key);
        }
        key
    }
}

/// `key` as `page` has it: its own entry, else the nearest ancestor's in the page tree. The
/// ancestors looked through are added to `through`.
fn effective(cos: &Cos, page: &Dict, key: &[u8], through: &mut Vec<u32>) -> Option<Object> {
    if let Some(v) = page.get(key) {
        return Some(v.clone());
    }
    let mut seen = HashSet::new();
    let mut parent = page.get(b"Parent").cloned();
    for _ in 0..MAX_ANCESTORS {
        let Some(Object::Ref(r)) = parent else { return None };
        if !seen.insert(r.num) {
            return None;
        }
        through.push(r.num);
        let node = cos.get(r);
        let d = node.as_dict()?;
        if let Some(v) = d.get(key) {
            return Some(v.clone());
        }
        parent = d.get(b"Parent").cloned();
    }
    None
}
