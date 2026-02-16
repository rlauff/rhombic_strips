use crate::lattice::*;
use rayon::prelude::*;
use std::iter::Iterator;

fn layer_ok(layer: &Vec<usize>, cyclic: bool) -> bool {
    if layer.is_empty() {
        return true;
    }

    // Optimization: Pre-allocate capacity to avoid reallocations
    let mut groups = Vec::with_capacity(layer.len());

    groups.push(layer[0]);
    for &val in layer.iter().skip(1) {
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

fn combine_to_layer(bridges: &Vec<usize>, gaps: &Vec<Vec<usize>>) -> Vec<usize> {
    // Optimization: Calculate exact size needed
    let total_len = bridges.len() + gaps.iter().map(|g| g.len()).sum::<usize>();
    let mut layer = Vec::with_capacity(total_len);

    for i in 0..bridges.len() {
        // Gaps in lazy iterator come as Vec<usize>, simply push contents
        // Optimization: Use extend_from_slice for bulk copy
        layer.extend_from_slice(&gaps[i]);
        layer.push(bridges[i]);
    }
    if gaps.len() > bridges.len() {
        // Optimization: Avoid clone(), use extend directly
        layer.extend_from_slice(&gaps[gaps.len()-1]); 
    }
    layer
}

fn duplicates_removed(v: Vec<usize>) -> Vec<usize> {
    if v.len() == 0 { return vec![] };
    // Optimization: Pre-allocate capacity
    let mut res = Vec::with_capacity(v.len());
    res.push(v[0]);
    for i in 1..v.len() {
        if v[i-1] != v[i] && v[i] != v[0] {
            res.push(v[i]);
        }
    }
    res
}

/// LAZY GAP ASSIGNMENTS (OPTIMIZED)
/// Instead of generating huge intermediate iterators via multi_cartesian_product,
/// this uses an explicit iterative Depth-First Search (DFS) stack.
///
/// Logic:
/// 1. Pre-calculate valid gaps for each face to speed up lookup.
/// 2. Maintain a single mutable `buckets` state.
/// 3. Push faces one by one. For each face, try inserting it into every valid gap
///    at every possible position (slot).
/// 4. Inserting at specific indices covers both "assignment" and "permutation".
pub struct GapAssignmentIterator {
    // Static data
    faces_to_place: Vec<usize>,
    allowed_gaps_per_face: Vec<Vec<usize>>, // Pre-computed optimization
    
    // Dynamic state (DFS Stack)
    // Stack stores the decision made at each depth: (gap_index, slot_index)
    stack: Vec<(usize, usize)>,
    
    // The current state of assignments. We mutate this in-place.
    buckets: Vec<Vec<usize>>,
    
    // Cursor to resume search after yielding or backtracking.
    // Represents (next_gap_idx, next_slot_idx) to try for the current depth.
    search_cursor: (usize, usize),
    
    // State flags
    is_done: bool,
    has_yielded_initial: bool, // Handle case where faces_to_place is empty
}

impl GapAssignmentIterator {
    pub fn new(gaps_allowed: Vec<Vec<usize>>, faces_to_place: Vec<usize>) -> Self {
        let n_gaps = gaps_allowed.len();
        
        // Invert the mapping: For each face, which gap indices allow it?
        // optimization: allows O(1) access during the tight inner loop
        let allowed_gaps_per_face: Vec<Vec<usize>> = faces_to_place.iter().map(|&face| {
            (0..n_gaps).filter(|&i| gaps_allowed[i].contains(&face)).collect()
        }).collect();

        Self {
            faces_to_place,
            allowed_gaps_per_face,
            stack: Vec::with_capacity(n_gaps), // Reserve efficient capacity
            buckets: vec![vec![]; n_gaps],
            search_cursor: (0, 0),
            is_done: false,
            has_yielded_initial: false,
        }
    }
}

impl Iterator for GapAssignmentIterator {
    type Item = Vec<Vec<usize>>;

    fn next(&mut self) -> Option<Self::Item> {
        // Handle edge case: No faces to place. Should yield one empty configuration.
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

            // 1. Solution found?
            // If stack depth matches number of faces, we have successfully placed everyone.
            if depth == self.faces_to_place.len() {
                // Snapshot the current valid state
                let result = self.buckets.clone();

                // Prepare for next iteration: Backtrack one step
                // We pop the last move and advance the cursor to try the next option
                let (last_gap, last_slot) = self.stack.pop().unwrap();
                self.buckets[last_gap].remove(last_slot);
                
                // Resume search at the *next* slot after the one we just used
                self.search_cursor = (last_gap, last_slot + 1);

                return Some(result);
            }

            // 2. Search for the next valid move for the current face
            let face = self.faces_to_place[depth];
            let allowed_gaps = &self.allowed_gaps_per_face[depth];
            let mut found_move = false;

            // Iterate through valid gaps starting from cursor position
            // We use standard loop indices to avoid iterator allocation in tight loop
            let start_gap_idx_in_allowed = allowed_gaps.iter()
                .position(|&g| g >= self.search_cursor.0)
                .unwrap_or(allowed_gaps.len());

            for &gap_idx in &allowed_gaps[start_gap_idx_in_allowed..] {
                let bucket_len = self.buckets[gap_idx].len();
                
                // Determine starting slot. 
                // If we are in the same gap as the cursor, respect the cursor slot.
                // If we moved to a new gap, start at slot 0.
                let start_slot = if gap_idx == self.search_cursor.0 { 
                    self.search_cursor.1 
                } else { 
                    0 
                };

                // Try all insertion slots: 0 to len (inclusive for insert)
                // Using a loop here instead of iterators allows early break
                if start_slot <= bucket_len {
                    // Valid move found!
                    let slot_idx = start_slot; // Take the first available slot
                    
                    // Apply move
                    self.buckets[gap_idx].insert(slot_idx, face);
                    self.stack.push((gap_idx, slot_idx));

                    // Reset cursor for the NEXT depth level (start fresh)
                    self.search_cursor = (0, 0);
                    
                    found_move = true;
                    break; 
                }
            }

            if found_move {
                // Continue loop to go deeper (depth + 1)
                continue;
            }

            // 3. No moves possible at this depth? Backtrack.
            if self.stack.is_empty() {
                // We backtracked all the way to the top and found no more options.
                self.is_done = true;
                return None;
            }

            let (prev_gap, prev_slot) = self.stack.pop().unwrap();
            // Undo the previous move
            self.buckets[prev_gap].remove(prev_slot);
            
            // Set cursor to try the next possibility for the *previous* face
            self.search_cursor = (prev_gap, prev_slot + 1);
            
            // Loop continues with reduced depth
        }
    }
}

// Wrapper function to maintain your API signature
fn gap_assignments_lazy(
    gaps_allowed: Vec<Vec<usize>>, 
    faces_to_place: Vec<usize>
) -> impl Iterator<Item = Vec<Vec<usize>>> {
    GapAssignmentIterator::new(gaps_allowed, faces_to_place)
}

pub fn next_layers_lazy<'a>(last_layer: &'a Vec<usize>, l: &'a Lattice, cyclic: bool) -> impl Iterator<Item = Vec<usize>> + 'a {

    let dim = l.faces[last_layer[0]].dim;
    let n = last_layer.len();
    let bridges_upper = if cyclic { n } else { n - 1 };
    
    let bridges: Vec<_> = (0..bridges_upper)
        .map(|x| l.bridges[&(last_layer[x], last_layer[(x+1) % n])])
        .collect();

    if !layer_ok(&bridges, cyclic) { 
        return itertools::Either::Left(std::iter::empty()); 
    };   

    let mut faces_left = Vec::with_capacity(l.levels[dim+1].len());
    for x in l.levels[dim+1].iter() {
        if !bridges.contains(x) {
            faces_left.push(*x);
        }
    }

    // Optimization: Pre-allocate vector
    let mut gaps = Vec::with_capacity(n);
    for i in 0..n {
        let mut new_gap = Vec::with_capacity(l.faces[last_layer[i]].upset.len());
        for x in l.faces[last_layer[i]].upset.iter() {
            if !bridges.contains(x) {
                new_gap.push(*x);
            }
        }
        gaps.push(new_gap);
    }

    let iter = gap_assignments_lazy(gaps, faces_left)
        .map(move |x| combine_to_layer(&bridges, &x))
        .filter(move |x| layer_ok(x, cyclic))
        .map(|x| duplicates_removed(x));

    itertools::Either::Right(iter)
}

// The main function replacing rhombic_strips_dfs_simple
pub fn rhombic_strips_dfs_lazy(
    strip: Vec<Vec<usize>>, 
    l: &Lattice, 
    max_dim: usize, 
    cyclic: bool
) -> Vec<Vec<Vec<usize>>> {

    // Base case: if we reached the max dimension, return the current strip
    if max_dim == strip.len() - 1 {
        return vec![strip];
    }

    // Use par_bridge() to parallelize the consumption of the lazy iterator.
    // This allows us to process branches in parallel as they are generated,
    // without ever storing all possible next layers in memory at once.
    next_layers_lazy(&strip[strip.len()-1], l, cyclic).par_bridge()
        .map(|next_layer| {
            let mut new_strip = strip.clone();
            new_strip.push(next_layer);
            rhombic_strips_dfs_lazy(new_strip, l, max_dim, cyclic)
        })
        .flatten()
        .collect()
}

/// Optimized Existence Check using Lazy Generation + ParBridge
pub fn rhombic_strip_exists(
    current_layer: &Vec<usize>, 
    current_dim: usize, 
    l: &Lattice, 
    max_dim: usize, 
    cyclic: bool
) -> bool {

    if current_dim == max_dim {
        return true;
    }

    let next_iter = next_layers_lazy(current_layer, l, cyclic);
    // par_bridge converts the sequential iterator into a parallel one.
    // It pulls items from the iterator on one thread and distributes processing to others.
    next_iter.par_bridge().any(|next_layer| {
        rhombic_strip_exists(&next_layer, current_dim + 1, l, max_dim, cyclic)
    })
}