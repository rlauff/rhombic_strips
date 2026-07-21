//! Batch "scripts" behind the browser's *Scripts* panel.
//!
//! Unlike everything else in the crate, these do not operate on the single
//! drawn diagram but on *families* of objects:
//!
//! * [`GraphSurvey`] — every connected graph on up to `n` vertices (one per
//!   isomorphism class), each with Hamilton path/cycle status and rhombic /
//!   cyclic strip existence for its tube poset. This probes the conjecture
//!   "Hamilton path ⇒ the tube poset admits a rhombic strip" (the cyclic
//!   analogue is known to fail).
//! * [`BoundaryEnumerator`] — every (linear) rhombic strip of one poset,
//!   reduced to its pair of boundary chains, read bottom-to-top as
//!   permutations and tallied.
//!
//! Architecture mirrors [`crate::web`]: the pure logic lives in [`api`]
//! (plain Rust, unit-tested on the host), and the `#[wasm_bindgen]` steppers
//! below expose it to `www/worker.js` with the same `step(budget_ms)`
//! contract as `web::StripEnumerator`, so long runs stay sliceable,
//! streamable and cancellable.

use std::collections::HashMap;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::lattice::Lattice;
use crate::rhombic::{self, Strip};
use crate::web::{api as webapi, now_ms};

pub mod api {
    //! Pure, host-testable implementations.

    use std::collections::HashSet;

    use serde::Serialize;

    use crate::lattice::{Face, FaceId, Lattice};
    use crate::rhombic;
    use crate::web::now_ms;

    /// Largest vertex count the survey accepts. Edge bit `i * n + j`
    /// (for `i < j`) then tops out at `5 * 7 + 6 = 41 < 64`, and the
    /// canonical-form search over all `n! = 5040` relabelings stays cheap.
    pub const MAX_SURVEY_N: usize = 7;

    // -----------------------------------------------------------------------
    // Small graphs as edge bitmasks
    // -----------------------------------------------------------------------

    fn edge_bit(n: usize, i: usize, j: usize) -> u64 {
        debug_assert!(i < j && j < n);
        1u64 << (i * n + j)
    }

    /// Per-vertex neighbour masks (vertex subsets) of an edge mask.
    pub fn adjacency(n: usize, mask: u64) -> Vec<u64> {
        let mut adj = vec![0u64; n];
        for i in 0..n {
            for j in (i + 1)..n {
                if mask & edge_bit(n, i, j) != 0 {
                    adj[i] |= 1 << j;
                    adj[j] |= 1 << i;
                }
            }
        }
        adj
    }

    /// Is the nonempty vertex subset `sub` connected under `adj`?
    pub fn subset_connected(sub: u64, adj: &[u64]) -> bool {
        if sub == 0 {
            return false;
        }
        let mut seen = 1u64 << sub.trailing_zeros();
        loop {
            let mut grow = seen;
            let mut m = seen;
            while m != 0 {
                let v = m.trailing_zeros() as usize;
                m &= m - 1;
                grow |= adj[v] & sub;
            }
            if grow == seen {
                break;
            }
            seen = grow;
        }
        seen == sub
    }

    pub fn graph_connected(n: usize, adj: &[u64]) -> bool {
        subset_connected((1u64 << n) - 1, adj)
    }

    /// Does the graph have a Hamilton path? Plain backtracking; n ≤ 7.
    pub fn ham_path(n: usize, adj: &[u64]) -> bool {
        fn dfs(v: usize, visited: u64, n: usize, adj: &[u64]) -> bool {
            if visited.count_ones() as usize == n {
                return true;
            }
            let mut cand = adj[v] & !visited;
            while cand != 0 {
                let u = cand.trailing_zeros() as usize;
                cand &= cand - 1;
                if dfs(u, visited | (1 << u), n, adj) {
                    return true;
                }
            }
            false
        }
        n == 1 || (0..n).any(|s| dfs(s, 1u64 << s, n, adj))
    }

    /// Does the graph have a Hamilton cycle? (`false` for n < 3.)
    pub fn ham_cycle(n: usize, adj: &[u64]) -> bool {
        if n < 3 {
            return false;
        }
        fn dfs(v: usize, visited: u64, n: usize, adj: &[u64]) -> bool {
            if visited.count_ones() as usize == n {
                return adj[v] & 1 != 0; // close up to vertex 0
            }
            let mut cand = adj[v] & !visited;
            while cand != 0 {
                let u = cand.trailing_zeros() as usize;
                cand &= cand - 1;
                if dfs(u, visited | (1 << u), n, adj) {
                    return true;
                }
            }
            false
        }
        // every Hamilton cycle passes through vertex 0
        dfs(0, 1, n, adj)
    }

    fn permutations(n: usize) -> Vec<Vec<u8>> {
        let mut out = vec![vec![]];
        for k in 0..n as u8 {
            out = out
                .into_iter()
                .flat_map(|p| {
                    (0..=p.len()).map(move |i| {
                        let mut q = p.clone();
                        q.insert(i, k);
                        q
                    })
                })
                .collect();
        }
        out
    }

    fn relabel(mask: u64, n: usize, p: &[u8]) -> u64 {
        let mut out = 0;
        for i in 0..n {
            for j in (i + 1)..n {
                if mask & edge_bit(n, i, j) != 0 {
                    let (a, b) = (p[i] as usize, p[j] as usize);
                    out |= if a < b { edge_bit(n, a, b) } else { edge_bit(n, b, a) };
                }
            }
        }
        out
    }

    /// Canonical form: minimum edge mask over all vertex relabelings.
    pub fn canonical(mask: u64, n: usize, perms: &[Vec<u8>]) -> u64 {
        perms.iter().map(|p| relabel(mask, n, p)).min().unwrap_or(0)
    }

    /// Re-index an edge mask from `from_n`-vertex to `to_n`-vertex layout.
    fn embed(mask: u64, from_n: usize, to_n: usize) -> u64 {
        let mut out = 0;
        for i in 0..from_n {
            for j in (i + 1)..from_n {
                if mask & edge_bit(from_n, i, j) != 0 {
                    out |= edge_bit(to_n, i, j);
                }
            }
        }
        out
    }

    pub fn edges_of(mask: u64, n: usize) -> Vec<(u8, u8)> {
        let mut out = vec![];
        for i in 0..n {
            for j in (i + 1)..n {
                if mask & edge_bit(n, i, j) != 0 {
                    out.push((i as u8, j as u8));
                }
            }
        }
        out
    }

    // -----------------------------------------------------------------------
    // Tube posets straight from masks
    // -----------------------------------------------------------------------

    /// The tube poset of the graph (connected vertex subsets under inclusion)
    /// as a [`Lattice`]. Tubes are ranked by `|tube| - 1`, so the singletons
    /// form level 0; covers add one neighbouring vertex. Face labels are the
    /// vertex digits, matching `web::api::gen_tube_poset` for ≤ 10 vertices.
    pub fn tube_poset(n: usize, adj: &[u64]) -> Lattice {
        let full = (1u64 << n) - 1;
        let mut subs: Vec<u64> = (1..=full).filter(|&m| subset_connected(m, adj)).collect();
        subs.sort_by_key(|m| (m.count_ones(), *m));
        let idx: std::collections::HashMap<u64, FaceId> =
            subs.iter().enumerate().map(|(i, &m)| (m, i)).collect();

        let mut upsets: Vec<Vec<FaceId>> = vec![vec![]; subs.len()];
        let mut downsets: Vec<Vec<FaceId>> = vec![vec![]; subs.len()];
        for (i, &a) in subs.iter().enumerate() {
            // covers of a tube: add one vertex adjacent to it (anything else
            // disconnects); the result is connected again, hence a tube.
            let mut nb = 0u64;
            let mut m = a;
            while m != 0 {
                let v = m.trailing_zeros() as usize;
                m &= m - 1;
                nb |= adj[v];
            }
            let mut cand = nb & !a & full;
            while cand != 0 {
                let v = cand.trailing_zeros();
                cand &= cand - 1;
                let j = idx[&(a | (1u64 << v))];
                upsets[i].push(j);
                downsets[j].push(i);
            }
        }

        let label = |m: u64| -> String {
            (0..n).filter(|&v| m & (1 << v) != 0).map(|v| v.to_string()).collect()
        };
        let faces = subs
            .iter()
            .enumerate()
            .map(|(i, &m)| {
                Face::new(
                    label(m),
                    m.count_ones() as usize - 1,
                    std::mem::take(&mut upsets[i]),
                    std::mem::take(&mut downsets[i]),
                )
            })
            .collect();
        Lattice::from_faces(faces)
    }

    // -----------------------------------------------------------------------
    // Graph survey
    // -----------------------------------------------------------------------

    #[derive(Serialize, Clone)]
    #[serde(rename_all = "camelCase")]
    pub struct GraphResult {
        pub n: usize,
        pub edges: Vec<(u8, u8)>,
        pub ham_path: bool,
        pub ham_cycle: bool,
        /// Tube poset admits a rhombic strip; absent if unchecked.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub strip: Option<bool>,
        /// Tube poset admits a cyclic rhombic strip; absent if unchecked.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub cyclic_strip: Option<bool>,
        pub tubes: usize,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SurveyStep {
        /// Results that became available during this step only.
        pub results: Vec<GraphResult>,
        /// "generate" while enumerating isomorphism classes, then "check".
        pub phase: &'static str,
        /// Vertex count currently being generated (generate phase).
        pub level: usize,
        pub checked: usize,
        /// Number of connected graphs to check; 0 until generation is done.
        pub total: usize,
        pub done: bool,
    }

    enum Phase {
        /// Building `levels[gen_level]` by vertex augmentation.
        Generate,
        /// Working through `targets`.
        Check,
        Done,
    }

    /// Incremental survey over all connected graphs on 2..=max_n vertices,
    /// one representative per isomorphism class.
    ///
    /// Graphs are generated by vertex augmentation: every graph on `k`
    /// vertices arises from one on `k - 1` (delete any vertex), so extending
    /// each (k-1)-class by a new vertex with every neighbour subset —
    /// including the empty one, disconnected graphs are needed as parents —
    /// and deduplicating canonical forms yields every class exactly once.
    pub struct SurveyCore {
        max_n: usize,
        check_linear: bool,
        check_cyclic: bool,

        // generation state
        levels: Vec<Vec<u64>>, // levels[k]: all k-vertex classes (incl. disconnected)
        gen_level: usize,
        gen_parent: usize,
        gen_subset: u64,
        perms: Vec<Vec<u8>>, // permutations of 0..gen_level
        seen: HashSet<u64>,
        next_level: Vec<u64>,

        // check state
        targets: Vec<(usize, u64)>, // (n, edge mask), connected only
        next_target: usize,

        phase: Phase,
    }

    impl SurveyCore {
        pub fn new(max_n: usize, check_linear: bool, check_cyclic: bool) -> Result<Self, String> {
            if !(2..=MAX_SURVEY_N).contains(&max_n) {
                return Err(format!("n must be between 2 and {}.", MAX_SURVEY_N));
            }
            Ok(SurveyCore {
                max_n,
                check_linear,
                check_cyclic,
                levels: vec![vec![], vec![0u64]], // the single 1-vertex graph
                gen_level: 2,
                gen_parent: 0,
                gen_subset: 0,
                perms: permutations(2),
                seen: HashSet::new(),
                next_level: vec![],
                targets: vec![],
                next_target: 0,
                phase: Phase::Generate,
            })
        }

        /// Advance for at most `budget_ms`; the budget is checked between
        /// candidates/graphs, so one very hard strip search can overshoot it
        /// (cancellation still works — the worker is simply terminated).
        pub fn step(&mut self, budget_ms: f64) -> SurveyStep {
            let deadline = now_ms() + budget_ms;
            let mut fresh = vec![];

            loop {
                match self.phase {
                    Phase::Generate => self.generate_one(),
                    Phase::Check => {
                        if let Some(r) = self.check_one() {
                            fresh.push(r);
                        }
                    }
                    Phase::Done => break,
                }
                if now_ms() >= deadline {
                    break;
                }
            }

            SurveyStep {
                results: fresh,
                phase: match self.phase {
                    Phase::Generate => "generate",
                    _ => "check",
                },
                level: self.gen_level.min(self.max_n),
                checked: self.next_target,
                total: self.targets.len(),
                done: matches!(self.phase, Phase::Done),
            }
        }

        /// Process one augmentation candidate, advancing the cursor; on level
        /// completion, finalise it and either start the next level or switch
        /// to the check phase.
        fn generate_one(&mut self) {
            let k = self.gen_level;
            let parent = self.levels[k - 1][self.gen_parent];
            let mut child = embed(parent, k - 1, k);
            let mut s = self.gen_subset;
            while s != 0 {
                let v = s.trailing_zeros() as usize;
                s &= s - 1;
                child |= edge_bit(k, v, k - 1);
            }
            let canon = canonical(child, k, &self.perms);
            if self.seen.insert(canon) {
                self.next_level.push(canon);
            }

            // advance (subset-minor, parent-major)
            self.gen_subset += 1;
            if self.gen_subset == 1u64 << (k - 1) {
                self.gen_subset = 0;
                self.gen_parent += 1;
                if self.gen_parent == self.levels[k - 1].len() {
                    // level complete
                    self.next_level.sort_unstable();
                    self.levels.push(std::mem::take(&mut self.next_level));
                    self.seen.clear();
                    self.gen_parent = 0;
                    self.gen_level += 1;
                    if self.gen_level > self.max_n {
                        self.collect_targets();
                        self.phase = Phase::Check;
                    } else {
                        self.perms = permutations(self.gen_level);
                    }
                }
            }
        }

        /// Connected classes only, sparse graphs first — their tube posets
        /// are small, so results stream in from easy to hard.
        fn collect_targets(&mut self) {
            for n in 2..=self.max_n {
                for &mask in &self.levels[n] {
                    if graph_connected(n, &adjacency(n, mask)) {
                        self.targets.push((n, mask));
                    }
                }
            }
            self.targets.sort_by_key(|&(n, mask)| (n, mask.count_ones()));
        }

        fn check_one(&mut self) -> Option<GraphResult> {
            if self.next_target == self.targets.len() {
                self.phase = Phase::Done;
                return None;
            }
            let (n, mask) = self.targets[self.next_target];
            self.next_target += 1;

            let adj = adjacency(n, mask);
            let lat = tube_poset(n, &adj);
            Some(GraphResult {
                n,
                edges: edges_of(mask, n),
                ham_path: ham_path(n, &adj),
                ham_cycle: ham_cycle(n, &adj),
                strip: self
                    .check_linear
                    .then(|| rhombic::strips(&lat, false).next().is_some()),
                cyclic_strip: self
                    .check_cyclic
                    .then(|| rhombic::strips(&lat, true).next().is_some()),
                tubes: lat.num_faces(),
            })
        }
    }

    // -----------------------------------------------------------------------
    // Boundary chains → permutations
    // -----------------------------------------------------------------------

    /// Read a maximal chain (bottom to top) as a permutation, when the labels
    /// support it: labels are tokenised (comma-split if any comma, else into
    /// characters); if the bottom is a single token and every cover adds
    /// exactly one token, the sequence of added tokens *is* the permutation
    /// (e.g. tube posets: the order in which vertices join the tube).
    /// Otherwise the chain of labels itself is returned, joined by " < ".
    pub fn chain_to_perm(labels: &[String]) -> String {
        // Tokenisation is decided once per chain: if any label contains a
        // comma, all labels are comma-separated token lists (a comma-free
        // label is then a single token — e.g. a singleton tube "v1");
        // otherwise every character is a token.
        let comma_mode = labels.iter().any(|l| l.contains(','));
        let tokens = |label: &str| -> Vec<String> {
            if comma_mode {
                label
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            } else {
                label.chars().map(String::from).collect()
            }
        };

        let fallback = || labels.join(" < ");
        let Some(first) = labels.first() else {
            return String::new();
        };
        let first = tokens(first);
        if first.len() != 1 {
            return fallback();
        }
        let mut seq = vec![first[0].clone()];
        for w in labels.windows(2) {
            let mut lower = tokens(&w[0]);
            let upper = tokens(&w[1]);
            if upper.len() != lower.len() + 1 {
                return fallback();
            }
            // multiset difference upper \ lower; must be a single token
            let mut added: Option<String> = None;
            for t in upper {
                if let Some(pos) = lower.iter().position(|x| *x == t) {
                    lower.remove(pos);
                } else if added.is_none() {
                    added = Some(t);
                } else {
                    return fallback();
                }
            }
            if !lower.is_empty() {
                return fallback();
            }
            match added {
                Some(t) => seq.push(t),
                None => return fallback(),
            }
        }
        if seq.iter().all(|t| t.chars().count() == 1) {
            seq.concat()
        } else {
            seq.join(",")
        }
    }
}

// ===========================================================================
// wasm-bindgen steppers (same step/budget contract as web::StripEnumerator)
// ===========================================================================

/// Sliceable survey over all connected graphs on ≤ n vertices.
#[wasm_bindgen]
pub struct GraphSurvey {
    core: api::SurveyCore,
}

#[wasm_bindgen]
impl GraphSurvey {
    #[wasm_bindgen(constructor)]
    pub fn new(max_n: usize, check_linear: bool, check_cyclic: bool) -> Result<GraphSurvey, JsValue> {
        api::SurveyCore::new(max_n, check_linear, check_cyclic)
            .map(|core| GraphSurvey { core })
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Advance for about `budget_ms`. Returns JSON (see [`api::SurveyStep`]).
    pub fn step(&mut self, budget_ms: f64) -> String {
        serde_json::to_string(&self.core.step(budget_ms)).expect("SurveyStep serializes")
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairOut {
    left: String,
    right: String,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BoundaryStep {
    /// Full tally so far, sorted by count (desc), then lexicographically.
    pairs: Vec<PairOut>,
    /// Strips enumerated so far.
    count: usize,
    distinct: usize,
    done: bool,
}

/// Enumerates all linear rhombic strips of one poset and tallies the pairs
/// (left boundary, right boundary), each boundary chain read bottom-to-top
/// as a permutation (see [`api::chain_to_perm`]).
///
/// Cyclic strips have no boundary, so this is inherently non-cyclic.
/// Lattice ownership follows `web::StripEnumerator`: leaked so the borrowing
/// iterator can live alongside, reclaimed in `Drop`.
#[wasm_bindgen]
pub struct BoundaryEnumerator {
    lattice: *mut Lattice,
    iter: Option<Box<dyn Iterator<Item = Strip>>>,
    pairs: HashMap<(String, String), usize>,
    count: usize,
    done: bool,
}

#[wasm_bindgen]
impl BoundaryEnumerator {
    #[wasm_bindgen(constructor)]
    pub fn new(graph_json: &str) -> Result<BoundaryEnumerator, JsValue> {
        Self::create(graph_json).map_err(|e| JsValue::from_str(&e))
    }

    /// Advance for about `budget_ms`. Returns JSON (see [`BoundaryStep`]).
    pub fn step(&mut self, budget_ms: f64) -> String {
        let deadline = now_ms() + budget_ms;
        if !self.done {
            // SAFETY: see `create` — the lattice outlives every borrow here.
            let l: &Lattice = unsafe { &*self.lattice };
            let iter = self.iter.as_mut().expect("iterator present until done");
            loop {
                match iter.next() {
                    Some(strip) => {
                        let chain = |pick: fn(&[usize]) -> usize| -> Vec<String> {
                            strip
                                .iter()
                                .map(|layer| l.face(pick(layer)).label().to_string())
                                .collect()
                        };
                        let left = api::chain_to_perm(&chain(|layer| layer[0]));
                        let right =
                            api::chain_to_perm(&chain(|layer| *layer.last().unwrap()));
                        *self.pairs.entry((left, right)).or_insert(0) += 1;
                        self.count += 1;
                    }
                    None => {
                        self.done = true;
                        self.iter = None; // release the borrow eagerly
                        break;
                    }
                }
                if now_ms() >= deadline {
                    break;
                }
            }
        }

        let mut pairs: Vec<PairOut> = self
            .pairs
            .iter()
            .map(|((left, right), &count)| PairOut {
                left: left.clone(),
                right: right.clone(),
                count,
            })
            .collect();
        pairs.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.left.cmp(&b.left))
                .then_with(|| a.right.cmp(&b.right))
        });
        let out = BoundaryStep {
            distinct: pairs.len(),
            pairs,
            count: self.count,
            done: self.done,
        };
        serde_json::to_string(&out).expect("BoundaryStep serializes")
    }
}

impl BoundaryEnumerator {
    fn create(graph_json: &str) -> Result<Self, String> {
        let g = webapi::WireGraph::parse(graph_json)?;
        let faces = webapi::wire_to_faces(&g)?;
        let lattice: *mut Lattice = Box::into_raw(Box::new(Lattice::from_faces(faces)));
        // SAFETY: the iterator borrows the leaked lattice; it is dropped
        // before the lattice in `Drop`, and `lattice` is never moved.
        let iter: Box<dyn Iterator<Item = Strip>> =
            Box::new(rhombic::strips(unsafe { &*lattice }, false));
        Ok(BoundaryEnumerator {
            lattice,
            iter: Some(iter),
            pairs: HashMap::new(),
            count: 0,
            done: false,
        })
    }
}

impl Drop for BoundaryEnumerator {
    fn drop(&mut self) {
        self.iter = None; // the borrower goes first
        if !self.lattice.is_null() {
            // SAFETY: allocated with Box::into_raw in `create`, dropped once.
            unsafe { drop(Box::from_raw(self.lattice)) };
            self.lattice = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::api::*;
    use super::*;

    fn run_survey(max_n: usize, linear: bool, cyclic: bool) -> Vec<GraphResult> {
        let mut core = SurveyCore::new(max_n, linear, cyclic).expect("survey builds");
        let mut results = vec![];
        loop {
            let step = core.step(f64::INFINITY);
            results.extend(step.results);
            if step.done {
                assert_eq!(step.checked, step.total);
                return results;
            }
        }
    }

    #[test]
    fn survey_counts_match_oeis() {
        // Connected graphs up to isomorphism (OEIS A001349): 1, 2, 6, 21, 112.
        let results = run_survey(6, false, false);
        let count = |n| results.iter().filter(|r| r.n == n).count();
        assert_eq!(count(2), 1);
        assert_eq!(count(3), 2);
        assert_eq!(count(4), 6);
        assert_eq!(count(5), 21);
        assert_eq!(count(6), 112);
    }

    #[test]
    fn hamiltonicity_spot_checks() {
        // path P4: Hamilton path, no cycle
        let p4 = adjacency(4, {
            let e = edges_to_mask(4, &[(0, 1), (1, 2), (2, 3)]);
            e
        });
        assert!(ham_path(4, &p4) && !ham_cycle(4, &p4));
        // cycle C4: both
        let c4 = adjacency(4, edges_to_mask(4, &[(0, 1), (1, 2), (2, 3), (0, 3)]));
        assert!(ham_path(4, &c4) && ham_cycle(4, &c4));
        // star K_{1,3}: neither
        let star = adjacency(4, edges_to_mask(4, &[(0, 1), (0, 2), (0, 3)]));
        assert!(!ham_path(4, &star) && !ham_cycle(4, &star));
    }

    fn edges_to_mask(n: usize, edges: &[(usize, usize)]) -> u64 {
        let mut m = 0;
        for &(a, b) in edges {
            let (a, b) = if a < b { (a, b) } else { (b, a) };
            m |= 1u64 << (a * n + b);
        }
        m
    }

    #[test]
    fn tube_poset_of_p3_matches_web_generator() {
        // Same graph through both code paths: scripts::api::tube_poset and
        // web::api::gen_tube_poset must agree on faces per level.
        let adj = adjacency(3, edges_to_mask(3, &[(0, 1), (1, 2)]));
        let lat = tube_poset(3, &adj);
        assert_eq!(lat.num_faces(), 6); // {0},{1},{2},{01},{12},{012}
        assert_eq!(lat.level(0).len(), 3);
        assert_eq!(lat.level(1).len(), 2);
        assert_eq!(lat.level(2).len(), 1);

        let via_web = crate::web::api::gen_tube_poset(
            &crate::web::api::gen_graph("path", 3).unwrap(),
        )
        .unwrap();
        let g = crate::web::api::WireGraph::parse(&via_web).unwrap();
        assert_eq!(g.labels.len(), lat.num_faces());
        assert_eq!(
            g.edges.len(),
            lat.faces().map(|(_, f)| f.upset().len()).sum::<usize>()
        );
    }

    #[test]
    fn survey_small_graphs_strip_results() {
        let results = run_survey(3, true, true);
        assert_eq!(results.len(), 3); // K2, P3, K3
        for r in &results {
            // Level 0 of the tube poset is the graph itself (singletons are
            // bridged iff adjacent), so a strip forces a Hamilton path...
            if r.strip == Some(true) {
                assert!(r.ham_path);
            }
            // ...and on ≤ 3 vertices the conjectured converse holds too.
            if r.ham_path {
                assert_eq!(r.strip, Some(true));
            }
        }
        // K3: Hamilton cycle and a cyclic strip.
        let k3 = results.iter().find(|r| r.n == 3 && r.edges.len() == 3).unwrap();
        assert!(k3.ham_cycle);
        assert_eq!(k3.cyclic_strip, Some(true));
    }

    #[test]
    fn chain_to_perm_tokenises_and_falls_back() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // single-char tokens concatenate
        assert_eq!(chain_to_perm(&s(&["0", "01", "012"])), "012");
        assert_eq!(chain_to_perm(&s(&["2", "12", "012"])), "210");
        // comma-mode chains with single-char tokens still concatenate
        assert_eq!(chain_to_perm(&s(&["b", "a,b", "a,b,c"])), "bac");
        // multi-char tokens join with commas
        assert_eq!(chain_to_perm(&s(&["v1", "v1,v2", "v1,v2,v3"])), "v1,v2,v3");
        // cube-style labels don't add tokens: fall back to the chain
        assert_eq!(chain_to_perm(&s(&["00", "0*", "**"])), "00 < 0* < **");
    }

    #[test]
    fn boundary_pairs_of_p3_tube_poset() {
        // Tube poset of the path 0-1-2 has exactly one strip:
        // [0,1,2] / [01,12] / [012]; boundaries 0<01<012 and 2<12<012.
        let g = crate::web::api::gen_tube_poset(
            &crate::web::api::gen_graph("path", 3).unwrap(),
        )
        .unwrap();
        let mut en = BoundaryEnumerator::create(&g).expect("enumerator builds");
        let out: serde_json::Value =
            serde_json::from_str(&en.step(f64::INFINITY)).unwrap();
        assert_eq!(out["done"], true);
        assert_eq!(out["count"], 1);
        assert_eq!(out["distinct"], 1);
        assert_eq!(out["pairs"][0]["left"], "012");
        assert_eq!(out["pairs"][0]["right"], "210");
        assert_eq!(out["pairs"][0]["count"], 1);
    }

    #[test]
    fn boundary_counts_sum_to_strip_count() {
        // Boolean lattice B3 (simplex face lattice): pair counts must sum to
        // the total number of strips reported by the sequential search.
        let g = crate::web::api::gen_simplex(2).unwrap(); // subsets of {0,1,2}
        let faces =
            crate::web::api::wire_to_faces(&crate::web::api::WireGraph::parse(&g).unwrap())
                .unwrap();
        let lat = crate::lattice::Lattice::from_faces(faces);
        let total = crate::rhombic::strips(&lat, false).count();
        assert!(total > 0);

        let mut en = BoundaryEnumerator::create(&g).expect("enumerator builds");
        let out: serde_json::Value =
            serde_json::from_str(&en.step(f64::INFINITY)).unwrap();
        assert_eq!(out["count"], total as u64);
        let sum: u64 = out["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["count"].as_u64().unwrap())
            .sum();
        assert_eq!(sum, total as u64);
    }
}
