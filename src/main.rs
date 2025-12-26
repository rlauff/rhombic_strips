
mod lattice;
use crate::lattice::*;

mod rhombic;
use crate::rhombic::*;

use colored::Colorize;

use std::fs::read_to_string;

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

fn main() {
    let source = "trees_6";
    let centered = true;
    let mut graphs: Vec<Graph> = Vec::new();

    for edgelist in read_to_string(source).expect("Read failed.").lines() {
        let edges_as_strings: Vec<&str> = edgelist[2..edgelist.len()-2].split("), (").collect();
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
        graphs.push(
            Graph {
                vertices: vertices,
                edges: edges,
                tubes: None
            })
    }

    let mut graphs_with = vec![];
    let mut graphs_without = vec![];
    let num_graphs = graphs.len();

    for (i, mut g) in graphs.into_iter().enumerate() {
        if g.edges.iter().filter(|x| x[0]==0).collect::<Vec<_>>().len() == 1 { continue };
        let l = lattice_from_graph(&mut g, centered);
        let hcs = l.ham_cycles.clone();
        let num_hcs = hcs.len();
        if num_hcs == 0 { continue };
        let mut found_one = false;

        println!("{}", format!("Graph ({i}/{num_graphs}): {:?}", g.edges).blue().bold());
        for (_i, hc) in hcs.into_iter().enumerate() {
            //print!("  Number of rhombic strips based on Hamilton cycle {:?} ({i} / {num_hcs}):    ", hc);
            let found = rhombic_strips_dfs_simple(vec![hc], &l, l.dim.clone()-1);
            let num_found = found.len();
            //println!("{}", num_found);
            if num_found > 0 { found_one = true; break };
            // if g.edges.len() >= g.vertices.len()*(g.vertices.len()-1)/2-1 {
            //     break
            // };
        }
        if found_one {
            println!("{}", format!("Found one!").green());
            graphs_with.push(g.edges.clone());
        } else {
            println!("{}", format!("No luck!").red());
            graphs_without.push(g.edges.clone());
        }
        println!();
    }
    println!("{}", "Graphs without rhombic strip:");
    for edges in graphs_without.iter() {
        println!("{:?}", edges);
    }

    println!("\n\nRatio with/without: {}", graphs_with.len() as f64/graphs_without.len() as f64);
}

