
mod lattice;
use crate::lattice::*;

mod rhombic;
use crate::rhombic::*;

use std::time::Instant;

use colored::Colorize;


fn layers_to_sequence(layers: &Vec<Vec<usize>>, l: &Lattice) -> Vec<usize> {
    let n = layers.len();
    let mut pointers = vec![0; n];
    //let goals = layers.into_iter().map(|x| x.len()).collect::<Vec<usize>>();
    let mut seq = vec![];
    let mut change_made = true;
    while change_made {
        change_made = false;
        if pointers[0] != layers[0].len() &&
            l.faces[layers[0][(pointers[0]+1)%layers[0].len()]].upset.contains(&layers[1][pointers[1]%layers[1].len()])
            {
                seq.push(0);
                pointers[0] += 1;
                change_made = true;
            }
            for i in 1..(n-1) {
                if pointers[i] != layers[i].len() && !change_made &&
                    l.faces[layers[i][(pointers[i]+1)%layers[i].len()]].upset.contains(&layers[i+1][pointers[i+1]%layers[i+1].len()]) &&
                    l.faces[layers[i][(pointers[i]+1)%layers[i].len()]].downset.contains(&layers[i-1][pointers[i-1]%layers[i-1].len()])
                    {
                        seq.push(i);
                        pointers[i] += 1;
                        change_made = true;
                        break
                    }
            }
            if pointers[n-1] != layers[n-1].len() && !change_made &&
                l.faces[layers[n-1][(pointers[n-1]+1)%layers[n-1].len()]].downset.contains(&layers[n-2][pointers[n-2]%layers[n-2].len()])
                {
                    seq.push(n-1);
                    pointers[n-1] += 1;
                    change_made = true;
                }
    }
    seq
}

fn reduced(mut s: Vec<usize>) -> Vec<usize> {
    let n = s.len();
    if s[0]+1 < s[n-1] {
        s.swap(0, n-1);
        return reduced(s);
    };
    for i in 0..n-1 {
        if s[i] > s[i+1]+1 {
            s.swap(i, i+1);
            return reduced(s);
        }
    }
    let k = s.iter().max().unwrap() + 1;
    let mut champion = 0;
    let mut record = 0;
    for i in 0..s.len() {
        let mut val = 0;
        for j in 0..s.len() {
            val += (s[(i+j)%n] * k).pow(j.try_into().unwrap());
        }
        if val > record {
            record = val;
            champion = i;
        }
    }
    let mut ret = vec![];
    for i in 0..n {
        ret.push(s[(i+champion)%n]);
    }
    ret
}



fn main() {
    // Files to process
    let input_files = vec!["cube3d", "cube4d"];

    for filename in input_files {
        println!("========================================");

        // 1. Load the lattice using the new function
        let l = lattice_from_file(filename);
        println!("Lattice loaded from {}. Dimension: {}", filename, l.dim);

        // 2. Compute the Rhombic Strips structure
        let start = Instant::now();
        // parameters: lattice, cyclic=true, find_all=false
        let levels = rhombic_strips_simple(&l, true);
        let duration = start.elapsed();

        println!("Computed strip structure in {:?}", duration);

        if levels.is_empty() {
            println!("No rhombic strips found.");
            continue;
        }

        // 3. Count solutions using DFS with Memoization
        // The structure is a DAG (Directed Acyclic Graph).
        // We use memoization to avoid re-calculating the number of paths for the same node multiple times.
        // memo[layer_index][node_index] -> Option<Number of paths to top>
        let mut memo: Vec<Vec<Option<u128>>> = levels.iter()
            .map(|layer| vec![None; layer.len()])
            .collect();

        let mut total_solutions: u128 = 0;

        println!("\nBreakdown by 0-dimensional layer (Hamilton Cycles):");

        // We start looking for paths at the bottom layer (Level 0)
        for (i, node) in levels[0].iter().enumerate() {
            // Calculate how many valid strips start with this specific cycle
            let count = count_paths(0, i, &levels, &mut memo);

            if count > 0 {
                println!("  Cycle {:?}: found {} strip(s)", node.faces, count);
                total_solutions += count;
            }
        }

        println!("\nTotal Rhombic Strips found: {}", total_solutions);
        println!("========================================\n");
    }
}

/// Recursively counts the number of paths from the current node to the top layer.
/// Uses the 'memo' table to cache results.
fn count_paths(
    layer_idx: usize,
    node_idx: usize,
    levels: &Vec<Vec<Level>>,
    memo: &mut Vec<Vec<Option<u128>>>
) -> u128 {
    // Base Case: If we reached the top layer, this counts as 1 valid strip.
    if layer_idx == levels.len() - 1 {
        return 1;
    }

    // Check Cache: Have we already computed the count for this specific node?
    if let Some(saved_count) = memo[layer_idx][node_idx] {
        return saved_count;
    }

    // Recursive Step: Sum the path counts of all valid parents.
    // In our structure, 'parents' are the compatible nodes in the layer above (layer_idx + 1).
    let mut count: u128 = 0;

    for &parent_idx in &levels[layer_idx][node_idx].parents {
        count += count_paths(layer_idx + 1, parent_idx, levels, memo);
    }


    // Save result to cache before returning
    memo[layer_idx][node_idx] = Some(count);

    count
}

// fn read_graphs(source: &str, centered: bool) -> Vec<Graph> {
//     let mut graphs: Vec<Graph> = Vec::new();
//
//     for edgelist in read_to_string(source).expect("Read failed.").lines() {
//         let edges_as_strings: Vec<&str> = edgelist[2..edgelist.len()-2].split("), (").collect();
//         let mut edges: Vec<[usize; 2]> = Vec::new();
//         for edge_str in edges_as_strings.iter() {
//             let edge_vec: Vec<usize> = edge_str.split(", ").map(|r| r.parse::<usize>().unwrap()).collect();
//             edges.push([edge_vec[0], edge_vec[1]]);
//         }
//         let mut vertices = Vec::new();
//         for [a, b] in edges.iter() {
//             if !vertices.contains(a) {
//                 vertices.push(*a);
//             }
//             if !vertices.contains(b) {
//                 vertices.push(*b);
//             }
//         }
//         graphs.push(
//             Graph {
//                 vertices: vertices,
//                 edges: edges,
//                 tubes: None
//             })
//     }
//     graphs
// }
