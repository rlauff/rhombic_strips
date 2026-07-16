//! Rhombic strips in graded posets.
//!
//! A strip is a sequence of layers, one per dimension level of the lattice,
//! where layer `d+1` is obtained from layer `d` by inserting the bridges
//! between consecutive faces and distributing the remaining faces of level
//! `d+1` into the gaps.
//!
//! Entry points, all lazy where possible:
//! * [`strips`] / [`strips_parallel`] — all strips of a lattice
//! * [`count_strips`] — number of strips without storing them
//! * [`strip_exists`] — existence check with early exit
//! * [`extensions`] — all completions of a partial strip
//! * [`next_layers`] — all valid successor layers of a single layer

use crate::lattice::{FaceId, Lattice};
// rayon needs OS threads, which wasm32-unknown-unknown lacks. The parallel
// entry points below are native-only; the browser uses the sequential
// `strips` iterator (rayon-free) driven by web::StripEnumerator.
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

/// One level of a strip: a sequence of face ids of equal dimension.
pub type Layer = Vec<FaceId>;
/// A strip: one layer per dimension, from dim 0 up.
pub type Strip = Vec<Layer>;

// ---------------------------------------------------------------------------
// Layer predicates and assembly
// ---------------------------------------------------------------------------

/// A layer is ok if, after collapsing runs of equal faces into groups, no
/// face appears in two different groups. In the cyclic case the sequence is
/// read cyclically, so a wrap-around run counts as one group.
fn layer_ok(layer: &[FaceId], cyclic: bool) -> bool {
    if layer.is_empty() {
        return true;
    }

    let mut groups = Vec::with_capacity(layer.len());
    groups.push(layer[0]);
    for &val in &layer[1..] {
        if val != *groups.last().unwrap() {
            groups.push(val);
        }
    }

    if cyclic && groups.len() > 1 && groups.first() == groups.last() {
        groups.pop();
    }

    for i in 0..groups.len() {
        for j in (i + 1)..groups.len() {
            if groups[i] == groups[j] {
                return false;
            }
        }
    }
    true
}

/// Interleave gap contents and bridges: g_0 b_0 g_1 b_1 ... (plus a trailing
/// gap in the non-cyclic case, where there is one more gap than bridges).
fn combine_to_layer(bridges: &[FaceId], gaps: &[Vec<FaceId>]) -> Layer {
    let total_len = bridges.len() + gaps.iter().map(Vec::len).sum::<usize>();
    let mut layer = Vec::with_capacity(total_len);

    for (i, &b) in bridges.iter().enumerate() {
        layer.extend_from_slice(&gaps[i]);
        layer.push(b);
    }
    if gaps.len() > bridges.len() {
        layer.extend_from_slice(&gaps[gaps.len() - 1]);
    }
    layer
}

/// Collapse consecutive duplicates; additionally drop later occurrences of
/// the first element (cyclic wrap-around duplicates).
fn duplicates_removed(v: Layer) -> Layer {
    if v.is_empty() {
        return vec![];
    }
    let mut res = Vec::with_capacity(v.len());
    res.push(v[0]);
    for i in 1..v.len() {
        if v[i - 1] != v[i] && v[i] != v[0] {
            res.push(v[i]);
        }
    }
    res
}

// ---------------------------------------------------------------------------
// Gap assignments: distribute faces into gaps, in all orders
// ---------------------------------------------------------------------------

/// Lazily enumerate all ways to distribute `faces_to_place` into the given
/// gaps (each face only into gaps that allow it), including all orderings
/// within a gap. Explicit DFS stack instead of a cartesian-product iterator,
/// so only O(depth) state is kept.
///
/// Invariants:
/// * `stack[d] = (gap, slot)` is the placement of `faces_to_place[d]`.
/// * `buckets` always reflects exactly the placements on the stack.
/// * `search_cursor` is the next (gap, slot) to try at the current depth.
pub struct GapAssignmentIterator {
    faces_to_place: Vec<FaceId>,
    /// For each face (by position in `faces_to_place`): the gap indices that
    /// allow it. Precomputed inversion of the `gaps_allowed` input.
    allowed_gaps_per_face: Vec<Vec<usize>>,

    stack: Vec<(usize, usize)>,
    buckets: Vec<Vec<FaceId>>,
    search_cursor: (usize, usize),

    is_done: bool,
    has_yielded_initial: bool, // for the empty faces_to_place case
}

impl GapAssignmentIterator {
    pub fn new(gaps_allowed: Vec<Vec<FaceId>>, faces_to_place: Vec<FaceId>) -> Self {
        let n_gaps = gaps_allowed.len();
        let allowed_gaps_per_face = faces_to_place
            .iter()
            .map(|&face| {
                (0..n_gaps)
                    .filter(|&i| gaps_allowed[i].contains(&face))
                    .collect()
            })
            .collect();

        Self {
            faces_to_place,
            allowed_gaps_per_face,
            stack: Vec::with_capacity(n_gaps),
            buckets: vec![vec![]; n_gaps],
            search_cursor: (0, 0),
            is_done: false,
            has_yielded_initial: false,
        }
    }
}

impl Iterator for GapAssignmentIterator {
    type Item = Vec<Vec<FaceId>>;

    fn next(&mut self) -> Option<Self::Item> {
        // no faces: exactly one (empty) assignment
        if self.faces_to_place.is_empty() {
            if !self.has_yielded_initial {
                self.has_yielded_initial = true;
                return Some(self.buckets.clone());
            }
            return None;
        }
        if self.is_done {
            return None;
        }

        loop {
            let depth = self.stack.len();

            // 1. all faces placed: yield, then backtrack one step
            if depth == self.faces_to_place.len() {
                let result = self.buckets.clone();

                let (last_gap, last_slot) = self.stack.pop().unwrap();
                self.buckets[last_gap].remove(last_slot);
                self.search_cursor = (last_gap, last_slot + 1);

                return Some(result);
            }

            // 2. find the next valid move for the current face
            let face = self.faces_to_place[depth];
            let allowed_gaps = &self.allowed_gaps_per_face[depth];
            let mut found_move = false;

            let start = allowed_gaps
                .iter()
                .position(|&g| g >= self.search_cursor.0)
                .unwrap_or(allowed_gaps.len());

            for &gap_idx in &allowed_gaps[start..] {
                let bucket_len = self.buckets[gap_idx].len();
                // resume at the cursor slot in the cursor gap, else at slot 0
                let start_slot = if gap_idx == self.search_cursor.0 {
                    self.search_cursor.1
                } else {
                    0
                };

                // insertion slots run from 0 to len inclusive
                if start_slot <= bucket_len {
                    self.buckets[gap_idx].insert(start_slot, face);
                    self.stack.push((gap_idx, start_slot));
                    self.search_cursor = (0, 0); // fresh cursor for next depth
                    found_move = true;
                    break;
                }
            }

            if found_move {
                continue;
            }

            // 3. no move at this depth: backtrack
            if self.stack.is_empty() {
                self.is_done = true;
                return None;
            }
            let (prev_gap, prev_slot) = self.stack.pop().unwrap();
            self.buckets[prev_gap].remove(prev_slot);
            self.search_cursor = (prev_gap, prev_slot + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Layer successors
// ---------------------------------------------------------------------------

/// Lazily enumerate all valid layers of dimension `d+1` following the given
/// layer of dimension `d`. The returned iterator owns all its data.
pub fn next_layers(
    last_layer: &[FaceId],
    l: &Lattice,
    cyclic: bool,
) -> impl Iterator<Item = Layer> + Send {
    debug_assert!(!last_layer.is_empty(), "next_layers: empty layer");

    let dim = l.face(last_layer[0]).dim();
    let n = last_layer.len();
    let num_bridges = match (cyclic, n) {
        (true, 1) => 0,      // single face: nothing to bridge
        (true, 2) => 1,      // the one bridge already closes the cycle
        (true, _) => n,      // cyclic: as many bridges as faces
        (false, _) => n - 1, // linear: one less
    };

    // bridges between consecutive layer faces; all must exist
    let bridges: Option<Vec<FaceId>> = (0..num_bridges)
        .map(|x| l.bridge(last_layer[x], last_layer[(x + 1) % n]))
        .collect();

    let bridges = match bridges {
        Some(b) if layer_ok(&b, cyclic) => b,
        _ => return itertools::Either::Left(std::iter::empty()),
    };

    // membership mask for the bridges (avoids repeated linear scans)
    let mut is_bridge = vec![false; l.num_faces()];
    for &b in &bridges {
        is_bridge[b] = true;
    }

    // faces of level d+1 that are not bridges must be placed into gaps
    let faces_left: Vec<FaceId> = l
        .level(dim + 1)
        .iter()
        .copied()
        .filter(|&x| !is_bridge[x])
        .collect();

    // gap i may contain any non-bridge face covering last_layer[i]
    let gaps: Vec<Vec<FaceId>> = last_layer
        .iter()
        .map(|&f| {
            l.face(f)
                .upset()
                .iter()
                .copied()
                .filter(|&x| !is_bridge[x])
                .collect()
        })
        .collect();

    let iter = GapAssignmentIterator::new(gaps, faces_left)
        .map(move |assignment| combine_to_layer(&bridges, &assignment))
        .filter(move |layer| layer_ok(layer, cyclic))
        .map(duplicates_removed);

    itertools::Either::Right(iter)
}

// ---------------------------------------------------------------------------
// Strips
// ---------------------------------------------------------------------------

/// Lazily enumerate all completions of a partial strip up to dimension
/// `max_dim`. The strip must be non-empty; its last layer is extended.
pub fn extensions<'a>(
    strip: Strip,
    l: &'a Lattice,
    max_dim: usize,
    cyclic: bool,
) -> Box<dyn Iterator<Item = Strip> + Send + 'a> {
    if strip.len() == max_dim + 1 {
        return Box::new(std::iter::once(strip));
    }
    let last = strip.last().expect("extensions: empty strip").clone();
    Box::new(next_layers(&last, l, cyclic).flat_map(move |layer| {
        let mut extended = strip.clone();
        extended.push(layer);
        extensions(extended, l, max_dim, cyclic)
    }))
}

/// Lazily enumerate all rhombic strips of the lattice, from dimension 0 up to
/// `l.dim()`. Sequential; supports early exit (`.next()`, `.take(k)`, ...).
pub fn strips(l: &Lattice, cyclic: bool) -> impl Iterator<Item = Strip> + '_ {
    let max_dim = l.dim();
    l.ham_paths(cyclic)
        .flat_map(move |path| extensions(vec![path], l, max_dim, cyclic))
}

/// How many independent search branches to split into: enough that rayon can
/// balance uneven subtrees across `threads` workers.
#[cfg(not(target_arch = "wasm32"))]
fn seed_target() -> usize {
    rayon::current_num_threads() * 16
}

/// All rhombic strips of the lattice, computed in parallel.
///
/// Parallelises over seeds of the hamiltonian-path DFS itself
/// (`Lattice::ham_path_seeds`): each worker owns an independent subtree of
/// the path search plus all its strip extensions. A `par_bridge()` over the
/// sequential `ham_paths` iterator would leave every core but one idle
/// whenever *finding* the paths dominates — which is the typical hard case.
#[cfg(not(target_arch = "wasm32"))]
pub fn strips_parallel(l: &Lattice, cyclic: bool) -> Vec<Strip> {
    let max_dim = l.dim();
    l.ham_path_seeds(cyclic, seed_target())
        .into_par_iter()
        .flat_map_iter(|paths| {
            paths.flat_map(move |path| {
                if max_dim == 0 {
                    itertools::Either::Left(std::iter::once(vec![path]))
                } else {
                    itertools::Either::Right(extensions(vec![path], l, max_dim, cyclic))
                }
            })
        })
        .collect()
}

/// Count all rhombic strips without storing them.
///
/// Native-only (parallel over DFS seeds, see [`strips_parallel`]). In the
/// browser, `web::StripEnumerator` counts by draining the sequential
/// [`strips`] iterator, so it needs no rayon.
#[cfg(not(target_arch = "wasm32"))]
pub fn count_strips(l: &Lattice, cyclic: bool) -> usize {
    let max_dim = l.dim();
    l.ham_path_seeds(cyclic, seed_target())
        .into_par_iter()
        .map(|paths| {
            paths
                .map(|path| {
                    if max_dim == 0 {
                        1
                    } else {
                        extensions(vec![path], l, max_dim, cyclic).count()
                    }
                })
                .sum::<usize>()
        })
        .sum()
}

/// Does the given layer of dimension `current_dim` extend to a full strip up
/// to `max_dim`? Sequential with early exit.
pub fn layer_extends(
    layer: &[FaceId],
    current_dim: usize,
    l: &Lattice,
    max_dim: usize,
    cyclic: bool,
) -> bool {
    if current_dim == max_dim {
        return true;
    }
    next_layers(layer, l, cyclic)
        .any(|next| layer_extends(&next, current_dim + 1, l, max_dim, cyclic))
}

/// Does the lattice admit a rhombic strip at all? Parallel over seeds of the
/// hamiltonian-path DFS, with a shared flag so all workers stop as soon as
/// any of them finds a strip.
///
/// Native-only (parallel). In the browser, `web::StripEnumerator` in "exists"
/// mode pulls a single item from the sequential [`strips`] iterator instead.
#[cfg(not(target_arch = "wasm32"))]
pub fn strip_exists(l: &Lattice, cyclic: bool) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    let max_dim = l.dim();
    let found = AtomicBool::new(false);
    l.ham_path_seeds(cyclic, seed_target())
        .into_par_iter()
        .any(|paths| {
            for path in paths {
                if found.load(Ordering::Relaxed) {
                    return false; // someone else already succeeded
                }
                if layer_extends(&path, 0, l, max_dim, cyclic) {
                    found.store(true, Ordering::Relaxed);
                    return true;
                }
            }
            false
        })
}
