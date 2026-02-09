//the actual code to construct the rhombic strip

use crate::lattice::*;
use rayon::prelude::*;
use itertools::Itertools;
use petgraph::graph::Graph;
use petgraph::Undirected;
use petgraph::graph::NodeIndex;
use std::cmp::{ min, max };
use std::collections::{HashSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn shift_min_to_front(cycle: &mut Vec<usize>) {
    if let Some((min_index, _)) = cycle.iter().enumerate().min_by_key(|&(_, val)| val) {
        cycle.rotate_left(min_index);
    }
}

// given a graph, uses a backtracking DFS to find all Hamilton paths in this graph.
// It enforces the "Color Block" constraint: edges of the same color must be adjacent in the path.
// Parallelized using Rayon with granular step-based progress tracking.
pub fn all_ham_paths(graph: &Graph<usize, (usize, usize), Undirected>, restrict_to_cycles: bool) -> Vec<Vec<usize>> {
    let node_count = graph.node_count();

    // Trivial cases
    if node_count == 0 {
        return vec![];
    }

    let indices: Vec<NodeIndex> = graph.node_indices().collect();

    // Progress Tracking:
    // nodes_processed: How many start nodes have been fully explored.
    // total_steps: How many recursive DFS steps have been taken globally.
    let nodes_processed = AtomicUsize::new(0);
    let total_steps = AtomicUsize::new(0);

    // We search from ALL nodes to ensure we don't miss cycles due to color-block boundaries.
    let raw_results: Vec<Vec<usize>> = (0..node_count).into_par_iter().flat_map(|start_idx| {
        let mut local_results = Vec::new();
        let mut path = Vec::with_capacity(node_count);
        let mut visited = vec![false; node_count];

        // Local step counter to batch atomic updates (avoids contention)
        let mut local_step_count = 0;

        let start_node = indices[start_idx];

        visited[start_idx] = true;
        path.push(start_idx);

        let mut forbidden_up = HashSet::new();
        let mut forbidden_down = HashSet::new();

        dfs_ham(
            graph,
            &indices,
            &mut path,
            &mut visited,
            &mut local_results,
            restrict_to_cycles,
            // Above Colors
            None, None, &mut forbidden_up,
            // Below Colors
            None, None, &mut forbidden_down,
            // Progress tracking
            &total_steps,
            &mut local_step_count
        );

        // Flush any remaining local steps to the global counter
        if local_step_count > 0 {
            total_steps.fetch_add(local_step_count, Ordering::Relaxed);
        }

        // Update completed nodes
        let completed = nodes_processed.fetch_add(1, Ordering::Relaxed) + 1;

        // Print final status for this node
        // (Optional: You can comment this out if the step-based printing is sufficient)
        // println!("Finished start node {}/{}", completed, node_count);

        local_results
    }).collect();

    // Deduplicate results
    if restrict_to_cycles {
        let mut unique_cycles = HashSet::new();
        let mut ret = Vec::new();

        for mut cycle in raw_results {
            let mut values: Vec<usize> = cycle.iter().map(|&i| graph[indices[i]]).collect();
            shift_min_to_front(&mut values);
            if unique_cycles.insert(values.clone()) {
                ret.push(values);
            }
        }
        ret
    } else {
        raw_results.into_iter()
            .map(|cycle| cycle.iter().map(|&i| graph[indices[i]]).collect())
            .collect()
    }
}

// Helper DFS function
#[allow(clippy::too_many_arguments)]
fn dfs_ham(
    graph: &Graph<usize, (usize, usize), Undirected>,
    indices: &[NodeIndex],
    path: &mut Vec<usize>,
    visited: &mut Vec<bool>,
    results: &mut Vec<Vec<usize>>,
    restrict_to_cycles: bool,
    // Color State Above
    cur_up: Option<usize>,
    start_up: Option<usize>,
    forbidden_up: &mut HashSet<usize>,
    // Color State Below
    cur_down: Option<usize>,
    start_down: Option<usize>,
    forbidden_down: &mut HashSet<usize>,
    // Progress Counters
    global_steps: &AtomicUsize,
    local_steps: &mut usize,
) {
    // 1. Progress Reporting Logic
    *local_steps += 1;
    // Sync with global counter every 20,000 steps to reduce atomic overhead
    if *local_steps >= 20_000 {
        let old_val = global_steps.fetch_add(*local_steps, Ordering::Relaxed);
        let new_val = old_val + *local_steps;
        *local_steps = 0;

        // Print a "Heartbeat" every 5 million steps globally
        // The check (old / N != new / N) ensures we print exactly once when crossing the threshold
        const PRINT_INTERVAL: usize = 5_000_000;
        // if old_val / PRINT_INTERVAL != new_val / PRINT_INTERVAL {
        //     let s = format!("Progress Update: ~{} million steps searched...",
        //                     new_val / 1_000_000);
        //     print!("{}{}", s, "\r".repeat(s.len()));
        // }
    }

    let n = graph.node_count();
    let current_u_idx = *path.last().unwrap();
    let current_node = indices[current_u_idx];

    // Base case: Path is full length
    if path.len() == n {
        if restrict_to_cycles {
            let start_u_idx = path[0];
            let start_node = indices[start_u_idx];

            if let Some(edge) = graph.find_edge(current_node, start_node) {
                let (c_up, c_down) = graph[edge];
                let valid_up = check_cycle_close(c_up, cur_up, start_up, forbidden_up);
                let valid_down = check_cycle_close(c_down, cur_down, start_down, forbidden_down);

                if valid_up && valid_down {
                    results.push(path.clone());
                }
            }
        } else {
            results.push(path.clone());
        }
        return;
    }

    // Recursive step: Try all unvisited neighbors
    let neighbors: Vec<_> = graph.neighbors(current_node).collect();

    for neighbor in neighbors {
        let neighbor_idx = match indices.iter().position(|&i| i == neighbor) {
            Some(i) => i,
            None => continue,
        };

        if !visited[neighbor_idx] {
            let edge = graph.find_edge(current_node, neighbor).unwrap();
            let (c_up, c_down) = graph[edge];

            // 1. Try step Above
            let (valid_up, switched_up) = check_step(c_up, cur_up, forbidden_up);
            if !valid_up { continue; }

            // 2. Try step Below
            let (valid_down, switched_down) = check_step(c_down, cur_down, forbidden_down);
            if !valid_down { continue; }

            // 3. Apply changes and Recurse
            visited[neighbor_idx] = true;
            path.push(neighbor_idx);

            let next_start_up = start_up.or(Some(c_up));
            let next_start_down = start_down.or(Some(c_down));

            let mut added_up = false;
            if switched_up {
                if let Some(old_c) = cur_up { added_up = forbidden_up.insert(old_c); }
            }

            let mut added_down = false;
            if switched_down {
                if let Some(old_c) = cur_down { added_down = forbidden_down.insert(old_c); }
            }

            dfs_ham(
                graph, indices, path, visited, results, restrict_to_cycles,
                Some(c_up), next_start_up, forbidden_up,
                Some(c_down), next_start_down, forbidden_down,
                global_steps, local_steps
            );

            // 4. Backtrack
            if added_up {
                if let Some(old_c) = cur_up { forbidden_up.remove(&old_c); }
            }
            if added_down {
                if let Some(old_c) = cur_down { forbidden_down.remove(&old_c); }
            }

            path.pop();
            visited[neighbor_idx] = false;
        }
    }
}

// Logic to check if taking an edge with 'color' is valid given current state
fn check_step(color: usize, current: Option<usize>, forbidden: &HashSet<usize>) -> (bool, bool) {
    match current {
        None => (true, false),
        Some(cur) => {
            if color == cur {
                (true, false)
            } else {
                if forbidden.contains(&color) {
                    (false, true)
                } else {
                    (true, true)
                }
            }
        }
    }
}

// Logic to check the final closing edge of a cycle
fn check_cycle_close(
    color: usize,
    current: Option<usize>,
    start: Option<usize>,
    forbidden: &HashSet<usize>
) -> bool {
    if current == Some(color) { return true; }
    if start == Some(color) { return true; }
    !forbidden.contains(&color)
}

// constructs the graph for each level and then calls all_ham_paths on this graph
// we connect x and y of the same level if and only if they have a bridge above and below
fn ham_cycles_levels(l: &Lattice, cyclic: bool) -> Vec<Vec<Vec<usize>>> {
    let num_levels = l.levels.len();
    let mut ret = Vec::new();

    for (i, level) in l.levels.iter().enumerate() {
        // println!("Processing level {}/{}", i + 1, num_levels);

        let mut graph = Graph::new_undirected();
        let mut node_indices = Vec::new();
        for x in level {
            let idx = graph.add_node(*x);
            node_indices.push(idx);
        }

        for a in &node_indices {
            for b in &node_indices {
                if a >= b { continue; }

                let (x, y) = (graph[*a], graph[*b]);
                if (i == num_levels-1 || l.bridges_above.contains_key(&pair(x, y))) && (i == 0 || l.bridges_below.contains_key(&pair(x, y))) {

                    let c_above = if i == num_levels - 1 { 0 } else { *l.bridges_above.get(&pair(x, y)).unwrap_or(&0) };
                    let c_below = if i == 0 { 0 } else { *l.bridges_below.get(&pair(x, y)).unwrap_or(&0) };

                    graph.add_edge(*a, *b, (c_above, c_below));
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
    if !violation_free(&bridges_above, cyclic) { panic!("Bridges not valid, should not happen with new Hamiltonicity check"); }; // if not valid, stop here
    bridges_above.dedup();                                  // dedup already here, no need to store the duplicates
    let mut bridges_below = if is_lowest {
        Vec::new()
    } else {
        gen_bridges_unchecked(level, Orientation::Below, cyclic, l)
    };
    if !violation_free(&bridges_below, cyclic) { panic!("Bridges not valid, should not happen with new Hamiltonicity check"); }; // if not valid, stop here
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
    // println!("Hamilton cycles found");

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
