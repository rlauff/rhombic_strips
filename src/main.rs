
mod lattice;
use crate::lattice::*;

mod rhombic;
use crate::rhombic::*;

use colored::Colorize;

use std::fs::read_to_string;

use std::collections::HashSet;


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

    let _l = lattice_from_file("cube3d");

    let dont_care_about_total = true;


    let source = "all_graphs";
    //let source = "test_graphs";
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
        let l = lattice_from_graph(&mut g);
        let mut total = 0;
        let hcs = l.ham_cycles.clone();
        let num_hcs = hcs.len();
        if num_hcs == 0 { continue };
        let mut sols = HashSet::new();

        println!("{}", format!("Graph ({i}/{num_graphs}): {:?}", g.edges).blue().bold());
        for (_i, hc) in hcs.into_iter().enumerate() {
            //print!("  Number of rhombic strips based on Hamilton cycle {:?} ({i} / {num_hcs}):    ", hc);
            let found = rhombic_strips_dfs_simple(vec![hc], &l, l.dim.clone());
            let num_found = found.len();
            for sol in found.iter() {
                sols.insert(reduced(layers_to_sequence(sol, &l)));
            }
            //println!("{}", num_found);
            total += num_found;
            if dont_care_about_total && total > 0 { break };
            // if g.edges.len() >= g.vertices.len()*(g.vertices.len()-1)/2-1 {
            //     break
            // };
        }
        if total > 0 {
            println!("In total {} were found. Up to isomorphism: {}", format!("{}", total).green(), format!("{}", sols.len()).green());
            graphs_with.push(g.edges.clone());
        } else {
            println!("In total {} were found.", format!("{}", total).red());
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





fn _main() {

    println!("3D cube:");

    let l = lattice_from_file("cube3d");
    let mut total = 0;

    let mut sols = HashSet::new();
    //print!("  Number of rhombic strips based on Hamilton cycle {:?} ({i} / {num_hcs}):    ", hc);
    let found = rhombic_strips_dfs_simple(vec![vec![0,4,5,7,6,2,3,1]], &l, l.dim.clone());
    let num_found = found.len();
    for sol in found.iter() {
        sols.insert(reduced(layers_to_sequence(sol, &l)));
    }
    //println!("{}", num_found);
    total += num_found;

    println!("In total {} were found. Up to isomorphism: {}\n", format!("{}", total).green(), format!("{}", sols.len()).green());


    println!("4D cube:");

    let l = lattice_from_file("cube4d");
    total = 0;

    let mut sols = HashSet::new();
    //print!("  Number of rhombic strips based on Hamilton cycle {:?} ({i} / {num_hcs}):    ", hc);
    let found = rhombic_strips_dfs_simple(vec![vec![0,1,3,2,6,7,5,4,12,13,15,14,10,11,9,8]], &l, l.dim.clone());
    let num_found = found.len();
    for sol in found.iter() {
        sols.insert(reduced(layers_to_sequence(sol, &l)));
    }
    //println!("{}", num_found);
    total += num_found;

    println!("In total {} were found. Up to isomorphism: {}", format!("{}", total).green(), format!("{}", sols.len()).green());
}
