//the actual code to construct the rhombic strip

use crate::lattice::*;
use rayon::prelude::*;
use itertools::Itertools;
use petgraph::graph::Graph;
use petgraph::Undirected;
use rustsat::{
    solvers::{Solve, SolveIncremental, SolverResult, SolveStats},
    types::{Clause, Lit, TernaryVal},
};
use rustsat_cadical::CaDiCaL;
use std::cmp::{ min, max };
use std::collections::HashSet;

pub fn shift_min_to_front(cycle: &mut Vec<usize>) {
    if let Some((min_index, _)) = cycle.iter().enumerate().min_by_key(|&(_, val)| val) {
        cycle.rotate_left(min_index);
    }
}

// given a graph, uses CaDiCaL to find all Hamilton paths in this graph. 
pub fn all_ham_paths(graph: &Graph<usize, (), Undirected>, restrict_to_cycles: bool) -> Vec<Vec<usize>> {
    let node_count = graph.node_count();
    println!("Running on nodes: {}", node_count);

    // Trivial cases
    if node_count == 0 {
        return vec![];
    }

    let mut solver = CaDiCaL::default();

    // Variable mapping:
    // We need variables x_{v, i} representing "Vertex v is at position i in the path".
    // We map these to a linear variable index: var = v * node_count + i

    // helper to create a literal for x_{v, i}
    let var_for = |v_idx: usize, pos_idx: usize| -> Lit {
        let var_idx = v_idx * node_count + pos_idx;
        Lit::positive(var_idx as u32)
    };

    // 1. Each position i must be occupied by at least one vertex
    for i in 0..node_count {
        let mut clause = Clause::new();
        for v in 0..node_count {
            clause.add(var_for(v, i));
        }
        solver.add_clause(clause);
    }

    // 2. Each position i must be occupied by at most one vertex
    for i in 0..node_count {
        for v in 0..node_count {
            for w in (v + 1)..node_count {
                solver.add_clause(Clause::from_iter([
                    !var_for(v, i),
                    !var_for(w, i)
                ]));
            }
        }
    }

    // 3. Each vertex v must appear at least once in the path
    for v in 0..node_count {
        let mut clause = Clause::new();
        for i in 0..node_count {
            clause.add(var_for(v, i));
        }
        solver.add_clause(clause);
    }

    // 4. Each vertex v must appear at most once in the path
    for v in 0..node_count {
        for i in 0..node_count {
            for j in (i + 1)..node_count {
                solver.add_clause(Clause::from_iter([
                    !var_for(v, i),
                    !var_for(v, j)
                ])).expect("failed to add clause");
            }
        }
    }

    // 5. Adjacency constraints (The path/cycle must follow edges)
    // We collect indices first to ensure stable mapping between v_idx (0..N) and graph nodes
    let node_indices: Vec<_> = graph.node_indices().collect();

    for (v_idx, &node_idx) in node_indices.iter().enumerate() {
        let neighbors: Vec<_> = graph.neighbors(node_idx).collect();

        for i in 0..node_count {
            if i == node_count-1 && !restrict_to_cycles { break; };
            let next_i = (i + 1) % node_count;

            let mut clause = Clause::new();

            // If v is at i...
            clause.add(!var_for(v_idx, i));

            // ...then one of the neighbors must be at i+1
            for neighbor_node_idx in &neighbors {
                if let Some(neighbor_v_idx) = node_indices.iter().position(|n| n == neighbor_node_idx) {
                    clause.add(var_for(neighbor_v_idx, next_i));
                }
            }

            solver.add_clause(clause);
        }
    }

    let mut ham_cycles = Vec::new();
    loop {
        let result = solver.solve().expect("Solver error");

        match result {
            SolverResult::Sat => {
                let sol = solver.full_solution().unwrap();
                let mut ham_cycle = Vec::new();

                // Incremental: Block the found solution to find the next one
                // We construct the negation of the current solution vector
                let mut blocking_clause = Clause::new();
                for i in 0..node_count {
                    for v in 0..node_count {
                        let lit = var_for(v, i);
                        // If the variable was true in the model, add its negation to the clause
                        if sol[lit.var()] == TernaryVal::True {
                            blocking_clause.add(!lit);

                            // BUG FIX: Map the internal index 'v' back to the actual graph node weight.
                            // Previously: ham_cycle.push(v); -> this only pushed 0,1,2...
                            let graph_node_index = node_indices[v];
                            let real_value = graph[graph_node_index];
                            ham_cycle.push(real_value);
                        }
                    }
                }
                solver.add_clause(blocking_clause);
                ham_cycles.push(ham_cycle);
            },
            SolverResult::Unsat => {
                break;
            },
            SolverResult::Interrupted => {
                panic!("Solver got interruped");
            }
        }
    }
    // if restrict_to_cycles , we shift the array to a canonical starting point and then deduplicate.
    if restrict_to_cycles {
        for cycle in ham_cycles.iter_mut() {
            shift_min_to_front(cycle);
        }
    }
    let mut ret = Vec::with_capacity(ham_cycles.len());
    for cycle in ham_cycles.into_iter() {
        if !ret.contains(&cycle) {
            ret.push(cycle);
        }
    }
    ret
}

// constructs the graph for each level and then calls all_ham_paths on this graph
// we connect x and y of the same level if and only if they have a bridge above and below
fn ham_cycles_levels(l: &Lattice, cyclic: bool) -> Vec<Vec<Vec<usize>>> { // Vec over evels < Vec over ham cycles < vec over vertices of cycle
    let num_levels = l.levels.len();
    let mut ret = Vec::new();

    for (i, level) in l.levels.iter().enumerate() {
        // build the graph
        let mut graph = Graph::new_undirected();
        let mut node_indices = Vec::new();
        // add vertices
        for x in level {
            let idx = graph.add_node(*x);
            node_indices.push(idx);
        }
        // add edges between x and y if they have a bridge in the layer above and in the layer below
        for a in &node_indices {
            for b in &node_indices {
                let (x, y) = (graph[*a], graph[*b]);
                if (i == num_levels-1 || l.bridges_above.contains_key(&pair(x, y))) && (i == 0 || l.bridges_below.contains_key(&pair(x, y))) {
                    graph.add_edge(*a, *b, ());
                }
            }
        }
        ret.push(all_ham_paths(&graph, cyclic));
    }
    ret
}

// a sorted tuple of length 2
fn pair(a: usize, b: usize) -> (usize, usize) {
    (min(a,b), max(a,b))
}

enum Orientation {
    Above,
    Below,
}

// given a level, it generates the bridges above/below this level.
// If an adjacent pair in level does not have a bridge, we panic
fn gen_bridges_unchecked(level: &Vec<usize>, orientation: Orientation, cyclic: bool, l: &Lattice) -> Vec<usize> {
    let mut bridges: Vec<usize> = Vec::with_capacity(level.len());
    let n = level.len();
    for i in (0..(n-1)) {
        bridges.push( match orientation {
            Orientation::Above => { l.bridges_above[&pair(level[i], level[i+1])] },
            Orientation::Below => { l.bridges_below[&pair(level[i], level[i+1])] },
        })
    }
    if cyclic {
        bridges.push( match orientation {
            Orientation::Above => { l.bridges_above[&pair(level[0], level[level.len()-1])] },
            Orientation::Below => { l.bridges_below[&pair(level[0], level[level.len()-1])] },
        })
    }
    bridges
}

/// Checks if every element in the vector occurs in a single contiguous interval.
pub fn violation_free(s: &Vec<usize>, cyclic: bool) -> bool {
    // Empty vectors or single elements are always valid
    if s.len() <= 1 {
        return true;
    }

    let mut visited = HashSet::new();
    let mut start_index = 0;
    let mut end_index = s.len();

    // Handle cyclic case where start and end wrap around
    if cyclic && s.first() == s.last() {
        let wrap_val = s[0];
        
        // Mark the wrapped value as visited immediately
        visited.insert(wrap_val);

        // Advance start_index past the first group of 'wrap_val'
        while start_index < s.len() && s[start_index] == wrap_val {
            start_index += 1;
        }

        // If start_index reached the end, all elements were equal -> valid
        if start_index == s.len() {
            return true;
        }

        // Move end_index backwards past the last group of 'wrap_val'
        // We look at end_index - 1 because end_index is exclusive
        while end_index > start_index && s[end_index - 1] == wrap_val {
            end_index -= 1;
        }
    }

    // Check the linear segment (or the middle part if cyclic wrapped)
    let slice = &s[start_index..end_index];
    
    // We need to track the previous value to detect group changes
    let mut prev = None;

    for &x in slice {
        // Only trigger logic if the value changes
        if Some(x) != prev {
            if visited.contains(&x) {
                // If we have seen 'x' before and it wasn't the immediate predecessor,
                // it's a violation (split interval).
                return false;
            }
            visited.insert(x);
            prev = Some(x);
        }
    }
    true
}

// is s1 a subsequence of s2?
fn is_subsequence(s1: &Vec<usize>, s2: &Vec<usize>, cyclic: bool) -> bool {
    // Handle Empty Case
    if s1.is_empty() {
        return true;
    }
    let mut s1_clone = s1.clone();

    // Cyclic Deduplication (Merge first and last if needed)
    if cyclic && s1_clone.len() > 1 {
        if s1_clone.first() == s1_clone.last() {
            s1_clone.pop();
        }
    }

    // If cyclic: search in s2 followed by s2 (simulates wrapping).
    // If not cyclic: search in s2 only.
    let s2_iter = s2.iter();
    
    // We use .chain() to repeat s2 without allocating a new vector
    let mut search_space = if cyclic {
        s2_iter.clone().chain(s2_iter).peekable()
    } else {
        s2_iter.clone().chain([].iter()).peekable()
    };

    // We try to find every element of s1 in order within the search_space
    for target in s1_clone {
        loop {
            match search_space.next() {
                Some(&candidate) => {
                    if candidate == target {
                        // Found the current target, move to the next number in s1
                        break;
                    }
                },
                None => {
                    // We ran out of elements in s2 (or s2+s2) without finding the target
                    return false;
                }
            }
        }
    }
    true
}

fn bridges_if_valid(level: &Vec<usize>, cyclic: bool, is_lowest: bool, is_highest: bool, l: &Lattice) -> Option<(Vec<usize>, Vec<usize>)> {
    let mut bridges_above = if is_highest {
        Vec::new()
    } else {
        gen_bridges_unchecked(level, Orientation::Above, cyclic, l)
    };
    if !violation_free(&bridges_above, cyclic) { return None; }; // if not valid, stop here
    bridges_above.dedup();                                  // dedup already here, no need to store the duplicates
    let mut bridges_below = if is_lowest {
        Vec::new()
    } else {
        gen_bridges_unchecked(level, Orientation::Below, cyclic, l)
    };
    if !violation_free(&bridges_below, cyclic) { return None }; // if not valid, stop here
    bridges_below.dedup();
    Some((bridges_above, bridges_below))
}

pub struct Level {
    pub faces: Vec<usize>,
    bridges_above: Vec<usize>,
    bridges_below: Vec<usize>,
    pub parents: Vec<usize>,        // pointers into the levels one dimension up
}

pub fn rhombic_strips_simple(l: &Lattice, cyclic: bool) -> Vec<Vec<Level>> {
    // 1. Compute all Hamilton cycles for each level
    let ham_cycles = ham_cycles_levels(l, cyclic);
    println!("Hamilton cycles found");

    // Temporary storage for candidates (Dimension -> List of (Faces, BridgesUp, BridgesDown))
    let mut levels_raw = Vec::with_capacity(ham_cycles.len());

    // 2. Populate raw data: Validate cycles and compute bridges
    for (i, cycles) in ham_cycles.iter().enumerate() {
        let mut current_level_candidates = Vec::with_capacity(cycles.len());
        for ham_cycle in cycles {
            // Compute bridges and check for "violation_free"
            if let Some((bridges_above, bridges_below)) = bridges_if_valid(
                ham_cycle,
                cyclic,
                i == 0,
                i == ham_cycles.len() - 1,
                l
            ) {
                current_level_candidates.push((ham_cycle.clone(), bridges_above, bridges_below));
            }
        }
        levels_raw.push(current_level_candidates);
    }

    // Pre-initialize result vector (so we can access by index safely)
    let mut levels_with_parents: Vec<Vec<Level>> = (0..levels_raw.len()).map(|_| Vec::new()).collect();

    // 3. Top-down linking and filtering
    // We iterate from top (highest dimension) to bottom.
    for i in (0..levels_raw.len()).rev() {
        let candidates = &levels_raw[i];
        let mut kept_levels = Vec::new();

        if i == levels_raw.len() - 1 {
            // Topmost level: Accept all valid cycles (no parent check needed)
            for (faces, bridges_above, bridges_below) in candidates {
                kept_levels.push(Level {
                    faces: faces.clone(),
                    bridges_above: bridges_above.clone(),
                    bridges_below: bridges_below.clone(),
                    parents: vec![], // No parents at the very top
                });
            }
        } else {
            // Other levels: Check against the level ABOVE (already filtered) at i+1
            let potential_parents = &levels_with_parents[i + 1];

            for (faces, bridges_above, bridges_below) in candidates {
                let mut valid_parent_indices = Vec::new();

                // Find all compatible parents in level i+1
                for (parent_idx, parent) in potential_parents.iter().enumerate() {
                    // Condition:
                    // 1. My bridges UP must fit into the parent's faces.
                    // 2. The parent's bridges DOWN must fit into my faces.
                    if is_subsequence(bridges_above, &parent.faces, cyclic) &&
                        is_subsequence(&parent.bridges_below, faces, cyclic) {
                        valid_parent_indices.push(parent_idx);
                    }
                }

                // Only keep this element if we found at least one parent.
                // This fulfills the requirement "recursively delete levels without parents".
                if !valid_parent_indices.is_empty() {
                    kept_levels.push(Level {
                        faces: faces.clone(),
                        bridges_above: bridges_above.clone(),
                        bridges_below: bridges_below.clone(),
                        parents: valid_parent_indices,
                    });
                }
            }
        }

        // Store the filtered list at the correct dimension index
        levels_with_parents[i] = kept_levels;
    }

    levels_with_parents
}
