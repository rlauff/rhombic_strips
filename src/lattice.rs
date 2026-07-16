//! Face lattices of graded posets.
//!
//! Faces live in the `Lattice`, which acts as an arena; upsets, downsets,
//! levels and bridges are stored as indices (`FaceId`) into that arena.
//! A `Face` on its own is therefore meaningless.
//!
//! All internals are private. Access goes through the getter methods on
//! `Lattice` and `Face`, so the representation (flat bridge matrix,
//! per-level index lists, ...) can change without touching client code.

use std::fmt;
use std::fs::read_to_string;

/// Index of a face in the arena of its `Lattice`.
pub type FaceId = usize;

// ---------------------------------------------------------------------------
// Face
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Face {
    label: String,
    dim: usize,
    upset: Vec<FaceId>,   // faces covering this one
    downset: Vec<FaceId>, // faces covered by this one
}

impl Face {
    pub fn new(label: String, dim: usize, upset: Vec<FaceId>, downset: Vec<FaceId>) -> Self {
        Face { label, dim, upset, downset }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Faces covering this face (indices into the lattice arena).
    pub fn upset(&self) -> &[FaceId] {
        &self.upset
    }

    /// Faces covered by this face (indices into the lattice arena).
    pub fn downset(&self) -> &[FaceId] {
        &self.downset
    }
}

// ---------------------------------------------------------------------------
// Lattice
// ---------------------------------------------------------------------------

pub struct Lattice {
    faces: Vec<Face>,
    /// `levels[d]` lists the ids of all faces of dimension `d`.
    levels: Vec<Vec<FaceId>>,
    /// Flat `n x n` matrix; `bridges[i * n + j]` is the common cover of
    /// faces `i` and `j`, if any. Symmetric.
    bridges: Vec<Option<FaceId>>,
    dim: usize,
}

impl fmt::Debug for Lattice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lattice")
            .field("num_faces", &self.faces.len())
            .field("dim", &self.dim)
            .field("levels", &self.levels)
            .finish()
    }
}

impl Lattice {
    // -- construction -------------------------------------------------------

    /// Build a lattice from its faces; levels and bridges are derived.
    pub fn from_faces(faces: Vec<Face>) -> Self {
        let n = faces.len();
        let dim = faces.iter().map(|f| f.dim).max().unwrap_or(0);

        // levels
        let mut levels = vec![Vec::new(); dim + 1];
        for (i, face) in faces.iter().enumerate() {
            levels[face.dim].push(i);
        }

        // bridges: a face b is a bridge between i and j iff {i, j} ⊆ downset(b).
        // Instead of scanning all faces for every pair (O(n^2 * n)), walk the
        // downset pairs of every face once (O(sum_b deg(b)^2)).
        // Ties (several common covers) resolve to the smallest face id, as before.
        let mut bridges = vec![None; n * n];
        for (b, face) in faces.iter().enumerate() {
            for (k, &i) in face.downset.iter().enumerate() {
                for &j in &face.downset[k + 1..] {
                    if bridges[i * n + j].is_none() {
                        bridges[i * n + j] = Some(b);
                        bridges[j * n + i] = Some(b);
                    }
                }
            }
        }

        Lattice { faces, levels, bridges, dim }
    }

    /// Parse a lattice file. One face per line:
    /// `dim: label: {upset}, {downset}`, e.g. `0: 000: {16, 10, 8}, {}`.
    /// Empty and malformed-header lines are skipped, matching the old parser.
    pub fn from_file(path: &str) -> Result<Self, String> {
        let content =
            read_to_string(path).map_err(|e| format!("reading {} failed: {}", path, e))?;
        Self::from_str_content(&content)
    }

    /// Parse lattice data from a string (same format as `from_file`).
    pub fn from_str_content(content: &str) -> Result<Self, String> {
        fn parse_set(s: &str, line_no: usize) -> Result<Vec<FaceId>, String> {
            let s = s.trim().trim_start_matches('{').trim_end_matches('}').trim();
            if s.is_empty() {
                return Ok(vec![]);
            }
            s.split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(|t| {
                    t.parse::<FaceId>()
                        .map_err(|_| format!("line {}: '{}' is not an integer", line_no, t))
                })
                .collect()
        }

        let mut faces = Vec::new();
        for (line_no, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            // "dim: label: {upset}, {downset}" — limit to 3 so the sets stay intact
            let parts: Vec<&str> = line.splitn(3, ": ").collect();
            if parts.len() < 3 {
                continue;
            }

            let dim = parts[0]
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("line {}: dimension is not an integer", line_no + 1))?;
            let label = parts[1].trim().to_string();

            let sets: Vec<&str> = parts[2].split("}, {").collect();
            if sets.len() < 2 {
                return Err(format!(
                    "line {}: expected '{{upset}}, {{downset}}'",
                    line_no + 1
                ));
            }
            let upset = parse_set(sets[0], line_no + 1)?;
            let downset = parse_set(sets[1], line_no + 1)?;

            faces.push(Face::new(label, dim, upset, downset));
        }

        Ok(Self::from_faces(faces))
    }

    // -- getters -------------------------------------------------------------

    pub fn num_faces(&self) -> usize {
        self.faces.len()
    }

    /// Dimension of the lattice (maximal face dimension).
    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn face(&self, id: FaceId) -> &Face {
        &self.faces[id]
    }

    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &Face)> {
        self.faces.iter().enumerate()
    }

    /// Ids of all faces of dimension `d` (empty slice if out of range).
    pub fn level(&self, d: usize) -> &[FaceId] {
        self.levels.get(d).map_or(&[], |v| v.as_slice())
    }

    pub fn num_levels(&self) -> usize {
        self.levels.len()
    }

    /// The bridge (common cover) of two faces, if it exists.
    pub fn bridge(&self, f1: FaceId, f2: FaceId) -> Option<FaceId> {
        self.bridges[f1 * self.faces.len() + f2]
    }

    // -- hamiltonian paths on a level -----------------------------------------

    /// Lazily generate hamiltonian paths (or cycles) on the bridge graph of
    /// level 0. Avoids materialising all paths at once and allows early exit.
    pub fn ham_paths(&self, cyclic: bool) -> HamiltonianIter {
        self.ham_paths_on_level(0, cyclic)
    }

    /// Same as `ham_paths`, but on an arbitrary level: vertices are the faces
    /// of dimension `d`, edges are pairs with a common cover (a bridge).
    pub fn ham_paths_on_level(&self, d: usize, cyclic: bool) -> HamiltonianIter {
        let (nodes, adj) = self.level_graph(d);
        if nodes.is_empty() {
            return HamiltonianIter::empty();
        }
        HamiltonianIter::new(nodes, adj, cyclic)
    }

    /// Bridge graph of level `d`: its vertices and an adjacency list indexed
    /// directly by FaceId.
    fn level_graph(&self, d: usize) -> (Vec<FaceId>, Vec<Vec<FaceId>>) {
        let nodes: Vec<FaceId> = self.level(d).to_vec();
        let mut adj: Vec<Vec<FaceId>> = vec![vec![]; self.num_faces()];
        for (i, &u) in nodes.iter().enumerate() {
            for &v in &nodes[i + 1..] {
                if self.bridge(u, v).is_some() {
                    adj[u].push(v);
                    adj[v].push(u);
                }
            }
        }
        (nodes, adj)
    }

    /// Split `ham_paths(cyclic)` into independent iterators whose outputs
    /// partition the full set of hamiltonian paths — the unit of work for
    /// parallel search. Aim for at least `target` seeds (fewer only if the
    /// search tree is too small).
    ///
    /// Each seed owns the DFS subtree below one simple path of a fixed
    /// length k (the same start-node scheme and symmetry breaking as the
    /// sequential iterator, applied at yield time). Every hamiltonian path
    /// has exactly one length-k prefix, so the union over the uniform-depth
    /// prefix set is exact and duplicate-free. This parallelises the path
    /// *search* itself — with a plain `par_bridge()` over `ham_paths` the
    /// single sequential DFS producer is the bottleneck and all cores but
    /// one sit idle whenever generating paths dominates.
    pub fn ham_path_seeds(&self, cyclic: bool, target: usize) -> Vec<HamiltonianIter> {
        let (nodes, adj) = self.level_graph(0);
        let n = nodes.len();
        if n == 0 {
            return vec![HamiltonianIter::empty()];
        }
        if n <= 3 || target <= 1 {
            return vec![HamiltonianIter::new(nodes, adj, cyclic)];
        }

        // Uniform-depth prefix expansion. Cycles are anchored at nodes[0]
        // (every hamiltonian cycle is a rotation of one through it), paths
        // may start anywhere; both exactly as in `push_start_node`.
        let mut prefixes: Vec<Vec<FaceId>> = if cyclic {
            vec![vec![nodes[0]]]
        } else {
            nodes.iter().map(|&u| vec![u]).collect()
        };
        let mut depth = 1;
        while prefixes.len() < target && depth < n - 1 {
            let mut next = Vec::with_capacity(prefixes.len() * 4);
            for p in &prefixes {
                let last = *p.last().expect("prefix non-empty");
                for &v in &adj[last] {
                    if !p.contains(&v) {
                        let mut q = p.clone();
                        q.push(v);
                        next.push(q);
                    }
                }
            }
            if next.is_empty() {
                // No simple path of length depth+1 <= n-1: no hamiltonian
                // path exists at all.
                return vec![HamiltonianIter::empty()];
            }
            prefixes = next;
            depth += 1;
        }

        let nodes = std::sync::Arc::new(nodes);
        let adj = std::sync::Arc::new(adj);
        prefixes
            .into_iter()
            .map(|p| HamiltonianIter::with_prefix(nodes.clone(), adj.clone(), cyclic, p))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// HamiltonianIter: iterative DFS over hamiltonian paths/cycles
// ---------------------------------------------------------------------------

pub struct HamiltonianIter {
    nodes: std::sync::Arc<Vec<FaceId>>, // vertices of the level graph
    adj: std::sync::Arc<Vec<Vec<FaceId>>>, // adjacency list, indexed by FaceId
    cyclic: bool,

    // DFS state
    stack: Vec<(FaceId, usize)>, // (current node, next neighbour index to try)
    path: Vec<FaceId>,
    visited: Vec<bool>,

    start_node_index: usize, // which entry of `nodes` we are starting from
    prefix_mode: bool,       // seeded: exhaust one subtree, don't advance starts
    finished: bool,
}

impl HamiltonianIter {
    fn new(nodes: Vec<FaceId>, adj: Vec<Vec<FaceId>>, cyclic: bool) -> Self {
        let n = nodes.len();
        let num_ids = adj.len();
        let mut iter = HamiltonianIter {
            nodes: std::sync::Arc::new(nodes),
            adj: std::sync::Arc::new(adj),
            cyclic,
            stack: Vec::with_capacity(n),
            path: Vec::with_capacity(n),
            visited: vec![false; num_ids],
            start_node_index: 0,
            prefix_mode: false,
            finished: false,
        };
        iter.push_start_node();
        iter
    }

    /// Seeded iterator: exactly the hamiltonian paths whose first
    /// `prefix.len()` vertices equal `prefix` (which must be a simple path in
    /// the level graph). Frames above the deepest one get exhausted
    /// neighbour cursors, so backtracking never explores the prefix's
    /// siblings — those belong to other seeds.
    fn with_prefix(
        nodes: std::sync::Arc<Vec<FaceId>>,
        adj: std::sync::Arc<Vec<Vec<FaceId>>>,
        cyclic: bool,
        prefix: Vec<FaceId>,
    ) -> Self {
        let n = nodes.len();
        let num_ids = adj.len();
        let mut iter = HamiltonianIter {
            nodes,
            adj,
            cyclic,
            stack: Vec::with_capacity(n),
            path: Vec::with_capacity(n),
            visited: vec![false; num_ids],
            start_node_index: 0,
            prefix_mode: true,
            finished: false,
        };
        let deepest = prefix.len() - 1;
        for (i, &u) in prefix.iter().enumerate() {
            iter.visited[u] = true;
            iter.path.push(u);
            let cursor = if i == deepest { 0 } else { iter.adj[u].len() };
            iter.stack.push((u, cursor));
        }
        iter
    }

    fn empty() -> Self {
        HamiltonianIter {
            nodes: std::sync::Arc::new(vec![]),
            adj: std::sync::Arc::new(vec![]),
            cyclic: false,
            stack: vec![],
            path: vec![],
            visited: vec![],
            start_node_index: 0,
            prefix_mode: false,
            finished: true,
        }
    }

    /// Reset DFS state and start from the next start node.
    fn push_start_node(&mut self) {
        if self.start_node_index >= self.nodes.len() {
            self.finished = true;
            return;
        }
        // For cycles only the first start node is needed: every hamiltonian
        // cycle is a rotation of one through nodes[0].
        if self.cyclic && self.start_node_index > 0 {
            self.finished = true;
            return;
        }

        let start = self.nodes[self.start_node_index];
        self.path.clear();
        self.stack.clear();
        self.visited.fill(false);

        self.visited[start] = true;
        self.path.push(start);
        self.stack.push((start, 0));
    }
}

impl Iterator for HamiltonianIter {
    type Item = Vec<FaceId>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        // trivial single-vertex level
        if self.nodes.len() == 1 {
            self.finished = true;
            return if self.path.len() == 1 {
                Some(std::mem::take(&mut self.path))
            } else {
                None
            };
        }

        loop {
            // exhausted current start node -> advance to next one
            // (a seeded iterator owns exactly one subtree: it is done)
            if self.stack.is_empty() {
                if self.prefix_mode {
                    self.finished = true;
                    return None;
                }
                self.start_node_index += 1;
                self.push_start_node();
                if self.finished {
                    return None;
                }
                continue;
            }

            let (u, neighbor_idx) = *self.stack.last().unwrap();

            // all neighbours of u tried -> backtrack
            if neighbor_idx >= self.adj[u].len() {
                self.stack.pop();
                self.path.pop();
                self.visited[u] = false;
                continue;
            }

            // advance neighbour cursor for next visit
            self.stack.last_mut().unwrap().1 += 1;

            let v = self.adj[u][neighbor_idx];
            if self.visited[v] {
                continue;
            }

            self.visited[v] = true;
            self.path.push(v);
            self.stack.push((v, 0));

            if self.path.len() == self.nodes.len() {
                let result = if self.cyclic {
                    // must close up to the start node
                    self.adj[v].contains(&self.path[0]).then(|| self.path.clone())
                } else {
                    // symmetry breaking for paths: start <= end
                    (self.path[0] <= *self.path.last().unwrap()).then(|| self.path.clone())
                };

                // backtrack immediately so the search can continue
                self.stack.pop();
                self.path.pop();
                self.visited[v] = false;

                if result.is_some() {
                    return result;
                }
            }
        }
    }
}
