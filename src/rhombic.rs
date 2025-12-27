//the actual code to construct the rhombic strip

use crate::lattice::*;
use std::cmp;
use rayon::prelude::*;
use itertools::Itertools;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::Undirected;
use rustsat::{
    solvers::{Solve, SolveIncremental, SolverResult, SolveStats}, // SolveIncremental hinzugefügt
    types::{Clause, Lit, TernaryVal}, // TernaryVal statt LitValue
};
use rustsat_cadical::CaDiCaL;

// given a graph, uses CaDiCaL to find all Hamilton paths in this graph. 
pub fn all_ham_paths(graph: &Graph<usize, (), Undirected>, restrict_to_cycles: bool) -> Vec<Vec<usize>> {
    let node_count = graph.node_count();
    
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
                ]));
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
        let solve_start = Instant::now();
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
                            ham_cycle.push(v);
                        }
                    }
                }
                solver.add_clause(blocking_clause);
                ham_cycles-push(ham_cycle);
            },
            SolverResult::Unsat => {
                break;
            },
            SolverResult::Interrupted => {
                panic!("Solver got interruped");
            }
        }
    }
    ham_cycles
}

// constructs the graph for each level and then calls all_ham_cycles on this graph
// we connect x and y of the same level if and only if they have a bridge above and below
fn ham_cycles_levels(l: &Lattice) -> Vec<Vec<Vec<usize>>> { // Vec over evels < Vec over ham cycles < vec over vertices of cycle
    let num_levels = l.levels.len();
    let mut ret = Vec::new();

    for (i, level) in l.levels.into_iter().enumerate() {
        // build the graph
        let graph = Graph::new_undirected();
        // add vertices
        for x in level {
            graph.add_node(x);
        }
        // add edges between x and y if they have a bridge in the layer above and in the layer below
        for x in level {
            for y in level {
                if (i == num_levels || l.bridges_above.contains_key((x, y))) && (i == 0 || l.bridges_below.contains_key((x, y))) {
                    graph.add_edge(x, y, ());
                }
            }
        }
        ret.push(all_ham_cycles(&graph));
    }
    ret
}

// given a level, it generates the bridges above/below this level.
// If an adjacent pair in level does not have a bridge, or if the bridges do not form intervals, we return None
fn gen_bridges(level: &Vec<usize>, above: bool, l: &Lattice) -> Option<Vec<usize>> {
  unimplemented!()
}

// is s1 a subsequence of s2? 
fn is_subsequence(s1: Vec<usize>, s2: Vec<usize>, cyclic: bool) -> bool {
    // Handle Empty Case
    if s1.is_empty() {
        return true;
    }

    // Deduplicate s1
    let mut s1_dedup = Vec::new();
    for &val in &s1 {
        if s1_dedup.last() != Some(&val) {
            s1_dedup.push(val);
        }
    }

    // Cyclic Deduplication (Merge first and last if needed)
    if cyclic && s1_dedup.len() > 1 {
        if s1_dedup.first() == s1_dedup.last() {
            s1_dedup.pop();
        }
    }

    // If cyclic: search in s2 followed by s2 (simulates wrapping).
    // If not cyclic: search in s2 only.
    let s2_iter = s2.iter();
    
    // We use .chain() to repeat s2 without allocating a new vector
    let mut search_space = if cyclic {
        s2_iter.clone().chain(s2_iter).peekable()
    } else {
        s2_iter.clone().chain(std::slice::Iter::empty()).peekable()
    };

    // We try to find every element of s1_dedup in order within the search_space
    for &target in &s1_dedup {
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

fn main() {
    // Test: The case where looking for s1[0] first would fail
    // s1 = [1, 2, 3]. Rotation [3, 1, 2] is the one that fits.
    let s1 = vec![1, 2, 3];
    let s2 = vec![3, 1, 5, 2];

    println!("Optimized Check: {}", is_subsequence(s1, s2, true)); // Prints: true
}

pub fn rhombic_strips_simple(l: &Lattice, find_all: bool) -> Vec<Vec<Vec<usize>>> {
    unimplemented!()
}
