//the actual code to construct the rhombic strip

use crate::lattice::*;
use std::cmp;
use rayon::prelude::*;
use itertools::Itertools;

/*
                        def orbit(c):
                        return [c[i:]+c[:i] for i in range(len(c))]

                        def reduced(c):
                        if c[0] < c[-1] - 1:
                            c[0], c[-1] = c[-1], c[0]
                            return reduced(c)
                            for i in range(len(c)-1):
                                if c[i] > c[i+1] + 1:
                                    c[i], c[i+1] = c[i+1], c[i]
                                    return reduced(c)
                                    n = max(c) + 1
                                    return tuple(max(orbit(c), key=lambda x: sum([k*n**i for i,k in enumerate(x)])))*/

pub fn layers_to_sequence(layers: &Vec<Vec<usize>>, l: &Lattice) -> Vec<usize> {
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
    // if pointers != goals {
    //     for layer in layers.into_iter() {
    //         println!("{:?}", layer.into_iter().map(|i| l.faces[*i].label.clone()).collect::<Vec<_>>());
    //     };
    //     println!("{:?}", pointers);
    //     println!("{:?}", seq);
    //     for i in 0..n {
    //         println!("{:?}", l.faces[layers[i][pointers[i]%layers[i].len()]]);
    //     }
    //     panic!();
    // }
    seq
}

fn min_max(a: usize, b: usize) -> (usize, usize) {
    return (cmp::min(a,b), cmp::max(a,b))
}

fn is_good_for_path(indices: &Vec<usize>) -> bool {
    if indices.len() == 0 { return false };
    return indices.len() == indices.last().unwrap() - indices[0] + 1
}

fn is_good_for_cycle(indices: &Vec<usize>, n: usize) -> bool {
    if !indices.contains(&0)|| !indices.contains(&(n-1)) { return is_good_for_path(indices) }
    let mut found_break = false;
    for i in 0..indices.len()-1 {
        if indices[i+1] != indices[i] + 1 {
            if found_break { return false };
                found_break = true;
        }
    }
    true
}

fn layer_ok(layer: &Vec<usize>) -> bool { //returns wether the list of bridges is ok, i.e. each face appears in an interval along it
    for face in layer {
        if !is_good_for_cycle(&(0..layer.len()).filter(|x| layer[*x] == *face).collect(), layer.len()) {
            return false
        }
    }
    true
}

fn replace_by_permutations(v: Vec<usize>) -> Vec<Vec<usize>> {
    let n = v.len();
    v.into_iter().permutations(n).collect()
}

fn gap_assignments_simple(gaps: &Vec<Vec<usize>>, faces: &Vec<usize>) -> Vec<Vec<Vec<usize>>>{
    let n = gaps.len();
    let mut active = vec![vec![vec![]; n]];
    let mut new: Vec<Vec<Vec<usize>>> = vec![];
    for face in faces.into_iter() {
        for assignment in active.iter() {
            for i in (0..n).filter(|i| gaps[*i].contains(&face)).into_iter() {
                let mut new_assignment: Vec<Vec<usize>> = assignment.clone();
                new_assignment[i].push(*face);
                new.push(new_assignment);
            }
        }
        active = new.clone();
        new.clear();
    }
    let mut res = vec![];
    for a in active.into_iter() {
        for choice in a.into_iter().map(|x| replace_by_permutations(x)).multi_cartesian_product() {
            res.push(choice);
        }
    }
    res
}

fn combine_to_layer(bridges: &Vec<usize>, gaps: &Vec<Vec<usize>>) -> Vec<usize> {
    let mut layer = vec![];
    for i in 0..bridges.len() {
        for elem in gaps[i].iter() {
            layer.push(*elem);
        }
        layer.push(bridges[i]);
    }
    layer
}

fn duplicates_removed(v: Vec<usize>) -> Vec<usize> {
    let mut res = vec![v[0]];
    for i in 1..v.len() {
        if v[i-1] != v[i] && v[i] != v[0] {
            res.push(v[i]);
        }
    }
    res
}

//          Indices for the next_layers function:
//
//  indices:        0    1    2    3    4
//                  |    |    |    |    |
//  last_layer:     a    b    c    d    e
//                    /    /    /    /    /
//                   /    /    /    /    /
//  bridges:        a    b    c    d    e
//                \    \    \    \    \
//                 \    \    \    \    \
//  gaps:           a    b    c    d    e
//                  |    |    |    |    |
//  indices:        0    1    2    3    4

pub fn next_layers_simple(last_layer: &Vec<usize>, l: &Lattice) -> Vec<Vec<usize>> {
    //println!("{:?}", last_layer.clone().into_iter().map(|i| l.faces[i].label.clone()).collect::<Vec<_>>());
    let dim = l.faces[last_layer[0]].dim;
    let n = last_layer.len();
    // let mut bridges_option = vec![];
    // for x in 0..n {
    //     //println!("{:?}", min_max(last_layer[x], last_layer[(x+1)%n]));
    //     bridges_option.push(l.bridges[&min_max(last_layer[x], last_layer[(x+1)%n])]);
    // }
    let bridges: Vec<_> = (0..n).map(|x| l.bridges[&min_max(last_layer[x], last_layer[(x+1)%n])]).collect();
    // let mut bridges = vec![1000; n];
    // for i in 0..n {
    //     if !bridges_option[i].is_none() {
    //         let mut last_bridge = bridges_option[i].unwrap();
    //         for j in 0..n {
    //             if bridges_option[(i+j)%n].is_none() {
    //                 bridges[(i+j)%n] = last_bridge;
    //             } else {
    //                 last_bridge = bridges_option[(i+j)%n].unwrap();
    //                 bridges[(i+j)%n] = last_bridge;
    //
    //             }
    //         }
    //         break
    //     }
    // }
    /*println!("{:?}", bridges);
    println!("{:?}", bridges.clone().into_iter().map(|i| l.faces[i].label.clone()).collect::<Vec<_>>());
    */
    if !layer_ok(&bridges) { return vec![] };
    let faces_left: &Vec<_> = &l.levels[dim+1].clone().into_iter().filter(|x| !bridges.contains(x)).collect();
    let gaps: Vec<_> = (0..n).map(|i| l.faces[last_layer[i]].upset.clone().into_iter().filter(|x| !bridges.contains(x)).collect::<Vec<_>>()).collect();

    gap_assignments_simple(&gaps, &faces_left)
        .iter()
        .map(|x| combine_to_layer(&bridges, x))
        .filter(|x| layer_ok(x))
        .map(|x| duplicates_removed(x))
        .collect()
}

pub fn rhombic_strips_dfs_simple(strip: Vec<Vec<usize>>, l: &Lattice, max_dim: usize) -> Vec<Vec<Vec<usize>>> {

    //if strips.len() == 0 { return vec![] };
    if max_dim == strip.len()-1 { return vec![strip] };
    let mut continuations = vec![];

    for next_layer in next_layers_simple(&strip[strip.len()-1], l).into_iter() {
        let mut new_strip = strip.clone();
        new_strip.push(next_layer);
        continuations.push(new_strip);
    }
    continuations.into_par_iter().map(|x| rhombic_strips_dfs_simple(x, l, max_dim)).flatten().collect()
}











// struct GapAssignments {
//     gaps: Vec<Vec<Vec<usize>>>,
//     num_faces: usize,
//     counter: Vec<usize>,
//     seen_zero: bool, //dirty, find a better way
// }
//
// //Gap_assignment needs alg X, else it is too slow
//
// impl GapAssignments {
//
//     fn advance_counter(&mut self, full_until: usize) -> bool { //return false if there was an overflow, else true
//         if full_until >= self.gaps.len() { return false };
//         if self.counter[full_until] == self.gaps[full_until].len()-1 { return self.advance_counter(full_until+1) };
//         self.counter[full_until] += 1;
//         for i in 0..full_until {
//             self.counter[i] = 0;
//         }
//         true
//     }
//
//     fn next(&mut self) -> usize { //0: no success, 1: success, 2: done
//         if self.seen_zero {
//             if !self.advance_counter(0) {
//                 return 2;
//             };
//         };
//         self.seen_zero = true;
//         let mut all_seen = vec![];
//         let assignment: Vec<_> = (0..self.gaps.len()).map(|x| self.gaps[x][self.counter[x]].clone()).collect();  //this clone might be avoidable
//         for g in assignment.iter() {
//             for elem in g.iter() {
//                 if all_seen.contains(elem) {
//                     return 0;
//                 };
//                 all_seen.push(*elem);
//             }
//         }
//         if all_seen.len() != self.num_faces {
//             return 0;
//         }
//         1
//     }
//
// }

// pub fn test_GapAssignments() {
//     let mut g = GapAssignments {
//         gaps: vec![vec![vec![1,2,3], vec![1], vec![2]], vec![vec![1], vec![2], vec![3]], vec![vec![1], vec![3]]],
//         all_faces: vec![],
//         counter: vec![0,0,0],
//     };
//     while g.next() {
//         println!("{:?}", g.counter);
//     }
// }

// fn min_one_mod_n(i: &usize, n: &usize) -> usize {
//     match i {
//         0 => { n-1 },
//         _ => { i-1 },
//     }
// }







// //try dfs for this, might be faster
// fn gap_assignments_non_simple(gap: &Vec<usize>, start: usize, end: usize, l: &Lattice) -> Vec<Vec<usize>>{
//     //all possible paths across this gap as a vector. The graph is encoded in the lattice as the keys of the bridges object
//
//     if gap.len() == 0 { return vec![vec![]]; };
//     if start == end { return vec![vec![]] };
//
//     let g: Vec<usize> = gap.into_iter().map(|x| x.clone()).collect();
//     let mut result: Vec<Vec<usize>> = vec![];
//     if l.bridges.contains_key(&min_max(start, end)) { result.push(vec![]); };
//
//     let mut active: Vec<Vec<usize>> = g.clone().into_iter().filter(|x| l.bridges.contains_key(&min_max(start, *x))).map(|x| vec![x]).collect();
//     let mut new = vec![];
//
//     while active.len() != 0 {
//         new.clear();
//         for path in active.into_iter() {
//             let p_head = path[path.len()-1];
//             if l.bridges.contains_key(&min_max(p_head, end)) {
//                 result.push(path.clone());
//             }
//             for face in g.iter() {
//                 if !path.contains(face) && l.bridges.contains_key(&min_max(p_head, *face)) {
//                     let mut new_path = path.clone();
//                     new_path.push(*face);
//                     new.push(new_path);
//                 }
//             }
//         }
//         active = new.clone();
//     }
//     result
// }




// pub fn next_layers_non_simple(last_layer: &Vec<usize>, l: &Lattice) -> Vec<Vec<usize>> {
//     let dim = l.faces[last_layer[0]].dim;
//     let n = last_layer.len();
//     let bridges_option: Vec<_> = (0..n).map(|x| l.bridges[&min_max(last_layer[x], last_layer[(x+1)%n])]).collect();
//     let mut bridges = vec![0; n];
//     for i in 0..n {
//         if !bridges_option[i].is_none() {
//             let mut last_bridge = bridges_option[i].unwrap();
//             for j in 0..n {
//                 if bridges_option[(i+j)%n].is_none() {
//                     bridges[(i+j)%n] = last_bridge;
//                 } else {
//                     last_bridge = bridges_option[(i+j)%n].unwrap();
//                     bridges[(i+j)%n] = last_bridge;
//
//                 }
//             }
//             break
//         }
//     }
//     if !layer_ok(&bridges) { return vec![] };
//     //let faces_left: Vec<_> = <Vec<usize> as Clone>::clone(&l.levels[dim+1]).into_iter().filter(|x| !bridges.contains(x)).collect();
//     let gaps: Vec<_> = (0..n).map(|i| l.faces[last_layer[i]].upset.clone().into_iter().filter(|x| !bridges.contains(x)).collect::<Vec<_>>()).collect();
//     let assignments: Vec<Vec<Vec<usize>>> = (0..n).map(|i| gap_assignments_non_simple(&gaps[i], bridges[min_one_mod_n(&i, &n)], bridges[i], l)).collect::<Vec<_>>();
//
//     // println!("{:?}", gaps);
//     // println!("{:?}", assignments);
//
//     let mut g = GapAssignments {
//         gaps: assignments,
//         num_faces: l.levels[dim+1].iter().filter(|x| !bridges.contains(x)).collect::<Vec<_>>().len(),
//         counter: vec![0; last_layer.len()],
//         seen_zero: false,
//     };
//     let mut layers = vec![];
//
//     loop {
//         match g.next() {
//             0 => (),
//             1 => layers.push(combine_to_layer(&bridges, &(0..g.gaps.len()).map(|x| g.gaps[x][g.counter[x]].clone()).collect::<Vec<_>>())),
//             _ => break,
//         }
//     }
//     layers
// }
//
// pub fn rhombic_strips_dfs_non_simple(strip: Vec<Vec<usize>>, l: &Lattice, max_dim: usize) -> Vec<Vec<Vec<usize>>> {
//
//     //if strips.len() == 0 { return vec![] };
//     if max_dim == strip.len()-1 { return vec![strip] };
//     let mut continuations = vec![];
//
//     for next_layer in next_layers_non_simple(&strip[strip.len()-1], l).into_iter() {
//         let mut new_strip = strip.clone();
//         new_strip.push(next_layer);
//         continuations.push(new_strip);
//     }
//     continuations.into_par_iter().map(|x| rhombic_strips_dfs_non_simple(x, l, max_dim)).flatten().collect()
// }
//
//
//
//
//
//
//



















