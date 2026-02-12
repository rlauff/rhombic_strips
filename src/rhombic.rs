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

fn layer_ok_non_cyclic(layer: &Vec<usize>) -> bool { //returns wether the list of bridges is ok, i.e. each face appears in an interval along it
    for face in layer {
        if !is_good_for_path(&(0..layer.len()).filter(|x| layer[*x] == *face).collect()) {
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
    for face in faces.into_iter() {
        let mut new: Vec<Vec<Vec<usize>>> = vec![];
        for assignment in active.iter() {
            for i in (0..n).filter(|i| gaps[*i].contains(face)).into_iter() {
                let mut new_assignment: Vec<Vec<usize>> = assignment.clone();
                new_assignment[i].push(*face);
                new.push(new_assignment);
            }
        }
        active = new;
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

fn combine_to_layer_non_cyclic(bridges: &Vec<usize>, gaps: &Vec<Vec<usize>>) -> Vec<usize> {
    let mut layer = vec![];
    for i in 0..bridges.len() {
        for elem in gaps[i].iter() {
            layer.push(*elem);
        }
        layer.push(bridges[i]);
    }
    layer.append(&mut gaps[gaps.len()-1].clone()); // the last gap is after the last bridge in the non-cyclic case, while in the cyclic case it is before the first bridge, but since we check for duplicates later it doesn't matter where we put it, so we put it here to avoid an extra if statement in the loop
    layer
}

fn duplicates_removed(v: Vec<usize>) -> Vec<usize> {
    if v.len() == 0 { return vec![] };
    let mut res = vec![v[0]];
    for i in 1..v.len() {
        if v[i-1] != v[i] && v[i] != v[0] {
            res.push(v[i]);
        }
    }
    res
}

//          Indices for the next_layers function, when looking for cyclic strips:
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

pub fn _next_layers_simple_cyclic(last_layer: &Vec<usize>, l: &Lattice) -> Vec<Vec<usize>> {
    //println!("{:?}", last_layer.clone().into_iter().map(|i| l.faces[i].label.clone()).collect::<Vec<_>>());
    let dim = l.faces[last_layer[0]].dim;
    let n = last_layer.len();
    let bridges: Vec<_> = (0..n).map(|x| l.bridges[&min_max(last_layer[x], last_layer[(x+1)%n])]).collect();
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


//          Indices for the next_layers function, when looking for non-cyclic strips:
//
//  indices:        0    1    2    3    4
//                  |    |    |    |    |
//  last_layer:     a    b    c    d    e
//                    /    /    /    /    
//                   /    /    /    /    
//  bridges:        a    b    c    d    
//                \    \    \    \    \
//                 \    \    \    \    \
//  gaps:           a    b    c    d    e
//                  |    |    |    |    |
//  indices:        0    1    2    3    4

pub fn next_layers_simple_non_cyclic(last_layer: &Vec<usize>, l: &Lattice) -> Vec<Vec<usize>> {
    assert!(!last_layer.is_empty());
    let dim = l.faces[last_layer[0]].dim;
    let n = last_layer.len();
    // generate the bridges. Note that the range goes from 0 to n-2 inclusive, because in this case the number of bridges is n-1
    let bridges: Vec<_> = (0..n-1)
        // replace each index x by the bridge connecting positions x and x+1
        .map(|x| l.bridges[&min_max(last_layer[x], last_layer[x+1])])
        .collect();

    // if the last layer is not ok (violating the interval property), then no next layer will be valid
    if !layer_ok_non_cyclic(&bridges) { return vec![] };   

    // generate the faces which remain. These need to be assigned to the gaps later
    let mut faces_left = Vec::with_capacity(l.levels[dim+1].len());
    for x in l.levels[dim+1].iter() {
        if !bridges.contains(x) {
            faces_left.push(*x);
        }
    }

    // generate the list of gaps. These contain, a gap is the subset of faces from faces_left which can go in that place
    let mut gaps = Vec::new();
    for i in 0..n {
        let mut new_gap = Vec::with_capacity(l.faces[last_layer[i]].upset.len());
        for x in l.faces[last_layer[i]].upset.iter() {
            if !bridges.contains(x) {
                new_gap.push(*x);
            }
        }
        gaps.push(new_gap);
    }

    // itereate over all gap_assignments and build the possible next layers
    gap_assignments_simple(&gaps, &faces_left)
        .iter()
        .map(|x| combine_to_layer_non_cyclic(&bridges, x))
        .filter(|x| layer_ok_non_cyclic(x))
        .map(|x| duplicates_removed(x))
        .collect()
}

pub fn rhombic_strips_dfs_simple(strip: Vec<Vec<usize>>, l: &Lattice, max_dim: usize, cyclic: bool) -> Vec<Vec<Vec<usize>>> {

    if max_dim == strip.len()-1 {
        //println!("{:?}", strip);
        return vec![strip];

    };
    let mut continuations = vec![];
    let next_function = if cyclic { _next_layers_simple_cyclic } else { next_layers_simple_non_cyclic };

    for next_layer in next_function(&strip[strip.len()-1], l).into_iter() {
        let mut new_strip = strip.clone();
        new_strip.push(next_layer);
        continuations.push(new_strip);
    }
    continuations.into_par_iter().map(|x| rhombic_strips_dfs_simple(x, l, max_dim, cyclic)).flatten().collect()
}

pub fn rhombic_strip_exists(
    current_layer: &Vec<usize>, // Only the most recent layer is needed
    current_dim: usize,      // Tracks the current depth/dimension manually
    l: &Lattice, 
    max_dim: usize, 
    cyclic: bool
) -> bool {

    // Base case: If we have reached the target dimension, a strip exists
    if current_dim == max_dim {
        return true;
    }

    let next_function = if cyclic { _next_layers_simple_cyclic } else { next_layers_simple_non_cyclic };
    
    // Generate the potential next layers based only on the current layer
    let next_layers = next_function(current_layer, l);

    // Use par_iter with any() to check paths in parallel without storing them.
    // This returns true immediately if any branch finds a solution.
    next_layers.par_iter().any(|next_layer| {
        rhombic_strip_exists(next_layer, current_dim + 1, l, max_dim, cyclic)
    })
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



















