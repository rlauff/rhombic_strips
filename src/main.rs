mod lattice;
use crate::lattice::*;

mod rhombic;
use crate::rhombic::*;

use std::time::Instant;
use std::env;
use std::fs::read_to_string;

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

// Logic extracted from original main to reuse for both single lattice files and graph lists
fn process_and_count(l: &Lattice) {
    // 2. Compute the Rhombic Strips structure
    let start = Instant::now();
    // parameters: lattice, cyclic=true, find_all=false
    let levels = rhombic_strips_simple(&l, true);
    let duration = start.elapsed();

    let s = format!("Computed strip structure in {:?}", duration);
    println!("{}", s);

    if levels.is_empty() {
        println!("{}", "No rhombic strips found.".red());
        return;
    }

    // 3. Count solutions using DFS with Memoization
    let mut memo: Vec<Vec<Option<u128>>> = levels.iter()
    .map(|layer| vec![None; layer.len()])
    .collect();

    let mut total_solutions: u128 = 0;

    // We start looking for paths at the bottom layer (Level 0)
    for (i, node) in levels[0].iter().enumerate() {
        // Calculate how many valid strips start with this specific cycle
        let count = count_paths(0, i, &levels, &mut memo);

        if count > 0 {
            // Optional: print detailed cycle info if needed
            // println!("  Cycle {:?}: found {} strip(s)", node.faces, count);
            total_solutions += count;
        }
    }

    if total_solutions == 0 {
        println!("{}", format!("Total Rhombic Strips found: {}", total_solutions).red());
    } else {
        println!("{}", format!("Total Rhombic Strips found: {}", total_solutions).green());
    }
}


fn main() {
    // Collect args
    let args: Vec<String> = env::args().collect();
    let mut centered = false;
    let mut input_files = Vec::new();

    // Simple argument parsing
    for arg in args.iter().skip(1) {
        if arg == "--centered" {
            centered = true;
        } else {
            input_files.push(arg.clone());
        }
    }

    // Default if no files provided
    if input_files.is_empty() {
        input_files = vec!["cube3d".to_string(), "cube4d".to_string()];
    }

    for filename in input_files {
        println!("========================================");
        println!("Processing file: {}", filename);

        // Determine if file is a Graph list or a Lattice definition
        // Heuristic: check if the first non-empty line starts with "[("
        let content = match read_to_string(&filename) {
            Ok(c) => c,
            Err(e) => {
                println!("Error reading file {}: {}", filename, e);
                continue;
            }
        };

        let first_line = content.lines().find(|l| !l.trim().is_empty());
        let is_graph_file = match first_line {
            Some(line) => line.trim().starts_with("[("),
            None => false,
        };

        if is_graph_file {
            println!("Detected Graph file format.");
            let graphs = read_graphs(&filename);
            let total_graphs = graphs.len();

            for (idx, mut g) in graphs.into_iter().enumerate() {
                println!("----------------------------------------");
                println!("{}", format!("Graph ( {} / {} ): Edgelist: {:?}", idx + 1, total_graphs, g.edges).bold().blue());

                // Construct lattice from graph, passing the centered flag
                let l = lattice_from_graph(&mut g, centered);
                println!("Lattice constructed. Dimension: {}", l.dim);

                process_and_count(&l);
            }

        } else {
            // Assume Standard Lattice File
            // 1. Load the lattice using the new function
            let l = lattice_from_file(&filename);
            println!("Lattice loaded from {}. Dimension: {}", filename, l.dim);

            process_and_count(&l);
        }

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

fn read_graphs(source: &str) -> Vec<Graph> {
    let mut graphs: Vec<Graph> = Vec::new();

    for edgelist in read_to_string(source).expect("Read failed.").lines() {
        if edgelist.trim().is_empty() { continue; }

        // Expected format: [(0, 1), (1, 2), ...]
        // We strip the outer [( and )] manually or via split
        let trimmed = edgelist.trim();
        if !trimmed.starts_with("[(") { continue; } // Skip malformed lines

        // Remove starting "[(" and ending ")]" safely
        // But the code below uses a split trick that works if the string is exactly standard
        // edges_as_strings: split by "), (" handles the middle separators

        // We need to handle the start and end of the string to avoid parsing errors
        let inner = &trimmed[2..trimmed.len()-2];

        let edges_as_strings: Vec<&str> = inner.split("), (").collect();
        let mut edges: Vec<[usize; 2]> = Vec::new();

        for edge_str in edges_as_strings.iter() {
            let edge_vec: Vec<usize> = edge_str.split(", ").map(|r| r.parse::<usize>().unwrap()).collect();
            edges.push([edge_vec[0], edge_vec[1]]);
        }

        let mut vertices = Vec::new();
        for [a, b] in edges.iter() {
            if !vertices.contains(a) {
                vertices.push(*a);
            }
            if !vertices.contains(b) {
                vertices.push(*b);
            }
        }
        // sorting vertices makes things deterministic
        vertices.sort();

        graphs.push(
            Graph {
                vertices: vertices,
                edges: edges,
                tubes: None
            })
    }
    graphs
}
