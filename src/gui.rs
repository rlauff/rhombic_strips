//! Interactive lattice editor and rhombic strip explorer.
//!
//! Structure:
//! * [`PosetGraph`] — the editable model (nodes + relations). In poset mode
//!   the relations are cover relations `(lower, upper)`; in graph mode they
//!   are undirected edges of a plain graph.
//! * Generators — products of chains, digit-label relation inference, the
//!   distributive lattice J(P), face lattices of cubes and simplices, and
//!   graph associahedra: the poset of tubes under inclusion and the full
//!   face lattice of tubings (Carr–Devadoss nested set complex).
//! * A background worker thread streaming strips over a bounded channel, so
//!   the UI never blocks and "next strip" advances a lazy enumeration.
//! * The egui app: pan/zoom canvas, node dragging & renaming, click-click
//!   relations, undo, strip overlay with prev/next navigation and layer
//!   readout, TikZ export, PDF rendering, file load/save.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::lattice::{Face, FaceId, Lattice};
use crate::plotting;
use crate::rhombic::{self, Strip};

// ===========================================================================
// Model: an editable poset / graph diagram
// ===========================================================================

type NodeId = usize;

#[derive(Clone)]
struct Node {
    id: NodeId,
    label: String,
    pos: Pos2, // world space
}

#[derive(Default, Clone)]
struct PosetGraph {
    nodes: Vec<Node>,
    /// Poset mode: cover relations `(lower, upper)`. Graph mode: undirected edges.
    edges: Vec<(NodeId, NodeId)>,
    next_id: NodeId,
}

impl PosetGraph {
    fn add_node(&mut self, label: String, pos: Pos2) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        let label = if label.is_empty() { id.to_string() } else { label };
        self.nodes.push(Node { id, label, pos });
        id
    }

    fn remove_node(&mut self, id: NodeId) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|&(a, b)| a != id && b != id);
    }

    fn index_of(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn label_of(&self, id: NodeId) -> &str {
        self.node(id).map_or("?", |n| n.label.as_str())
    }

    /// Add the relation/edge `(a, b)`, or remove it if already present in
    /// either orientation. Returns true if added, false if removed.
    fn toggle_edge(&mut self, a: NodeId, b: NodeId) -> bool {
        if let Some(i) = self.edges.iter().position(|&e| e == (a, b) || e == (b, a)) {
            self.edges.remove(i);
            false
        } else {
            self.edges.push((a, b));
            true
        }
    }

    /// Longest-path rank of every node (minimal elements have rank 0).
    /// Errors if the relation is cyclic.
    fn ranks(&self) -> Result<HashMap<NodeId, usize>, String> {
        let idx: HashMap<NodeId, usize> =
            self.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let n = self.nodes.len();
        let mut succ: Vec<Vec<usize>> = vec![vec![]; n];
        let mut indeg = vec![0usize; n];
        for &(a, b) in &self.edges {
            let (Some(&ia), Some(&ib)) = (idx.get(&a), idx.get(&b)) else { continue };
            succ[ia].push(ib);
            indeg[ib] += 1;
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
        Ok(self.nodes.iter().enumerate().map(|(i, node)| (node.id, rank[i])).collect())
    }

    /// Convert to faces for `Lattice::from_faces`. Returns the faces and the
    /// mapping `FaceId -> NodeId` (faces are in node order).
    fn to_faces(&self) -> Result<(Vec<Face>, Vec<NodeId>), String> {
        let ranks = self.ranks()?;
        let idx: HashMap<NodeId, usize> =
            self.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();

        let mut upsets: Vec<Vec<FaceId>> = vec![vec![]; self.nodes.len()];
        let mut downsets: Vec<Vec<FaceId>> = vec![vec![]; self.nodes.len()];
        for &(a, b) in &self.edges {
            let (Some(&ia), Some(&ib)) = (idx.get(&a), idx.get(&b)) else { continue };
            upsets[ia].push(ib);
            downsets[ib].push(ia);
        }

        let faces = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                Face::new(
                    node.label.clone(),
                    ranks[&node.id],
                    std::mem::take(&mut upsets[i]),
                    std::mem::take(&mut downsets[i]),
                )
            })
            .collect();
        let id_map = self.nodes.iter().map(|n| n.id).collect();
        Ok((faces, id_map))
    }

    /// Rebuild the diagram from a lattice (used for file loading).
    fn from_lattice(l: &Lattice) -> Self {
        let mut g = PosetGraph::default();
        for (_, face) in l.faces() {
            g.add_node(face.label().to_string(), Pos2::ZERO);
        }
        for (id, face) in l.faces() {
            for &d in face.downset() {
                g.edges.push((g.nodes[d].id, g.nodes[id].id));
            }
        }
        g.layout_by_rank();
        g
    }

    // -- layouts ---------------------------------------------------------------

    /// Hasse-diagram layout: rows by rank, centered.
    fn layout_by_rank(&mut self) {
        let Ok(ranks) = self.ranks() else { return };
        let mut rows: HashMap<usize, Vec<NodeId>> = HashMap::new();
        for node in &self.nodes {
            rows.entry(ranks[&node.id]).or_default().push(node.id);
        }
        let (x_step, y_step) = (90.0, 90.0);
        for (rank, ids) in rows {
            let row_width = (ids.len() as f32 - 1.0) * x_step;
            for (k, id) in ids.into_iter().enumerate() {
                let pos = egui::pos2(
                    -row_width / 2.0 + k as f32 * x_step,
                    -(rank as f32) * y_step,
                );
                if let Some(i) = self.index_of(id) {
                    self.nodes[i].pos = pos;
                }
            }
        }
    }

    /// Circle layout (for graph mode).
    fn layout_circle(&mut self) {
        let n = self.nodes.len();
        if n == 0 {
            return;
        }
        let radius = 40.0 + 18.0 * n as f32;
        for (i, node) in self.nodes.iter_mut().enumerate() {
            let angle = std::f32::consts::TAU * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
            node.pos = egui::pos2(radius * angle.cos(), radius * angle.sin());
        }
    }

    // -- poset generators --------------------------------------------------------

    /// Product of chains. `"211"` gives C3 x C2 x C2 (per-digit); inputs with
    /// separators like `"12,3"` allow multi-digit chain lengths.
    fn grid(input: &str) -> Option<Self> {
        let dims: Vec<u32> = if input.chars().all(|c| c.is_ascii_digit()) {
            input.chars().filter_map(|c| c.to_digit(10)).collect()
        } else {
            input
                .split(|c: char| !c.is_ascii_digit())
                .filter(|t| !t.is_empty())
                .filter_map(|t| t.parse().ok())
                .collect()
        };
        if dims.is_empty() {
            return None;
        }

        // all points of the box [0, d_0] x ... x [0, d_k]
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
        let mut g = PosetGraph::default();
        for p in &points {
            let label = p.iter().map(u32::to_string).collect::<Vec<_>>().join(sep);
            g.add_node(label, Pos2::ZERO);
        }
        // covers: differ by +1 in exactly one coordinate
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
                    g.edges.push((g.nodes[i].id, g.nodes[j].id));
                }
            }
        }
        g.layout_by_rank();
        Some(g)
    }

    /// Face lattice of the d-cube (without the empty face): all words over
    /// {0, 1, *}, covers replace one fixed coordinate by *.
    fn cube_lattice(d: usize) -> Result<Self, String> {
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

        let mut g = PosetGraph::default();
        for f in &faces {
            g.add_node(f.clone(), Pos2::ZERO);
        }
        for (i, f) in faces.iter().enumerate() {
            for (k, c) in f.chars().enumerate() {
                if c != '*' {
                    let mut upper: Vec<char> = f.chars().collect();
                    upper[k] = '*';
                    let j = idx[&upper.into_iter().collect::<String>()];
                    g.edges.push((g.nodes[i].id, g.nodes[j].id));
                }
            }
        }
        g.layout_by_rank();
        Ok(g)
    }

    /// Face lattice of the d-simplex (without the empty face): nonempty
    /// subsets of {0, ..., d}, covers add one element.
    fn simplex_lattice(d: usize) -> Result<Self, String> {
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
        let mut g = PosetGraph::default();
        for &m in &masks {
            g.add_node(label(m), Pos2::ZERO);
        }
        for (i, &m) in masks.iter().enumerate() {
            for v in 0..n {
                if (m >> v) & 1 == 0 {
                    let j = idx[&(m | (1 << v))];
                    g.edges.push((g.nodes[i].id, g.nodes[j].id));
                }
            }
        }
        g.layout_by_rank();
        Ok(g)
    }

    /// Infer cover relations from all-digit labels: same length, digit sums
    /// differing by one, and exactly one differing position.
    fn infer_digit_relations(&mut self) -> usize {
        let digit_sum = |s: &str| -> Option<i64> {
            s.chars()
                .map(|c| c.to_digit(10).map(|d| d as i64))
                .sum::<Option<i64>>()
        };
        let mut added = 0;
        for i in 0..self.nodes.len() {
            for j in 0..self.nodes.len() {
                if i == j {
                    continue;
                }
                let (a, b) = (&self.nodes[i], &self.nodes[j]);
                if a.label.len() != b.label.len() {
                    continue;
                }
                let (Some(sa), Some(sb)) = (digit_sum(&a.label), digit_sum(&b.label)) else {
                    continue;
                };
                if sb != sa + 1 {
                    continue;
                }
                let diff = a
                    .label
                    .chars()
                    .zip(b.label.chars())
                    .filter(|(x, y)| x != y)
                    .count();
                if diff == 1 && !self.edges.contains(&(a.id, b.id)) {
                    self.edges.push((a.id, b.id));
                    added += 1;
                }
            }
        }
        added
    }

    /// The distributive lattice J(P) of order ideals of the current poset.
    fn distributive(&self) -> Result<Self, String> {
        let n = self.nodes.len();
        if n > 20 {
            return Err(format!("Poset too large ({} > 20 elements) for J(P).", n));
        }
        let idx: HashMap<NodeId, usize> =
            self.nodes.iter().enumerate().map(|(i, node)| (node.id, i)).collect();
        let mut down_mask = vec![0u64; n]; // direct lower covers as bitmask
        for &(a, b) in &self.edges {
            let (Some(&ia), Some(&ib)) = (idx.get(&a), idx.get(&b)) else { continue };
            down_mask[ib] |= 1 << ia;
        }

        // enumerate ideals: subsets closed under going down (covers suffice)
        let mut ideals: Vec<u64> = Vec::new();
        for mask in 0u64..(1 << n) {
            let ok = (0..n).all(|i| (mask >> i) & 1 == 0 || (mask & down_mask[i]) == down_mask[i]);
            if ok {
                ideals.push(mask);
            }
        }

        let mut g = PosetGraph::default();
        for &mask in &ideals {
            let mut labels: Vec<&str> = (0..n)
                .filter(|&i| (mask >> i) & 1 == 1)
                .map(|i| self.nodes[i].label.as_str())
                .collect();
            labels.sort_unstable();
            let label = if labels.is_empty() {
                "0".to_string()
            } else {
                format!("{{{}}}", labels.join(","))
            };
            g.add_node(label, Pos2::ZERO);
        }
        for (i, &ma) in ideals.iter().enumerate() {
            for (j, &mb) in ideals.iter().enumerate() {
                if mb.count_ones() == ma.count_ones() + 1 && (ma & mb) == ma {
                    g.edges.push((g.nodes[i].id, g.nodes[j].id));
                }
            }
        }
        g.layout_by_rank();
        Ok(g)
    }

    // -- graph generators (graph mode) ------------------------------------------

    fn graph_path(n: usize) -> Self {
        let mut g = Self::graph_with_vertices(n);
        for i in 0..n.saturating_sub(1) {
            g.edges.push((g.nodes[i].id, g.nodes[i + 1].id));
        }
        for (i, node) in g.nodes.iter_mut().enumerate() {
            node.pos = egui::pos2(i as f32 * 80.0 - (n as f32 - 1.0) * 40.0, 0.0);
        }
        g
    }

    fn graph_cycle(n: usize) -> Self {
        let mut g = Self::graph_with_vertices(n);
        for i in 0..n {
            g.edges.push((g.nodes[i].id, g.nodes[(i + 1) % n].id));
        }
        g.layout_circle();
        g
    }

    fn graph_complete(n: usize) -> Self {
        let mut g = Self::graph_with_vertices(n);
        for i in 0..n {
            for j in (i + 1)..n {
                g.edges.push((g.nodes[i].id, g.nodes[j].id));
            }
        }
        g.layout_circle();
        g
    }

    /// Star K_{1,n-1}: vertex 0 in the center.
    fn graph_star(n: usize) -> Self {
        let mut g = Self::graph_with_vertices(n);
        for i in 1..n {
            g.edges.push((g.nodes[0].id, g.nodes[i].id));
        }
        g.layout_circle();
        if let Some(center) = g.nodes.first_mut() {
            center.pos = Pos2::ZERO;
        }
        g
    }

    fn graph_with_vertices(n: usize) -> Self {
        let mut g = PosetGraph::default();
        for i in 0..n {
            g.add_node(i.to_string(), Pos2::ZERO);
        }
        g
    }

    // -- persistence -----------------------------------------------------------

    /// Serialize in the lattice file format (`dim: label: {upset}, {downset}`).
    fn to_lattice_file(&self) -> Result<String, String> {
        let (faces, _) = self.to_faces()?;
        let fmt_set = |s: &[FaceId]| {
            s.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
        };
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
}

// ===========================================================================
// Graph associahedra: tubes and tubings (Carr–Devadoss)
// ===========================================================================

type Mask = u64;

/// Adjacency of a drawn graph as one neighbour bitmask per vertex
/// (vertices in node order).
fn adjacency_masks(g: &PosetGraph) -> Vec<Mask> {
    let idx: HashMap<NodeId, usize> =
        g.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let mut adj = vec![0u64; g.nodes.len()];
    for &(a, b) in &g.edges {
        let (Some(&ia), Some(&ib)) = (idx.get(&a), idx.get(&b)) else { continue };
        if ia != ib {
            adj[ia] |= 1 << ib;
            adj[ib] |= 1 << ia;
        }
    }
    adj
}

/// Does `mask` induce a connected subgraph?
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

/// Vertices outside `mask` adjacent to it.
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

/// Two tubes are compatible iff they are nested, or disjoint and non-adjacent.
fn tubes_compatible(a: Mask, b: Mask, adj: &[Mask]) -> bool {
    if a & b != 0 {
        let u = a | b;
        u == a || u == b
    } else {
        mask_neighbors(a, adj) & b == 0
    }
}

/// All tubings (sets of pairwise compatible tubes, including the empty one),
/// as sorted index vectors into `tubes`. Errors above `cap` tubings.
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

/// Label of a tube: the labels of its vertices, concatenated (or
/// comma-separated when labels are longer than one character).
fn tube_label(mask: Mask, vertex_labels: &[&str]) -> String {
    let single = vertex_labels.iter().all(|l| l.chars().count() == 1);
    let parts: Vec<&str> = (0..vertex_labels.len())
        .filter(|&v| (mask >> v) & 1 == 1)
        .map(|v| vertex_labels[v])
        .collect();
    parts.join(if single { "" } else { "," })
}

fn checked_graph(g: &PosetGraph, max_n: usize) -> Result<(usize, Vec<Mask>, Mask), String> {
    let n = g.nodes.len();
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

/// The poset of tubes of the drawn graph under inclusion. Includes the
/// singletons at the bottom and the full vertex set at the top; graded by
/// cardinality, covers add one vertex.
fn tube_poset(g: &PosetGraph) -> Result<PosetGraph, String> {
    let (_, adj, full) = checked_graph(g, 10)?;
    let mut subs: Vec<Mask> = (1..=full).filter(|&m| mask_connected(m, &adj)).collect();
    subs.sort_by_key(|m| (m.count_ones(), *m));

    let vertex_labels: Vec<&str> = g.nodes.iter().map(|n| n.label.as_str()).collect();
    let mut out = PosetGraph::default();
    for &m in &subs {
        out.add_node(tube_label(m, &vertex_labels), Pos2::ZERO);
    }
    for (i, &a) in subs.iter().enumerate() {
        for (j, &b) in subs.iter().enumerate() {
            if b.count_ones() == a.count_ones() + 1 && a & b == a {
                out.edges.push((out.nodes[i].id, out.nodes[j].id));
            }
        }
    }
    out.layout_by_rank();
    Ok(out)
}

/// The face lattice of the graph associahedron of the drawn graph: faces are
/// tubings ordered by reverse inclusion. Vertices of the polytope = maximal
/// tubings; the empty tubing (label `*`) is the full polytope on top.
/// Path -> associahedron, complete graph -> permutahedron, cycle ->
/// cyclohedron, star -> stellahedron.
fn graph_associahedron(g: &PosetGraph) -> Result<PosetGraph, String> {
    let (_, adj, full) = checked_graph(g, 12)?;
    // proper connected subsets
    let tubes: Vec<Mask> = (1..full).filter(|&m| mask_connected(m, &adj)).collect();
    let tubings = enumerate_tubings(&tubes, &adj, 20_000)?;

    let vertex_labels: Vec<&str> = g.nodes.iter().map(|n| n.label.as_str()).collect();
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

    let mut out = PosetGraph::default();
    let mut index: HashMap<Vec<usize>, usize> = HashMap::new();
    for (i, t) in tubings.iter().enumerate() {
        out.add_node(tubing_label(t), Pos2::ZERO);
        index.insert(t.clone(), i); // tubings come as sorted index vectors
    }
    // face(T) is covered by face(T \ {t}): removing a tube goes one dim up
    for (i, t) in tubings.iter().enumerate() {
        for k in 0..t.len() {
            let mut sup = t.clone();
            sup.remove(k);
            let j = index[&sup];
            out.edges.push((out.nodes[i].id, out.nodes[j].id));
        }
    }
    out.layout_by_rank();
    Ok(out)
}

// ===========================================================================
// Background worker: streams strips over a bounded channel
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
enum JobKind {
    Exists,    // stop after the first strip (shown as witness)
    Count,     // count only, with live progress
    Enumerate, // stream all strips for browsing
}

enum WorkerMsg {
    Strip {
        layers: Strip,
        edges: Vec<(FaceId, FaceId)>,
        cyclic_edges: Vec<(FaceId, FaceId)>,
    },
    Progress(usize),
    Done(usize),
}

struct Job {
    kind: JobKind,
    rx: mpsc::Receiver<WorkerMsg>,
    cancel: Arc<AtomicBool>,
    /// FaceId (index into the worker's lattice) -> NodeId in the editor.
    id_map: Vec<NodeId>,
    started: Instant,
    live_count: usize,
}

impl Job {
    fn spawn(faces: Vec<Face>, id_map: Vec<NodeId>, cyclic: bool, kind: JobKind) -> Self {
        let (tx, rx) = mpsc::sync_channel::<WorkerMsg>(64); // backpressure
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();

        std::thread::spawn(move || {
            let l = Lattice::from_faces(faces);
            let mut n = 0usize;
            for strip in rhombic::strips(&l, cyclic) {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                n += 1;
                match kind {
                    JobKind::Count => {
                        if n % 1024 == 0 {
                            let _ = tx.try_send(WorkerMsg::Progress(n));
                        }
                    }
                    JobKind::Exists | JobKind::Enumerate => {
                        let (edges, cyclic_edges) = plotting::edges_strip(&strip, &l, cyclic);
                        if tx
                            .send(WorkerMsg::Strip { layers: strip, edges, cyclic_edges })
                            .is_err()
                        {
                            return; // receiver dropped
                        }
                        if kind == JobKind::Exists {
                            break;
                        }
                    }
                }
            }
            let _ = tx.send(WorkerMsg::Done(n));
        });

        Job { kind, rx, cancel, id_map, started: Instant::now(), live_count: 0 }
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A strip translated into editor node ids, ready to draw.
struct StripView {
    layers: Vec<Vec<NodeId>>,
    edges: Vec<(NodeId, NodeId)>,
    cyclic_edges: Vec<(NodeId, NodeId)>,
}

// ===========================================================================
// The application
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
enum EditMode {
    Poset,
    Graph,
}

pub struct LatticeApp {
    graph: PosetGraph,
    mode: EditMode,

    // viewport
    view_offset: Vec2,
    view_scale: f32,
    canvas_rect: Rect,

    // edit state
    edge_start: Option<NodeId>,
    hovered_node: Option<NodeId>, // from last frame, for highlighting
    rename: Option<(NodeId, String)>,
    undo_stack: Vec<PosetGraph>,
    label_input: String,
    grid_input: String,
    file_path: String,
    example_n: usize,
    cyclic: bool,
    log: String,

    // computation
    job: Option<Job>,
    strips: Vec<StripView>,
    strip_cursor: usize,
    total_strips: Option<usize>, // known once a job finished
    viewing_strip: bool,
}

impl LatticeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            graph: PosetGraph::default(),
            mode: EditMode::Poset,
            view_offset: egui::vec2(500.0, 400.0),
            view_scale: 1.0,
            canvas_rect: Rect::from_min_size(egui::pos2(260.0, 0.0), egui::vec2(940.0, 800.0)),
            edge_start: None,
            hovered_node: None,
            rename: None,
            undo_stack: Vec::new(),
            label_input: String::new(),
            grid_input: String::new(),
            file_path: "lattice.txt".to_string(),
            example_n: 3,
            cyclic: false,
            log: "Welcome. Double-click the canvas to add nodes, click two nodes to relate them."
                .to_string(),
            job: None,
            strips: Vec::new(),
            strip_cursor: 0,
            total_strips: None,
            viewing_strip: false,
        }
    }

    // -- coordinate transforms -------------------------------------------------

    fn to_screen(&self, world: Pos2) -> Pos2 {
        (world.to_vec2() * self.view_scale + self.view_offset).to_pos2()
    }

    fn to_world(&self, screen: Pos2) -> Pos2 {
        ((screen.to_vec2() - self.view_offset) / self.view_scale).to_pos2()
    }

    /// Fit the viewport to the bounding box of all nodes.
    fn fit_view(&mut self) {
        if self.graph.nodes.is_empty() {
            return;
        }
        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for n in &self.graph.nodes {
            min_x = min_x.min(n.pos.x);
            max_x = max_x.max(n.pos.x);
            min_y = min_y.min(n.pos.y);
            max_y = max_y.max(n.pos.y);
        }
        let (bw, bh) = ((max_x - min_x).max(1.0), (max_y - min_y).max(1.0));
        let rect = self.canvas_rect;
        let margin = 120.0;
        self.view_scale = ((rect.width() - margin) / bw)
            .min((rect.height() - margin) / bh)
            .clamp(0.05, 2.5);
        let center = egui::vec2((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
        self.view_offset = rect.center().to_vec2() - center * self.view_scale;
    }

    // -- undo -------------------------------------------------------------------

    fn push_undo(&mut self) {
        self.undo_stack.push(self.graph.clone());
        if self.undo_stack.len() > 50 {
            self.undo_stack.remove(0);
        }
    }

    fn undo(&mut self) {
        if let Some(g) = self.undo_stack.pop() {
            self.invalidate_results();
            self.graph = g;
            self.log = "Undone.".to_string();
        } else {
            self.log = "Nothing to undo.".to_string();
        }
    }

    /// Replace the whole graph (generators, load, ...), with undo.
    fn replace_graph(&mut self, g: PosetGraph, mode: EditMode, log: String) {
        self.push_undo();
        self.invalidate_results();
        self.graph = g;
        self.mode = mode;
        self.log = log;
        self.fit_view();
    }

    // -- job handling ------------------------------------------------------------

    /// Any structural change invalidates running jobs and cached strips,
    /// since face ids refer to the graph at job start.
    fn invalidate_results(&mut self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        self.job = None;
        self.strips.clear();
        self.strip_cursor = 0;
        self.total_strips = None;
        self.viewing_strip = false;
        self.edge_start = None;
    }

    fn start_job(&mut self, kind: JobKind) {
        self.invalidate_results();
        match self.graph.to_faces() {
            Ok((faces, id_map)) => {
                self.log = match kind {
                    JobKind::Exists => "Checking existence...".to_string(),
                    JobKind::Count => "Counting strips...".to_string(),
                    JobKind::Enumerate => "Enumerating strips...".to_string(),
                };
                self.job = Some(Job::spawn(faces, id_map, self.cyclic, kind));
            }
            Err(e) => self.log = e,
        }
    }

    /// Drain worker messages. For enumeration we only pull a small lookahead
    /// past the cursor, so the bounded channel throttles the worker and
    /// memory stays proportional to how far the user has browsed.
    fn poll_job(&mut self) {
        let Some(mut job) = self.job.take() else { return };
        let mut finished = false;

        loop {
            if job.kind == JobKind::Enumerate && self.strips.len() >= self.strip_cursor + 8 {
                break;
            }
            match job.rx.try_recv() {
                Ok(WorkerMsg::Strip { layers, edges, cyclic_edges }) => {
                    let map = |f: FaceId| job.id_map.get(f).copied();
                    let view = StripView {
                        layers: layers
                            .iter()
                            .map(|l| l.iter().filter_map(|&f| map(f)).collect())
                            .collect(),
                        edges: edges
                            .iter()
                            .filter_map(|&(a, b)| Some((map(a)?, map(b)?)))
                            .collect(),
                        cyclic_edges: cyclic_edges
                            .iter()
                            .filter_map(|&(a, b)| Some((map(a)?, map(b)?)))
                            .collect(),
                    };
                    let first = self.strips.is_empty();
                    self.strips.push(view);
                    if first {
                        self.strip_cursor = 0;
                        self.viewing_strip = true;
                        self.arrange_as_strip(0);
                    }
                    if job.kind == JobKind::Exists {
                        self.log = format!(
                            "A rhombic strip EXISTS ({:.1?}) — shown below.",
                            job.started.elapsed()
                        );
                        job.cancel();
                        finished = true;
                        break;
                    }
                }
                Ok(WorkerMsg::Progress(n)) => job.live_count = n,
                Ok(WorkerMsg::Done(n)) => {
                    self.total_strips = Some(n);
                    self.log = match job.kind {
                        JobKind::Exists => {
                            if n == 0 {
                                format!("No rhombic strip exists ({:.1?}).", job.started.elapsed())
                            } else {
                                self.log.clone()
                            }
                        }
                        JobKind::Count => {
                            format!("{} rhombic strips ({:.1?}).", n, job.started.elapsed())
                        }
                        JobKind::Enumerate => {
                            format!(
                                "Enumeration finished: {} strips ({:.1?}).",
                                n,
                                job.started.elapsed()
                            )
                        }
                    };
                    finished = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }

        if !finished {
            self.job = Some(job);
        }
    }

    /// Layered layout of the currently displayed strip.
    fn arrange_as_strip(&mut self, strip_idx: usize) {
        let Some(view) = self.strips.get(strip_idx) else { return };
        let (x_step, y_step) = (90.0, 100.0);
        let positions: Vec<(NodeId, Pos2)> = view
            .layers
            .iter()
            .enumerate()
            .flat_map(|(layer_idx, layer)| {
                let count = layer.len() as f32;
                layer.iter().enumerate().map(move |(i, &id)| {
                    let x = (i as f32 - (count - 1.0) / 2.0) * x_step;
                    let y = -(layer_idx as f32) * y_step;
                    (id, egui::pos2(x, y))
                })
            })
            .collect();
        for (id, pos) in positions {
            if let Some(i) = self.graph.index_of(id) {
                self.graph.nodes[i].pos = pos;
            }
        }
        self.fit_view();
    }

    fn current_strip(&self) -> Option<&StripView> {
        self.viewing_strip
            .then(|| self.strips.get(self.strip_cursor))
            .flatten()
    }

    /// Render the displayed strip through plotting::show_strip (pdflatex).
    fn render_strip_pdf(&mut self) {
        let Some(view) = self.current_strip() else {
            self.log = "No strip displayed.".to_string();
            return;
        };
        match self.graph.to_faces() {
            Ok((faces, id_map)) => {
                let rev: HashMap<NodeId, FaceId> =
                    id_map.iter().enumerate().map(|(f, &id)| (id, f)).collect();
                let strip: Option<Strip> = view
                    .layers
                    .iter()
                    .map(|layer| {
                        layer.iter().map(|id| rev.get(id).copied()).collect::<Option<Vec<_>>>()
                    })
                    .collect();
                match strip {
                    Some(s) => {
                        let l = Lattice::from_faces(faces);
                        plotting::show_strip(&s, &l, self.cyclic);
                        self.log = "Strip rendered (see strip_visualization*.pdf).".to_string();
                    }
                    None => self.log = "Graph changed since the strip was computed.".to_string(),
                }
            }
            Err(e) => self.log = e,
        }
    }

    // -- TikZ export ---------------------------------------------------------------

    fn export_tikz(&mut self) {
        let strip = self.current_strip();

        // which nodes/edges to export
        let (node_ids, edges): (Vec<NodeId>, Vec<(NodeId, NodeId)>) = match strip {
            Some(v) => (
                v.layers.iter().flatten().copied().collect(),
                v.edges.iter().chain(v.cyclic_edges.iter()).copied().collect(),
            ),
            None => (
                self.graph.nodes.iter().map(|n| n.id).collect(),
                self.graph.edges.clone(),
            ),
        };

        let safe = |s: &str| {
            s.replace(['{', '}'], "")
                .replace([',', '|'], "_")
                .replace('*', "top")
                .replace('?', "empty")
        };

        let mut tex = String::new();
        tex.push_str("\\documentclass[tikz, border=1cm]{standalone}\n\\begin{document}\n");
        tex.push_str("\\begin{tikzpicture}[y=-1cm]\n\n% Coordinates\n");
        let scale = 0.02;
        for &id in &node_ids {
            let Some(node) = self.graph.node(id) else { continue };
            tex.push_str(&format!(
                "\\coordinate ({}) at ({:.2}, {:.2});\n",
                safe(&node.label),
                node.pos.x * scale,
                node.pos.y * scale
            ));
        }

        tex.push_str("\n% Edges\n\\foreach \\a/\\b in {");
        let edge_strs: Vec<String> = edges
            .iter()
            .filter_map(|&(a, b)| {
                let (na, nb) = (self.graph.node(a)?, self.graph.node(b)?);
                Some(format!("{}/{}", safe(&na.label), safe(&nb.label)))
            })
            .collect();
        tex.push_str(&edge_strs.join(", "));
        tex.push_str("} {\n    \\draw (\\a) -- (\\b);\n}\n\n% Nodes\n\\foreach \\v/\\l in {");
        let label_strs: Vec<String> = node_ids
            .iter()
            .filter_map(|&id| {
                let n = self.graph.node(id)?;
                Some(format!("{}/{{{}}}", safe(&n.label), n.label.replace('|', "$|$")))
            })
            .collect();
        tex.push_str(&label_strs.join(", "));
        tex.push_str(
            "} {\n    \\node[draw, circle, fill=white, inner sep=2pt] at (\\v) {\\footnotesize \\l};\n}\n",
        );
        tex.push_str("\\end{tikzpicture}\n\\end{document}\n");

        self.log = match std::fs::write("lattice_output.tex", tex) {
            Ok(_) => "Exported to lattice_output.tex".to_string(),
            Err(e) => format!("Export failed: {}", e),
        };
    }

    // -- UI panels -------------------------------------------------------------------

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Lattice Builder");
        ui.label(format!(
            "{} nodes, {} {}",
            self.graph.nodes.len(),
            self.graph.edges.len(),
            if self.mode == EditMode::Poset { "relations" } else { "edges" }
        ));

        let prev_mode = self.mode;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, EditMode::Poset, "Poset editor");
            ui.selectable_value(&mut self.mode, EditMode::Graph, "Graph editor");
        });
        if prev_mode != self.mode {
            self.invalidate_results();
        }
        ui.separator();

        // --- editing ---
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.label_input)
                .on_hover_text("Label for the next node (empty = numbered)");
            if ui.button("Add node").clicked() {
                self.push_undo();
                self.invalidate_results();
                let center = self.to_world(self.canvas_rect.center());
                let id = self.graph.add_node(self.label_input.trim().to_string(), center);
                self.label_input.clear();
                self.log = format!("Added node {}.", id);
            }
        });
        ui.horizontal(|ui| {
            if self.mode == EditMode::Poset {
                if ui.button("Arrange by rank").clicked() {
                    self.graph.layout_by_rank();
                    self.fit_view();
                }
            } else if ui.button("Circle layout").clicked() {
                self.graph.layout_circle();
                self.fit_view();
            }
            if ui.button("Fit view").clicked() {
                self.fit_view();
            }
            if ui.button("Undo").clicked() {
                self.undo();
            }
        });
        ui.separator();

        // --- examples ---
        ui.collapsing("Examples", |ui| {
            ui.horizontal(|ui| {
                ui.label("n =");
                ui.add(egui::DragValue::new(&mut self.example_n).range(1..=12));
            });

            ui.label("Face lattices:");
            ui.horizontal_wrapped(|ui| {
                if ui.button("n-cube").clicked() {
                    match PosetGraph::cube_lattice(self.example_n) {
                        Ok(g) => {
                            let msg = format!("{}-cube face lattice: {} faces.", self.example_n, g.nodes.len());
                            self.replace_graph(g, EditMode::Poset, msg);
                        }
                        Err(e) => self.log = e,
                    }
                }
                if ui.button("n-simplex").clicked() {
                    match PosetGraph::simplex_lattice(self.example_n) {
                        Ok(g) => {
                            let msg = format!("{}-simplex face lattice: {} faces.", self.example_n, g.nodes.len());
                            self.replace_graph(g, EditMode::Poset, msg);
                        }
                        Err(e) => self.log = e,
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                let assoc_of = |app: &mut Self, name: &str, g: PosetGraph| {
                    match graph_associahedron(&g) {
                        Ok(fl) => {
                            let msg = format!("{}: {} faces.", name, fl.nodes.len());
                            app.replace_graph(fl, EditMode::Poset, msg);
                        }
                        Err(e) => app.log = e,
                    }
                };
                if ui.button("Permutahedron").clicked() {
                    assoc_of(self, "Permutahedron (tubings of K_n)", PosetGraph::graph_complete(self.example_n));
                }
                if ui.button("Associahedron").clicked() {
                    assoc_of(self, "Associahedron (tubings of a path)", PosetGraph::graph_path(self.example_n));
                }
                if ui.button("Cyclohedron").clicked() {
                    assoc_of(self, "Cyclohedron (tubings of a cycle)", PosetGraph::graph_cycle(self.example_n));
                }
                if ui.button("Stellahedron").clicked() {
                    assoc_of(self, "Stellahedron (tubings of a star)", PosetGraph::graph_star(self.example_n));
                }
            });

            ui.label("Graphs (opens the graph editor):");
            ui.horizontal_wrapped(|ui| {
                let n = self.example_n;
                if ui.button("Path").clicked() {
                    self.replace_graph(PosetGraph::graph_path(n), EditMode::Graph, format!("Path on {} vertices.", n));
                }
                if ui.button("Cycle").clicked() {
                    self.replace_graph(PosetGraph::graph_cycle(n), EditMode::Graph, format!("Cycle on {} vertices.", n));
                }
                if ui.button("Complete").clicked() {
                    self.replace_graph(PosetGraph::graph_complete(n), EditMode::Graph, format!("K_{}.", n));
                }
                if ui.button("Star").clicked() {
                    self.replace_graph(PosetGraph::graph_star(n), EditMode::Graph, format!("Star K_(1,{}).", n.saturating_sub(1)));
                }
            });
        });
        ui.separator();

        match self.mode {
            EditMode::Graph => self.sidebar_graph_mode(ui),
            EditMode::Poset => self.sidebar_poset_mode(ui),
        }

        ui.separator();
        ui.heading("Log");
        ui.label(&self.log);

        ui.add_space(16.0);
        ui.collapsing("Controls", |ui| {
            ui.small(
                "Double-click canvas: add node\n\
                 Double-click node: rename it\n\
                 Click node, then another: toggle relation/edge\n\
                 Right-click node: delete it\n\
                 Right-click canvas / Esc: cancel relation\n\
                 Ctrl+Z: undo\n\
                 Drag node: move — drag canvas: pan — wheel: zoom",
            );
        });
    }

    /// Graph mode: convert the drawn graph into posets.
    fn sidebar_graph_mode(&mut self, ui: &mut egui::Ui) {
        ui.label("Graph associahedra:");
        if ui
            .button("Tube poset (inclusion)")
            .on_hover_text("Connected subsets of the drawn graph, ordered by inclusion")
            .clicked()
        {
            match tube_poset(&self.graph) {
                Ok(g) => {
                    let msg = format!("Tube poset: {} tubes.", g.nodes.len());
                    self.replace_graph(g, EditMode::Poset, msg);
                }
                Err(e) => self.log = e,
            }
        }
        if ui
            .button("Graph associahedron (tubings)")
            .on_hover_text(
                "Face lattice of the graph associahedron: tubings under reverse inclusion",
            )
            .clicked()
        {
            match graph_associahedron(&self.graph) {
                Ok(g) => {
                    let msg = format!("Graph associahedron face lattice: {} faces.", g.nodes.len());
                    self.replace_graph(g, EditMode::Poset, msg);
                }
                Err(e) => self.log = e,
            }
        }
    }

    /// Poset mode: generators, file handling, computation, strip navigation.
    fn sidebar_poset_mode(&mut self, ui: &mut egui::Ui) {
        // --- generators ---
        ui.label("Grid generator (product of chains):");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.grid_input)
                .on_hover_text("e.g. 211 for C3 x C2 x C2, or 12,3 for multi-digit");
            if ui.button("Create").clicked() {
                match PosetGraph::grid(self.grid_input.trim()) {
                    Some(g) => {
                        let msg = format!("Generated grid with {} elements.", g.nodes.len());
                        self.replace_graph(g, EditMode::Poset, msg);
                    }
                    None => self.log = "Could not parse grid dimensions.".to_string(),
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Infer digit relations").clicked() {
                self.push_undo();
                self.invalidate_results();
                let added = self.graph.infer_digit_relations();
                self.log = format!("Inferred {} new relations.", added);
            }
            if ui.button("J(P)").on_hover_text("Distributive lattice of order ideals").clicked() {
                match self.graph.distributive() {
                    Ok(g) => {
                        let msg = format!("J(P) has {} elements.", g.nodes.len());
                        self.replace_graph(g, EditMode::Poset, msg);
                    }
                    Err(e) => self.log = e,
                }
            }
        });
        ui.separator();

        // --- file ---
        ui.label("Lattice file:");
        ui.text_edit_singleline(&mut self.file_path);
        ui.horizontal(|ui| {
            if ui.button("Load").clicked() {
                match Lattice::from_file(&self.file_path) {
                    Ok(l) => {
                        let g = PosetGraph::from_lattice(&l);
                        let msg = format!("Loaded {} faces.", g.nodes.len());
                        self.replace_graph(g, EditMode::Poset, msg);
                    }
                    Err(e) => self.log = e,
                }
            }
            if ui.button("Save").clicked() {
                match self.graph.to_lattice_file() {
                    Ok(content) => {
                        self.log = match std::fs::write(&self.file_path, content) {
                            Ok(_) => format!("Saved to {}.", self.file_path),
                            Err(e) => format!("Save failed: {}", e),
                        }
                    }
                    Err(e) => self.log = e,
                }
            }
            if ui.button("Export TikZ").clicked() {
                self.export_tikz();
            }
        });
        ui.separator();

        // --- computation ---
        ui.checkbox(&mut self.cyclic, "Cyclic strips");
        ui.horizontal(|ui| {
            if ui.button("Existence").clicked() {
                self.start_job(JobKind::Exists);
            }
            if ui.button("Count").clicked() {
                self.start_job(JobKind::Count);
            }
            if ui.button("Enumerate").clicked() {
                self.start_job(JobKind::Enumerate);
            }
        });

        let mut cancel_clicked = false;
        if let Some(job) = &self.job {
            let n_strips = self.strips.len();
            ui.horizontal(|ui| {
                ui.spinner();
                let status = match job.kind {
                    JobKind::Count => format!("counted {} ...", job.live_count),
                    JobKind::Enumerate => format!("found {} ...", n_strips),
                    JobKind::Exists => "searching ...".to_string(),
                };
                ui.label(format!("{} ({:.0?})", status, job.started.elapsed()));
                cancel_clicked = ui.button("Cancel").clicked();
            });
        }
        if cancel_clicked {
            if let Some(job) = self.job.take() {
                job.cancel();
            }
            self.log = "Cancelled.".to_string();
        }

        // --- strip navigation ---
        if !self.strips.is_empty() {
            ui.separator();
            let total = match self.total_strips {
                Some(n) => format!("{}", n),
                None => format!("≥{}", self.strips.len()),
            };
            ui.label(format!("Strip {} of {}", self.strip_cursor + 1, total));
            ui.horizontal(|ui| {
                if ui.add_enabled(self.strip_cursor > 0, egui::Button::new("◀ Prev")).clicked() {
                    self.strip_cursor -= 1;
                    self.viewing_strip = true;
                    self.arrange_as_strip(self.strip_cursor);
                }
                let next_ok = self.strip_cursor + 1 < self.strips.len() || self.job.is_some();
                if ui.add_enabled(next_ok, egui::Button::new("Next ▶")).clicked()
                    && self.strip_cursor + 1 < self.strips.len()
                {
                    self.strip_cursor += 1;
                    self.viewing_strip = true;
                    self.arrange_as_strip(self.strip_cursor);
                }
                if ui.button("Arrange").clicked() {
                    self.arrange_as_strip(self.strip_cursor);
                }
                if self.viewing_strip {
                    if ui.button("Hide").clicked() {
                        self.viewing_strip = false;
                    }
                } else if ui.button("Show").clicked() {
                    self.viewing_strip = true;
                }
            });

            if self.viewing_strip {
                if ui.button("Render strip PDF").on_hover_text("Runs pdflatex").clicked() {
                    self.render_strip_pdf();
                }
                let mut copy_text: Option<String> = None;
                if let Some(view) = self.current_strip() {
                    ui.collapsing("Strip layers", |ui| {
                        let mut lines = Vec::with_capacity(view.layers.len());
                        for layer in &view.layers {
                            let labels: Vec<&str> =
                                layer.iter().map(|&id| self.graph.label_of(id)).collect();
                            let line = format!("[{}]", labels.join(", "));
                            ui.monospace(&line);
                            lines.push(line);
                        }
                        if ui.button("Copy layers").clicked() {
                            copy_text = Some(lines.join("\n"));
                        }
                    });
                }
                if let Some(text) = copy_text {
                    ui.ctx().copy_text(text);
                    self.log = "Layers copied to clipboard.".to_string();
                }
            }
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        self.canvas_rect = response.rect;

        let dark = ui.visuals().dark_mode;
        let bg = if dark { Color32::from_gray(24) } else { Color32::WHITE };
        let fg = if dark { Color32::from_gray(220) } else { Color32::BLACK };
        let faint = if dark { Color32::from_gray(70) } else { Color32::from_gray(200) };
        let accent = Color32::from_rgb(230, 97, 0);
        let select = Color32::from_rgb(86, 180, 233);
        painter.rect_filled(response.rect, 0.0, bg);

        // zoom towards pointer
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                let factor = if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 };
                if let Some(mouse) = response.hover_pos() {
                    let world = self.to_world(mouse);
                    self.view_scale = (self.view_scale * factor).clamp(0.05, 10.0);
                    self.view_offset = mouse.to_vec2() - world.to_vec2() * self.view_scale;
                }
            }
        }

        let node_radius = 18.0;
        let strip = self.current_strip();
        let strip_active = strip.is_some();
        let in_strip: Option<Vec<NodeId>> =
            strip.map(|v| v.layers.iter().flatten().copied().collect());

        // --- edges ---
        for &(a, b) in &self.graph.edges {
            let (Some(na), Some(nb)) = (self.graph.node(a), self.graph.node(b)) else { continue };
            let hovered = !strip_active
                && (self.hovered_node == Some(a) || self.hovered_node == Some(b));
            let (color, width) = if hovered {
                (select, 2.5)
            } else if strip_active {
                (faint, 1.5)
            } else {
                (fg, 1.5)
            };
            painter.line_segment(
                [self.to_screen(na.pos), self.to_screen(nb.pos)],
                Stroke::new(width, color),
            );
        }
        if let Some(view) = strip {
            for &(a, b) in &view.edges {
                let (Some(na), Some(nb)) = (self.graph.node(a), self.graph.node(b)) else { continue };
                painter.line_segment(
                    [self.to_screen(na.pos), self.to_screen(nb.pos)],
                    Stroke::new(3.0, accent),
                );
            }
            for &(a, b) in &view.cyclic_edges {
                let (Some(na), Some(nb)) = (self.graph.node(a), self.graph.node(b)) else { continue };
                painter.add(egui::Shape::dashed_line(
                    &[self.to_screen(na.pos), self.to_screen(nb.pos)],
                    Stroke::new(2.5, accent),
                    8.0,
                    6.0,
                ));
            }
        }

        // relation preview line
        if let Some(start_id) = self.edge_start {
            if let (Some(node), Some(pointer)) = (self.graph.node(start_id), response.hover_pos()) {
                painter.line_segment(
                    [self.to_screen(node.pos), pointer],
                    Stroke::new(1.5, select),
                );
            }
        }

        // --- nodes: interaction + drawing ---
        let mut any_node_dragged = false;
        let mut clicked_node: Option<NodeId> = None;
        let mut dbl_clicked_node: Option<NodeId> = None;
        let mut delete_node: Option<NodeId> = None;
        let mut new_hover: Option<NodeId> = None;

        let ids: Vec<NodeId> = self.graph.nodes.iter().map(|n| n.id).collect();
        for id in ids {
            let Some(i) = self.graph.index_of(id) else { continue };
            let screen_pos = self.to_screen(self.graph.nodes[i].pos);
            let rect = Rect::from_center_size(screen_pos, egui::vec2(node_radius * 2.0, node_radius * 2.0));
            let node_resp = ui.interact(rect, ui.id().with(("node", id)), Sense::click_and_drag());

            if node_resp.hovered() {
                new_hover = Some(id);
            }
            if node_resp.dragged() {
                self.graph.nodes[i].pos += node_resp.drag_delta() / self.view_scale;
                any_node_dragged = true;
            }
            if node_resp.double_clicked() {
                dbl_clicked_node = Some(id);
            } else if node_resp.clicked() {
                clicked_node = Some(id);
            }
            if node_resp.secondary_clicked() {
                delete_node = Some(id);
            }

            let dimmed = in_strip.as_ref().is_some_and(|s| !s.contains(&id));
            let hovered = new_hover == Some(id) && !strip_active;
            let (fill, stroke_color, text_color) = if self.edge_start == Some(id) {
                (select, fg, fg)
            } else if dimmed {
                (bg, faint, faint)
            } else if in_strip.is_some() {
                (bg, accent, fg)
            } else if hovered {
                (bg, select, fg)
            } else {
                (bg, fg, fg)
            };
            painter.circle(screen_pos, node_radius, fill, Stroke::new(1.5, stroke_color));
            painter.text(
                screen_pos,
                Align2::CENTER_CENTER,
                &self.graph.nodes[i].label,
                FontId::proportional(13.0),
                text_color,
            );
        }
        self.hovered_node = new_hover;

        // --- apply interactions ---
        if let Some(id) = delete_node {
            self.push_undo();
            self.invalidate_results();
            let label = self.graph.node(id).map_or_else(String::new, |n| n.label.clone());
            self.graph.remove_node(id);
            self.log = format!("Deleted node {}.", label);
        } else if let Some(id) = dbl_clicked_node {
            self.edge_start = None;
            let label = self.graph.label_of(id).to_string();
            self.rename = Some((id, label));
        } else if let Some(id) = clicked_node {
            match self.edge_start {
                None => {
                    self.edge_start = Some(id);
                    self.log = match self.mode {
                        EditMode::Poset => format!(
                            "Selected {}. Click a second node (it becomes the upper one).",
                            self.graph.label_of(id)
                        ),
                        EditMode::Graph => {
                            format!("Selected {}. Click a second node.", self.graph.label_of(id))
                        }
                    };
                }
                Some(start) if start == id => self.edge_start = None,
                Some(start) => {
                    self.push_undo();
                    self.invalidate_results();
                    let added = self.graph.toggle_edge(start, id);
                    let (la, lb) = (self.graph.label_of(start), self.graph.label_of(id));
                    self.log = match (self.mode, added) {
                        (EditMode::Poset, true) => format!("Relation added: {} < {}", la, lb),
                        (EditMode::Poset, false) => format!("Relation removed: {} / {}", la, lb),
                        (EditMode::Graph, true) => format!("Edge added: {} — {}", la, lb),
                        (EditMode::Graph, false) => format!("Edge removed: {} — {}", la, lb),
                    };
                }
            }
        }

        // double-click on empty canvas: add node there
        if response.double_clicked() && dbl_clicked_node.is_none() && clicked_node.is_none() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let over_node = self
                    .graph
                    .nodes
                    .iter()
                    .any(|n| self.to_screen(n.pos).distance(pointer) <= node_radius);
                if !over_node {
                    self.push_undo();
                    self.invalidate_results();
                    let id = self
                        .graph
                        .add_node(self.label_input.trim().to_string(), self.to_world(pointer));
                    self.label_input.clear();
                    self.log = format!("Added node {}.", id);
                }
            }
        }

        // pan with background drag
        if response.dragged_by(egui::PointerButton::Primary) && !any_node_dragged {
            self.view_offset += response.drag_delta();
        }

        // right-click background cancels relation creation
        if response.secondary_clicked() && delete_node.is_none() {
            self.edge_start = None;
            self.log = "Relation creation cancelled.".to_string();
        }
    }

    /// Modal-ish rename window.
    fn rename_window(&mut self, ctx: &egui::Context) {
        let Some((id, mut buf)) = self.rename.take() else { return };
        let mut apply = false;
        let mut cancel = false;

        egui::Window::new("Rename node")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, -100.0))
            .show(ctx, |ui| {
                let resp = ui.text_edit_singleline(&mut buf);
                resp.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if apply {
            if let Some(i) = self.graph.index_of(id) {
                self.push_undo();
                self.invalidate_results();
                self.log = format!("Renamed {} to {}.", self.graph.nodes[i].label, buf);
                self.graph.nodes[i].label =
                    if buf.trim().is_empty() { id.to_string() } else { buf.trim().to_string() };
            }
        } else if !cancel {
            self.rename = Some((id, buf)); // keep open
        }
    }
}

impl eframe::App for LatticeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        // global shortcuts
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.edge_start = None;
            self.rename = None;
        }

        self.rename_window(ctx);

        egui::SidePanel::left("controls")
            .min_width(260.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.sidebar(ui));
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| self.canvas(ui));
    }
}

pub fn interactive() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Rhombic Strips — Interactive",
        options,
        Box::new(|cc| Ok(Box::new(LatticeApp::new(cc)))),
    );
}

// ===========================================================================
// Tests (model, generators, tube machinery, worker; the UI is not unit-tested)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_enumeration(faces: Vec<Face>, id_map: Vec<NodeId>, cyclic: bool) -> usize {
        let job = Job::spawn(faces, id_map, cyclic, JobKind::Enumerate);
        let mut n = 0;
        for msg in job.rx.iter() {
            match msg {
                WorkerMsg::Strip { .. } => n += 1,
                WorkerMsg::Done(total) => {
                    assert_eq!(total, n);
                    break;
                }
                WorkerMsg::Progress(_) => {}
            }
        }
        n
    }

    #[test]
    fn grid_generator_and_ranks() {
        let g = PosetGraph::grid("211").unwrap();
        assert_eq!(g.nodes.len(), 3 * 2 * 2);
        let ranks = g.ranks().unwrap();
        assert_eq!(*ranks.values().max().unwrap(), 4); // top element 211
        assert_eq!(g.edges.len(), 20);
    }

    #[test]
    fn cycle_detection() {
        let mut g = PosetGraph::default();
        let a = g.add_node("a".into(), Pos2::ZERO);
        let b = g.add_node("b".into(), Pos2::ZERO);
        g.edges.push((a, b));
        g.edges.push((b, a));
        assert!(g.ranks().is_err());
    }

    #[test]
    fn save_load_roundtrip() {
        let g = PosetGraph::grid("21").unwrap();
        let file = g.to_lattice_file().unwrap();
        let l = Lattice::from_str_content(&file).unwrap();
        let g2 = PosetGraph::from_lattice(&l);
        assert_eq!(g.nodes.len(), g2.nodes.len());
        assert_eq!(g.edges.len(), g2.edges.len());
        let mut l1: Vec<_> = g.nodes.iter().map(|n| n.label.clone()).collect();
        let mut l2: Vec<_> = g2.nodes.iter().map(|n| n.label.clone()).collect();
        l1.sort();
        l2.sort();
        assert_eq!(l1, l2);
    }

    #[test]
    fn distributive_of_antichain_is_boolean() {
        let mut g = PosetGraph::default();
        for i in 0..3 {
            g.add_node(format!("{}", i), Pos2::ZERO);
        }
        let j = g.distributive().unwrap();
        assert_eq!(j.nodes.len(), 8); // J(antichain_3) = boolean lattice B_3
        assert_eq!(j.edges.len(), 12);
    }

    #[test]
    fn cube_and_simplex_lattices() {
        let sq = PosetGraph::cube_lattice(2).unwrap();
        assert_eq!(sq.nodes.len(), 9); // 4 + 4 + 1
        assert_eq!(sq.edges.len(), 12); // 8 vertex-edge + 4 edge-top covers
        let c3 = PosetGraph::cube_lattice(3).unwrap();
        assert_eq!(c3.nodes.len(), 27);

        let tri = PosetGraph::simplex_lattice(2).unwrap();
        assert_eq!(tri.nodes.len(), 7); // 3 + 3 + 1
        let tet = PosetGraph::simplex_lattice(3).unwrap();
        assert_eq!(tet.nodes.len(), 15); // 2^4 - 1
    }

    #[test]
    fn tubes_of_a_path() {
        let g = PosetGraph::graph_path(3);
        let adj = adjacency_masks(&g);
        // proper connected subsets: {0},{1},{2},{01},{12} — not {02}
        let tubes: Vec<Mask> = (1..0b111).filter(|&m| mask_connected(m, &adj)).collect();
        assert_eq!(tubes.len(), 5);
        assert!(!tubes.contains(&0b101));
    }

    #[test]
    fn tube_compatibility() {
        let g = PosetGraph::graph_path(3); // 0 - 1 - 2
        let adj = adjacency_masks(&g);
        assert!(tubes_compatible(0b001, 0b011, &adj)); // nested
        assert!(tubes_compatible(0b001, 0b100, &adj)); // disjoint, non-adjacent
        assert!(!tubes_compatible(0b001, 0b010, &adj)); // disjoint but adjacent
        assert!(!tubes_compatible(0b011, 0b110, &adj)); // properly overlapping
    }

    #[test]
    fn small_polytopes_have_correct_face_counts() {
        // P(path_3) = pentagon: 5 + 5 + 1 faces
        let pent = graph_associahedron(&PosetGraph::graph_path(3)).unwrap();
        assert_eq!(pent.nodes.len(), 11);
        // P(K_3) = hexagon: 6 + 6 + 1 = Fubini(3)
        let hex = graph_associahedron(&PosetGraph::graph_complete(3)).unwrap();
        assert_eq!(hex.nodes.len(), 13);
        // P(path_4) = 3d associahedron: 14 + 21 + 9 + 1
        let assoc3 = graph_associahedron(&PosetGraph::graph_path(4)).unwrap();
        assert_eq!(assoc3.nodes.len(), 45);
        // P(K_4) = permutahedron Pi_3: Fubini(4)
        let perm3 = graph_associahedron(&PosetGraph::graph_complete(4)).unwrap();
        assert_eq!(perm3.nodes.len(), 75);
        // ranks are the face dimensions: vertices at 0, top at n-1
        let ranks = perm3.ranks().unwrap();
        assert_eq!(*ranks.values().max().unwrap(), 3);
    }

    #[test]
    fn tube_poset_of_path() {
        let tp = tube_poset(&PosetGraph::graph_path(3)).unwrap();
        assert_eq!(tp.nodes.len(), 6); // 5 proper tubes + full set
        assert_eq!(tp.edges.len(), 6);
        let ranks = tp.ranks().unwrap();
        assert_eq!(*ranks.values().max().unwrap(), 2); // graded by cardinality - 1
    }

    #[test]
    fn disconnected_graph_rejected() {
        let mut g = PosetGraph::default();
        g.add_node("a".into(), Pos2::ZERO);
        g.add_node("b".into(), Pos2::ZERO);
        assert!(graph_associahedron(&g).is_err());
        assert!(tube_poset(&g).is_err());
    }

    #[test]
    fn hexagon_face_lattice_strips() {
        // hexagon = permutahedron of K_3; like the square, a polygon has
        // exactly 2 cyclic rhombic strips (one per orientation)
        let hex = graph_associahedron(&PosetGraph::graph_complete(3)).unwrap();
        let (faces, _) = hex.to_faces().unwrap();
        let l = Lattice::from_faces(faces);
        assert_eq!(rhombic::count_strips(&l, true), 2);
    }

    #[test]
    fn worker_matches_direct_count() {
        let g = PosetGraph::grid("11").unwrap();
        let (faces, id_map) = g.to_faces().unwrap();
        let l = Lattice::from_faces(faces.clone());
        for cyclic in [false, true] {
            let direct = rhombic::count_strips(&l, cyclic);
            let streamed = drain_enumeration(faces.clone(), id_map.clone(), cyclic);
            assert_eq!(direct, streamed, "cyclic={}", cyclic);
        }
    }

    #[test]
    fn worker_exists_stops_after_first() {
        let g = PosetGraph::grid("11").unwrap();
        let (faces, id_map) = g.to_faces().unwrap();
        let job = Job::spawn(faces, id_map, false, JobKind::Exists);
        let first = job.rx.iter().next();
        assert!(matches!(first, Some(WorkerMsg::Strip { .. })));
        job.cancel();
    }
}
