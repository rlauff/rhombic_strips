//! WebAssembly bindings for the browser frontend (`www/`).
//!
//! This module is the wasm counterpart of `gui.rs`: it re-exposes the poset
//! generators and the strip search without any egui dependency. The editor
//! itself (canvas, dragging, undo, ...) lives in JavaScript; Rust only sees
//! a *wire graph*: node labels plus index pairs.
//!
//! * Poset mode: `edges[k] = (lower, upper)` are cover relations.
//! * Graph mode: `edges` are undirected.
//!
//! All functions exchange JSON strings, so the JS side needs no generated
//! TypeScript. Errors surface as thrown JS strings. The pure logic lives in
//! [`api`] (plain `Result<String, String>`, unit-testable on the host); the
//! `#[wasm_bindgen]` wrappers below only translate errors to `JsValue`.
//!
//! Long-running work (existence / count / enumerate) goes through
//! [`StripEnumerator`], which replaces the native worker thread: the browser
//! calls `step(budget_ms, max_strips)` in a Web Worker loop, so the search is
//! sliceable, streamable and cancellable — same contract as the bounded
//! channel in `gui.rs`, with `postMessage` instead of `mpsc`.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::lattice::{FaceId, Lattice};
use crate::plotting;
use crate::rhombic::{self, Strip};

pub mod api {
    //! Pure, host-testable implementations.

    use std::collections::HashMap;

    use serde::{Deserialize, Serialize};

    use crate::lattice::{Face, FaceId, Lattice};

    // -- wire format ---------------------------------------------------------

    #[derive(Serialize, Deserialize, Default, Clone)]
    pub struct WireGraph {
        pub labels: Vec<String>,
        /// Poset mode: cover relations `(lower, upper)`; graph mode: edges.
        pub edges: Vec<(usize, usize)>,
        /// Longest-path ranks (poset outputs only); minima have rank 0.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ranks: Option<Vec<usize>>,
    }

    impl WireGraph {
        pub fn parse(json: &str) -> Result<Self, String> {
            serde_json::from_str(json).map_err(|e| format!("bad graph JSON: {}", e))
        }

        pub fn to_json(&self) -> String {
            serde_json::to_string(self).expect("WireGraph serializes")
        }

        fn with_ranks(mut self) -> Result<Self, String> {
            self.ranks = Some(ranks(&self)?);
            Ok(self)
        }
    }

    /// Longest-path rank of every node (minimal elements have rank 0).
    /// Errors if the relation is cyclic. Mirrors `PosetGraph::ranks`.
    pub fn ranks(g: &WireGraph) -> Result<Vec<usize>, String> {
        let n = g.labels.len();
        let mut succ: Vec<Vec<usize>> = vec![vec![]; n];
        let mut indeg = vec![0usize; n];
        for &(a, b) in &g.edges {
            if a >= n || b >= n {
                return Err(format!("edge ({}, {}) out of range", a, b));
            }
            succ[a].push(b);
            indeg[b] += 1;
        }

        let mut rank = vec![0usize; n];
        let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
        let mut processed = 0;
        while let Some(i) = queue.pop() {
            processed += 1;
            for &j in &succ[i] {
                rank[j] = rank[j].max(rank[i] + 1);
                indeg[j] -= 1;
                if indeg[j] == 0 {
                    queue.push(j);
                }
            }
        }
        if processed != n {
            return Err("Relation contains a cycle — not a poset.".to_string());
        }
        Ok(rank)
    }

    /// Convert a wire poset to faces for `Lattice::from_faces`.
    /// Faces are in node order, so FaceId == node index.
    pub fn wire_to_faces(g: &WireGraph) -> Result<Vec<Face>, String> {
        let rank = ranks(g)?;
        let n = g.labels.len();
        let mut upsets: Vec<Vec<FaceId>> = vec![vec![]; n];
        let mut downsets: Vec<Vec<FaceId>> = vec![vec![]; n];
        for &(a, b) in &g.edges {
            upsets[a].push(b);
            downsets[b].push(a);
        }
        Ok(g.labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                Face::new(
                    label.clone(),
                    rank[i],
                    std::mem::take(&mut upsets[i]),
                    std::mem::take(&mut downsets[i]),
                )
            })
            .collect())
    }

    /// Ranks as JSON `[r0, r1, ...]` (also validates acyclicity).
    pub fn poset_ranks(graph_json: &str) -> Result<String, String> {
        let g = WireGraph::parse(graph_json)?;
        Ok(serde_json::to_string(&ranks(&g)?).unwrap())
    }

    // -- persistence: `dim: label: {upset}, {downset}` -------------------------

    pub fn to_lattice_file(graph_json: &str) -> Result<String, String> {
        let g = WireGraph::parse(graph_json)?;
        let faces = wire_to_faces(&g)?;
        let fmt_set =
            |s: &[FaceId]| s.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
        let mut out = String::new();
        for face in &faces {
            let label = face.label().replace(": ", "-"); // ": " is the field separator
            out.push_str(&format!(
                "{}: {}: {{{}}}, {{{}}}\n",
                face.dim(),
                label,
                fmt_set(face.upset()),
                fmt_set(face.downset())
            ));
        }
        Ok(out)
    }

    pub fn from_lattice_file(content: &str) -> Result<String, String> {
        let l = Lattice::from_str_content(content)?;
        let mut g = WireGraph::default();
        for (_, face) in l.faces() {
            g.labels.push(face.label().to_string());
        }
        for (id, face) in l.faces() {
            for &d in face.downset() {
                g.edges.push((d, id));
            }
        }
        Ok(g.with_ranks()?.to_json())
    }

    // -- poset generators (ports of the PosetGraph generators in gui.rs) -------

    /// Product of chains. `"211"` gives C3 x C2 x C2 (per-digit); inputs with
    /// separators like `"12,3"` allow multi-digit chain lengths.
    pub fn gen_grid(spec: &str) -> Result<String, String> {
        let dims: Vec<u32> = if spec.chars().all(|c| c.is_ascii_digit()) {
            spec.chars().filter_map(|c| c.to_digit(10)).collect()
        } else {
            spec.split(|c: char| !c.is_ascii_digit())
                .filter(|t| !t.is_empty())
                .filter_map(|t| t.parse().ok())
                .collect()
        };
        if dims.is_empty() {
            return Err("Enter chain lengths, e.g. 211 or 12,3.".to_string());
        }
        let size: u64 = dims.iter().map(|&d| d as u64 + 1).product();
        if size > 5_000 {
            return Err(format!("Grid too large ({} elements).", size));
        }

        let mut points: Vec<Vec<u32>> = vec![vec![]];
        for &d in &dims {
            points = points
                .into_iter()
                .flat_map(|p| {
                    (0..=d).map(move |i| {
                        let mut q = p.clone();
                        q.push(i);
                        q
                    })
                })
                .collect();
        }

        let sep = if dims.iter().all(|&d| d <= 9) { "" } else { "," };
        let mut g = WireGraph::default();
        for p in &points {
            g.labels
                .push(p.iter().map(u32::to_string).collect::<Vec<_>>().join(sep));
        }
        for (i, p) in points.iter().enumerate() {
            for (j, q) in points.iter().enumerate() {
                let mut up_by_one = None;
                let mut ok = true;
                for k in 0..p.len() {
                    if p[k] != q[k] {
                        if up_by_one.is_none() && q[k] == p[k] + 1 {
                            up_by_one = Some(k);
                        } else {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok && up_by_one.is_some() {
                    g.edges.push((i, j));
                }
            }
        }
        Ok(g.with_ranks()?.to_json())
    }

    /// Face lattice of the d-cube (without the empty face): all words over
    /// {0, 1, *}, covers replace one fixed coordinate by *.
    pub fn gen_cube(d: usize) -> Result<String, String> {
        if !(1..=5).contains(&d) {
            return Err("Cube dimension must be between 1 and 5.".to_string());
        }
        let mut faces: Vec<String> = (0..3usize.pow(d as u32))
            .map(|mut k| {
                (0..d)
                    .map(|_| {
                        let c = ['0', '1', '*'][k % 3];
                        k /= 3;
                        c
                    })
                    .collect()
            })
            .collect();
        faces.sort_by_key(|f: &String| (f.matches('*').count(), f.clone()));
        let idx: HashMap<String, usize> =
            faces.iter().enumerate().map(|(i, f)| (f.clone(), i)).collect();

        let mut g = WireGraph { labels: faces.clone(), ..Default::default() };
        for (i, f) in faces.iter().enumerate() {
            for (k, c) in f.chars().enumerate() {
                if c != '*' {
                    let mut upper: Vec<char> = f.chars().collect();
                    upper[k] = '*';
                    g.edges.push((i, idx[&upper.into_iter().collect::<String>()]));
                }
            }
        }
        Ok(g.with_ranks()?.to_json())
    }

    /// Face lattice of the d-simplex (without the empty face): nonempty
    /// subsets of {0, ..., d}, covers add one element.
    pub fn gen_simplex(d: usize) -> Result<String, String> {
        if !(1..=6).contains(&d) {
            return Err("Simplex dimension must be between 1 and 6.".to_string());
        }
        let n = d + 1;
        let mut masks: Vec<u64> = (1..(1u64 << n)).collect();
        masks.sort_by_key(|m| (m.count_ones(), *m));
        let idx: HashMap<u64, usize> =
            masks.iter().enumerate().map(|(i, &m)| (m, i)).collect();

        let label = |m: u64| -> String {
            (0..n).filter(|&v| (m >> v) & 1 == 1).map(|v| v.to_string()).collect()
        };
        let mut g = WireGraph::default();
        for &m in &masks {
            g.labels.push(label(m));
        }
        for (i, &m) in masks.iter().enumerate() {
            for v in 0..n {
                if (m >> v) & 1 == 0 {
                    g.edges.push((i, idx[&(m | (1 << v))]));
                }
            }
        }
        Ok(g.with_ranks()?.to_json())
    }

    /// Infer cover relations from all-digit labels: same length, digit sums
    /// differing by one, and exactly one differing position.
    /// Returns `{"graph": ..., "added": n}`.
    pub fn infer_digit_relations(graph_json: &str) -> Result<String, String> {
        let mut g = WireGraph::parse(graph_json)?;
        let digit_sum = |s: &str| -> Option<i64> {
            s.chars().map(|c| c.to_digit(10).map(|d| d as i64)).sum::<Option<i64>>()
        };
        let mut added = 0usize;
        let n = g.labels.len();
        for i in 0..n {
            for j in 0..n {
                if i == j || g.labels[i].len() != g.labels[j].len() {
                    continue;
                }
                let (Some(sa), Some(sb)) =
                    (digit_sum(&g.labels[i]), digit_sum(&g.labels[j]))
                else {
                    continue;
                };
                if sb != sa + 1 {
                    continue;
                }
                let diff = g.labels[i]
                    .chars()
                    .zip(g.labels[j].chars())
                    .filter(|(x, y)| x != y)
                    .count();
                if diff == 1 && !g.edges.contains(&(i, j)) {
                    g.edges.push((i, j));
                    added += 1;
                }
            }
        }
        let g = g.with_ranks()?;
        Ok(format!("{{\"graph\":{},\"added\":{}}}", g.to_json(), added))
    }

    /// The distributive lattice J(P) of order ideals of the given poset.
    pub fn gen_distributive(graph_json: &str) -> Result<String, String> {
        let g = WireGraph::parse(graph_json)?;
        ranks(&g)?; // must be acyclic
        let n = g.labels.len();
        if n > 20 {
            return Err(format!("Poset too large ({} > 20 elements) for J(P).", n));
        }
        let mut down_mask = vec![0u64; n];
        for &(a, b) in &g.edges {
            down_mask[b] |= 1 << a;
        }

        let mut ideals: Vec<u64> = Vec::new();
        for mask in 0u64..(1 << n) {
            let ok = (0..n)
                .all(|i| (mask >> i) & 1 == 0 || (mask & down_mask[i]) == down_mask[i]);
            if ok {
                ideals.push(mask);
            }
        }

        let mut out = WireGraph::default();
        for &mask in &ideals {
            let mut labels: Vec<&str> = (0..n)
                .filter(|&i| (mask >> i) & 1 == 1)
                .map(|i| g.labels[i].as_str())
                .collect();
            labels.sort_unstable();
            out.labels.push(if labels.is_empty() {
                "0".to_string()
            } else {
                format!("{{{}}}", labels.join(","))
            });
        }
        for (i, &ma) in ideals.iter().enumerate() {
            for (j, &mb) in ideals.iter().enumerate() {
                if mb.count_ones() == ma.count_ones() + 1 && (ma & mb) == ma {
                    out.edges.push((i, j));
                }
            }
        }
        Ok(out.with_ranks()?.to_json())
    }

    // -- graph generators (graph mode) -----------------------------------------

    /// `kind`: "path" | "cycle" | "complete" | "star", on n vertices.
    pub fn gen_graph(kind: &str, n: usize) -> Result<String, String> {
        if n == 0 || n > 32 {
            return Err("n must be between 1 and 32.".to_string());
        }
        let mut g = WireGraph::default();
        for i in 0..n {
            g.labels.push(i.to_string());
        }
        match kind {
            "path" => {
                for i in 0..n.saturating_sub(1) {
                    g.edges.push((i, i + 1));
                }
            }
            "cycle" => {
                if n > 2 {
                    for i in 0..n {
                        g.edges.push((i, (i + 1) % n));
                    }
                } else if n == 2 {
                    g.edges.push((0, 1));
                }
            }
            "complete" => {
                for i in 0..n {
                    for j in (i + 1)..n {
                        g.edges.push((i, j));
                    }
                }
            }
            "star" => {
                for i in 1..n {
                    g.edges.push((0, i));
                }
            }
            _ => return Err(format!("unknown graph kind '{}'", kind)),
        }
        Ok(g.to_json())
    }

    // -- graph associahedra: tubes and tubings (Carr–Devadoss) ------------------

    type Mask = u64;

    fn adjacency_masks(g: &WireGraph) -> Vec<Mask> {
        let mut adj = vec![0u64; g.labels.len()];
        for &(a, b) in &g.edges {
            if a != b && a < adj.len() && b < adj.len() {
                adj[a] |= 1 << b;
                adj[b] |= 1 << a;
            }
        }
        adj
    }

    fn mask_connected(mask: Mask, adj: &[Mask]) -> bool {
        if mask == 0 {
            return false;
        }
        let mut seen = 1u64 << mask.trailing_zeros();
        loop {
            let mut grow = seen;
            let mut m = seen;
            while m != 0 {
                let v = m.trailing_zeros() as usize;
                grow |= adj[v] & mask;
                m &= m - 1;
            }
            if grow == seen {
                break;
            }
            seen = grow;
        }
        seen == mask
    }

    fn mask_neighbors(mask: Mask, adj: &[Mask]) -> Mask {
        let mut nb = 0;
        let mut m = mask;
        while m != 0 {
            let v = m.trailing_zeros() as usize;
            nb |= adj[v];
            m &= m - 1;
        }
        nb & !mask
    }

    fn tubes_compatible(a: Mask, b: Mask, adj: &[Mask]) -> bool {
        if a & b != 0 {
            let u = a | b;
            u == a || u == b
        } else {
            mask_neighbors(a, adj) & b == 0
        }
    }

    fn enumerate_tubings(
        tubes: &[Mask],
        adj: &[Mask],
        cap: usize,
    ) -> Result<Vec<Vec<usize>>, String> {
        let m = tubes.len();
        let words = m.div_ceil(64);
        let mut compat = vec![vec![0u64; words]; m];
        for i in 0..m {
            for j in (i + 1)..m {
                if tubes_compatible(tubes[i], tubes[j], adj) {
                    compat[i][j / 64] |= 1 << (j % 64);
                    compat[j][i / 64] |= 1 << (i % 64);
                }
            }
        }
        let is_compat = |i: usize, j: usize| compat[i][j / 64] >> (j % 64) & 1 == 1;

        fn rec(
            start: usize,
            current: &mut Vec<usize>,
            out: &mut Vec<Vec<usize>>,
            m: usize,
            cap: usize,
            is_compat: &dyn Fn(usize, usize) -> bool,
        ) -> Result<(), String> {
            out.push(current.clone());
            if out.len() > cap {
                return Err(format!("More than {} tubings — aborting.", cap));
            }
            for j in start..m {
                if current.iter().all(|&i| is_compat(i, j)) {
                    current.push(j);
                    rec(j + 1, current, out, m, cap, is_compat)?;
                    current.pop();
                }
            }
            Ok(())
        }

        let mut out = Vec::new();
        rec(0, &mut Vec::new(), &mut out, m, cap, &is_compat)?;
        Ok(out)
    }

    fn tube_label(mask: Mask, vertex_labels: &[&str]) -> String {
        let single = vertex_labels.iter().all(|l| l.chars().count() == 1);
        let parts: Vec<&str> = (0..vertex_labels.len())
            .filter(|&v| (mask >> v) & 1 == 1)
            .map(|v| vertex_labels[v])
            .collect();
        parts.join(if single { "" } else { "," })
    }

    fn checked_graph(g: &WireGraph, max_n: usize) -> Result<(usize, Vec<Mask>, Mask), String> {
        let n = g.labels.len();
        if n < 2 {
            return Err("Draw a graph with at least 2 vertices first.".to_string());
        }
        if n > max_n {
            return Err(format!("Graph too large ({} > {} vertices).", n, max_n));
        }
        let adj = adjacency_masks(g);
        let full: Mask = (1 << n) - 1;
        if !mask_connected(full, &adj) {
            return Err("Graph must be connected.".to_string());
        }
        Ok((n, adj, full))
    }

    /// The poset of tubes of the drawn graph under inclusion.
    pub fn gen_tube_poset(graph_json: &str) -> Result<String, String> {
        let g = WireGraph::parse(graph_json)?;
        let (_, adj, full) = checked_graph(&g, 10)?;
        let mut subs: Vec<Mask> = (1..=full).filter(|&m| mask_connected(m, &adj)).collect();
        subs.sort_by_key(|m| (m.count_ones(), *m));

        let vertex_labels: Vec<&str> = g.labels.iter().map(String::as_str).collect();
        let mut out = WireGraph::default();
        for &m in &subs {
            out.labels.push(tube_label(m, &vertex_labels));
        }
        for (i, &a) in subs.iter().enumerate() {
            for (j, &b) in subs.iter().enumerate() {
                if b.count_ones() == a.count_ones() + 1 && a & b == a {
                    out.edges.push((i, j));
                }
            }
        }
        Ok(out.with_ranks()?.to_json())
    }

    /// The face lattice of the graph associahedron of the drawn graph.
    /// Path -> associahedron, complete -> permutahedron, cycle -> cyclohedron,
    /// star -> stellahedron.
    pub fn gen_graph_associahedron(graph_json: &str) -> Result<String, String> {
        let g = WireGraph::parse(graph_json)?;
        let (_, adj, full) = checked_graph(&g, 12)?;
        let tubes: Vec<Mask> = (1..full).filter(|&m| mask_connected(m, &adj)).collect();
        let tubings = enumerate_tubings(&tubes, &adj, 20_000)?;

        let vertex_labels: Vec<&str> = g.labels.iter().map(String::as_str).collect();
        let tubing_label = |t: &[usize]| -> String {
            if t.is_empty() {
                return "*".to_string();
            }
            let mut sorted = t.to_vec();
            sorted.sort_by_key(|&i| (tubes[i].count_ones(), tubes[i]));
            sorted
                .iter()
                .map(|&i| tube_label(tubes[i], &vertex_labels))
                .collect::<Vec<_>>()
                .join("|")
        };

        let mut out = WireGraph::default();
        let mut index: HashMap<Vec<usize>, usize> = HashMap::new();
        for (i, t) in tubings.iter().enumerate() {
            out.labels.push(tubing_label(t));
            index.insert(t.clone(), i);
        }
        // face(T) is covered by face(T \ {t}): removing a tube goes one dim up
        for (i, t) in tubings.iter().enumerate() {
            for k in 0..t.len() {
                let mut sup = t.clone();
                sup.remove(k);
                out.edges.push((i, index[&sup]));
            }
        }
        Ok(out.with_ranks()?.to_json())
    }
}

// ===========================================================================
// wasm-bindgen wrappers
// ===========================================================================

macro_rules! js_api {
    ($(fn $name:ident($($arg:ident : $ty:ty),*);)*) => {$(
        #[wasm_bindgen]
        pub fn $name($($arg: $ty),*) -> Result<String, JsValue> {
            api::$name($($arg),*).map_err(|e| JsValue::from_str(&e))
        }
    )*};
}

js_api! {
    fn poset_ranks(graph_json: &str);
    fn to_lattice_file(graph_json: &str);
    fn from_lattice_file(content: &str);
    fn gen_grid(spec: &str);
    fn gen_cube(d: usize);
    fn gen_simplex(d: usize);
    fn infer_digit_relations(graph_json: &str);
    fn gen_distributive(graph_json: &str);
    fn gen_graph(kind: &str, n: usize);
    fn gen_tube_poset(graph_json: &str);
    fn gen_graph_associahedron(graph_json: &str);
}

// ===========================================================================
// StripEnumerator: sliceable, streamable strip search
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Exists,
    Count,
    Enumerate,
}

#[derive(Serialize)]
struct StripOut {
    layers: Vec<Vec<FaceId>>,
    edges: Vec<(FaceId, FaceId)>,
    #[serde(rename = "cyclicEdges")]
    cyclic_edges: Vec<(FaceId, FaceId)>,
}

#[derive(Serialize)]
struct StepOut {
    strips: Vec<StripOut>,
    count: usize,
    done: bool,
}

/// Owns a lattice and a lazy iterator over its rhombic strips.
///
/// The lattice is heap-allocated and leaked so the iterator (which borrows
/// it) can live alongside; `Drop` reclaims it after the iterator is gone.
/// FaceIds in the output are node indices of the graph passed to `new`
/// (faces are built in node order).
#[wasm_bindgen]
pub struct StripEnumerator {
    lattice: *mut Lattice,
    iter: Option<Box<dyn Iterator<Item = Strip>>>,
    cyclic: bool,
    mode: Mode,
    count: usize,
    done: bool,
}

#[wasm_bindgen]
impl StripEnumerator {
    /// `mode`: "exists" | "count" | "enumerate".
    #[wasm_bindgen(constructor)]
    pub fn new(graph_json: &str, cyclic: bool, mode: &str) -> Result<StripEnumerator, JsValue> {
        Self::create(graph_json, cyclic, mode).map_err(|e| JsValue::from_str(&e))
    }

    /// Advance the search for at most `budget_ms` milliseconds, collecting at
    /// most `max_strips` strips (ignored in count mode). Returns JSON:
    /// `{"strips": [...], "count": n, "done": bool}`.
    pub fn step(&mut self, budget_ms: f64, max_strips: usize) -> String {
        let mut out = StepOut { strips: vec![], count: self.count, done: self.done };
        if self.done {
            return serde_json::to_string(&out).unwrap();
        }

        let start = now_ms();
        // SAFETY: see `create` — the lattice outlives every borrow taken here.
        let l: &Lattice = unsafe { &*self.lattice };
        let iter = self.iter.as_mut().expect("iterator present until done");

        loop {
            match iter.next() {
                Some(strip) => {
                    self.count += 1;
                    match self.mode {
                        Mode::Count => {}
                        Mode::Exists | Mode::Enumerate => {
                            let (edges, cyclic_edges) =
                                plotting::edges_strip(&strip, l, self.cyclic);
                            out.strips.push(StripOut { layers: strip, edges, cyclic_edges });
                            if self.mode == Mode::Exists {
                                self.done = true;
                                break;
                            }
                            if out.strips.len() >= max_strips.max(1) {
                                break;
                            }
                        }
                    }
                }
                None => {
                    self.done = true;
                    break;
                }
            }
            // In count mode check the budget only every few iterations;
            // Date.now() is cheap but not free.
            if (self.mode != Mode::Count || self.count % 256 == 0)
                && now_ms() - start >= budget_ms
            {
                break;
            }
        }

        if self.done {
            self.iter = None; // release the borrow eagerly
        }
        out.count = self.count;
        out.done = self.done;
        serde_json::to_string(&out).unwrap()
    }
}

impl StripEnumerator {
    fn create(graph_json: &str, cyclic: bool, mode: &str) -> Result<Self, String> {
        let mode = match mode {
            "exists" => Mode::Exists,
            "count" => Mode::Count,
            "enumerate" => Mode::Enumerate,
            m => return Err(format!("unknown mode '{}'", m)),
        };
        let g = api::WireGraph::parse(graph_json)?;
        let faces = api::wire_to_faces(&g)?;

        let lattice: *mut Lattice = Box::into_raw(Box::new(Lattice::from_faces(faces)));
        // SAFETY: the iterator borrows the leaked lattice; it is dropped
        // before the lattice in `Drop`, and `lattice` is never moved.
        let iter: Box<dyn Iterator<Item = Strip>> =
            Box::new(rhombic::strips(unsafe { &*lattice }, cyclic));

        Ok(StripEnumerator { lattice, iter: Some(iter), cyclic, mode, count: 0, done: false })
    }
}

impl Drop for StripEnumerator {
    fn drop(&mut self) {
        self.iter = None; // the borrower goes first
        if !self.lattice.is_null() {
            // SAFETY: allocated with Box::into_raw in `create`, dropped once.
            unsafe { drop(Box::from_raw(self.lattice)) };
            self.lattice = std::ptr::null_mut();
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // Drive the real search path (parse -> faces -> rhombic::strips ->
    // plotting::edges_strip) the way the Web Worker does, and check we get a strip with a
    // non-empty, well-formed rhombic skeleton.
    fn run(gen_json: &str, cyclic: bool) -> serde_json::Value {
        let mut en = StripEnumerator::create(gen_json, cyclic, "enumerate")
            .expect("enumerator builds");
        let out = en.step(5000.0, 4);
        serde_json::from_str(&out).expect("step returns JSON")
    }

    fn check_strip(v: &serde_json::Value, n_nodes: usize) {
        let strips = v["strips"].as_array().expect("strips array");
        assert!(!strips.is_empty(), "expected at least one strip");
        let s = &strips[0];
        let layers = s["layers"].as_array().expect("layers");
        assert!(!layers.is_empty(), "strip has layers");
        // Edge *content* comes from plotting::edges_strip (real in your crate;
        // an empty stub here), so we don't assert it's non-empty. We do check
        // that whatever edges appear are well-formed node indices.
        let edges = s["edges"].as_array().expect("edges");
        for e in edges.iter().chain(s["cyclicEdges"].as_array().unwrap()) {
            let a = e[0].as_u64().unwrap() as usize;
            let b = e[1].as_u64().unwrap() as usize;
            assert!(a < n_nodes && b < n_nodes, "edge endpoints in range");
            assert_ne!(a, b, "no self-loops");
        }
    }

    #[test]
    fn cube2_enumerates_valid_strip() {
        // square face lattice: 9 faces (4 verts, 4 edges, 1 face)
        let g = api::gen_cube(2).expect("gen_cube");
        let n = api::WireGraph::parse(&g).unwrap().labels.len();
        assert_eq!(n, 9);
        check_strip(&run(&g, false), n);
    }

    #[test]
    fn simplex3_enumerates_valid_strip() {
        let g = api::gen_simplex(3).expect("gen_simplex");
        let n = api::WireGraph::parse(&g).unwrap().labels.len();
        check_strip(&run(&g, false), n);
    }

    #[test]
    fn count_mode_matches_native_count_strips() {
        // The browser's "count" mode drains the sequential iterator; it must
        // agree with the native parallel count_strips on the same lattice.
        let g = api::gen_cube(2).expect("gen_cube");
        let faces = api::wire_to_faces(&api::WireGraph::parse(&g).unwrap()).unwrap();
        let lat = Lattice::from_faces(faces);
        let native = rhombic::count_strips(&lat, false);

        let mut en = StripEnumerator::create(&g, false, "count").unwrap();
        let mut guard = 0;
        loop {
            let v: serde_json::Value =
                serde_json::from_str(&en.step(5000.0, 0)).unwrap();
            if v["done"].as_bool().unwrap() {
                assert_eq!(v["count"].as_u64().unwrap() as usize, native);
                break;
            }
            guard += 1;
            assert!(guard < 1000, "count did not terminate");
        }
    }
}
