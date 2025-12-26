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

// given a graph, uses CaDiCaL to find all Hamilton cycles in this graph
pub fn all_ham_cycles(graph: &Graph<usize, (), Undirected>) -> Vec<Vec<usize>> {
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

    // 3. Each vertex v must appear at least once in the cycle
    for v in 0..node_count {
        let mut clause = Clause::new();
        for i in 0..node_count {
            clause.add(var_for(v, i));
        }
        solver.add_clause(clause);
    }

    // 4. Each vertex v must appear at most once in the cycle
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

    // 5. Adjacency constraints (The cycle must follow edges)
    // We collect indices first to ensure stable mapping between v_idx (0..N) and graph nodes
    let node_indices: Vec<_> = graph.node_indices().collect();

    for (v_idx, &node_idx) in node_indices.iter().enumerate() {
        let neighbors: Vec<_> = graph.neighbors(node_idx).collect();
        
        for i in 0..node_count {
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

// is s1 a subsequence of s2?
fn is_subsequence(s1: &Vec<usize>, s2: &Vec<usize>) -> bool {
    let mut p = 0; // pointer into s2

    for &x in s1 {
        while p < s2.len() && s2[p] != x {
            p += 1;
        }
        if p == s2.len() {
            return false;
        }
        p += 1;
    }

    true
}

pub fn rhombic_strips_simple(l: &Lattice, find_all: bool) -> Vec<Vec<Vec<usize>>> {
    unimplemented!()
}
