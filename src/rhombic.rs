use crate::lattice::*;
use std::cmp;
use rayon::prelude::*;
use itertools::Itertools;

// Basic helpers remain the same
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

fn layer_ok(layer: &Vec<usize>) -> bool {
    for face in layer {
        if !is_good_for_cycle(&(0..layer.len()).filter(|x| layer[*x] == *face).collect(), layer.len()) {
            return false
        }
    }
    true
}

fn layer_ok_non_cyclic(layer: &Vec<usize>) -> bool {
    for face in layer {
        if !is_good_for_path(&(0..layer.len()).filter(|x| layer[*x] == *face).collect()) {
            return false
        }
    }
    true
}

fn combine_to_layer(bridges: &Vec<usize>, gaps: &Vec<Vec<usize>>) -> Vec<usize> {
    let mut layer = vec![];
    for i in 0..bridges.len() {
        // Gaps in lazy iterator come as Vec<usize>, simply push contents
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
    layer.append(&mut gaps[gaps.len()-1].clone()); 
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

/// LAZY GAP ASSIGNMENTS
/// Instead of returning a huge Vec<Vec<Vec<usize>>>, this returns an Iterator.
/// Each item yielded is one valid assignment of faces to gaps (with permutations applied).
/// 
/// Logic:
/// 1. Identify valid gap choices for each face.
/// 2. Use `multi_cartesian_product` to generate assignments of faces to gap indices.
/// 3. Reconstruct the buckets (gaps).
/// 4. Permute the faces within each bucket.
fn gap_assignments_lazy(
    gaps_allowed: Vec<Vec<usize>>, // gaps_allowed[i] = list of faces allowed in gap i
    faces_to_place: Vec<usize>
) -> impl Iterator<Item = Vec<Vec<usize>>> {
    
    let n_gaps = gaps_allowed.len();
    
    // Invert the mapping: For each face, which gap indices allow it?
    // choices_per_face[k] = list of gap indices that can accept faces_to_place[k]
    let choices_per_face: Vec<Vec<usize>> = faces_to_place.iter().map(|&face| {
        (0..n_gaps).filter(|&i| gaps_allowed[i].contains(&face)).collect()
    }).collect();

    // 1. Distribute faces to gaps
    choices_per_face.into_iter()
        .multi_cartesian_product()
        .map(move |assignment_indices| {
            // Reconstruct the buckets based on the indices chosen
            let mut buckets = vec![vec![]; n_gaps];
            // assignment_indices[k] is the gap index chosen for faces_to_place[k]
            for (face_idx, &gap_idx) in assignment_indices.iter().enumerate() {
                buckets[gap_idx].push(faces_to_place[face_idx]);
            }
            buckets
        })
        // 2. Permute faces within the gaps
        .flat_map(|buckets| {
             // buckets is Vec<Vec<usize>>. We need the cartesian product of permutations of each inner vec.
             // We map each bucket to an iterator of its permutations.
             buckets.into_iter()
                .map(|b| b.clone().into_iter().permutations(b.len())) 
                .multi_cartesian_product() // This generates the combinations of the permutations
        })
}

/// Lazy generator for Cyclic next layers
pub fn next_layers_cyclic_lazy<'a>(last_layer: &'a Vec<usize>, l: &'a Lattice) -> impl Iterator<Item = Vec<usize>> + 'a {
    // Basic validations
    if last_layer.is_empty() {
         // Return an empty iterator if input is invalid
         return itertools::Either::Left(std::iter::empty()); 
    }

    let dim = l.faces[last_layer[0]].dim;
    let n = last_layer.len();
    let bridges: Vec<_> = (0..n).map(|x| l.bridges[&min_max(last_layer[x], last_layer[(x+1)%n])]).collect();
    
    if !layer_ok(&bridges) { 
        return itertools::Either::Left(std::iter::empty()); 
    };

    let faces_left: Vec<_> = l.levels[dim+1].clone().into_iter().filter(|x| !bridges.contains(x)).collect();
    let gaps: Vec<_> = (0..n).map(|i| l.faces[last_layer[i]].upset.clone().into_iter().filter(|x| !bridges.contains(x)).collect::<Vec<_>>()).collect();

    // We use the lazy generator
    // We map the results immediately to the combined layer and filter/dedup
    let iter = gap_assignments_lazy(gaps, faces_left)
        .map(move |x| combine_to_layer(&bridges, &x)) // combine uses the bridges captured by move
        .filter(|x| layer_ok(x))
        .map(|x| duplicates_removed(x));

    itertools::Either::Right(iter)
}

/// Lazy generator for Non-Cyclic next layers
pub fn next_layers_non_cyclic_lazy<'a>(last_layer: &'a Vec<usize>, l: &'a Lattice) -> impl Iterator<Item = Vec<usize>> + 'a {
    assert!(!last_layer.is_empty());
    let dim = l.faces[last_layer[0]].dim;
    let n = last_layer.len();
    
    let bridges: Vec<_> = (0..n-1)
        .map(|x| l.bridges[&min_max(last_layer[x], last_layer[x+1])])
        .collect();

    if !layer_ok_non_cyclic(&bridges) { 
        return itertools::Either::Left(std::iter::empty()); 
    };   

    let mut faces_left = Vec::with_capacity(l.levels[dim+1].len());
    for x in l.levels[dim+1].iter() {
        if !bridges.contains(x) {
            faces_left.push(*x);
        }
    }

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

    let iter = gap_assignments_lazy(gaps, faces_left)
        .map(move |x| combine_to_layer_non_cyclic(&bridges, &x))
        .filter(|x| layer_ok_non_cyclic(x))
        .map(|x| duplicates_removed(x));

    itertools::Either::Right(iter)
}

// The main function replacing rhombic_strips_dfs_simple
pub fn rhombic_strips_dfs_lazy(
    strip: Vec<Vec<usize>>, 
    l: &Lattice, 
    max_dim: usize, 
    cyclic: bool
) -> Vec<Vec<Vec<usize>>> {

    // Base case: if we reached the max dimension, return the current strip
    if max_dim == strip.len() - 1 {
        return vec![strip];
    }

    // Get the lazy iterator for the next layers
    // We cannot use a simple 'if' assignment because the iterator types differ
    let next_layers_iter: Box<dyn Iterator<Item = Vec<usize>> + Send> = if cyclic {
        Box::new(next_layers_cyclic_lazy(&strip[strip.len()-1], l))
    } else {
        Box::new(next_layers_non_cyclic_lazy(&strip[strip.len()-1], l))
    };

    // Use par_bridge() to parallelize the consumption of the lazy iterator.
    // This allows us to process branches in parallel as they are generated,
    // without ever storing all possible next layers in memory at once.
    next_layers_iter.par_bridge()
        .map(|next_layer| {
            let mut new_strip = strip.clone();
            new_strip.push(next_layer);
            rhombic_strips_dfs_lazy(new_strip, l, max_dim, cyclic)
        })
        .flatten()
        .collect()
}

/// Optimized Existence Check using Lazy Generation + ParBridge
pub fn rhombic_strip_exists(
    current_layer: &Vec<usize>, 
    current_dim: usize, 
    l: &Lattice, 
    max_dim: usize, 
    cyclic: bool
) -> bool {

    if current_dim == max_dim {
        return true;
    }

    // Since our next_layers functions now return Iterators, we cannot assign them to a variable easily
    // because the types of the iterators differ (opaque types).
    // We simply branch the logic here.
    
    if cyclic {
        let next_iter = next_layers_cyclic_lazy(current_layer, l);
        // par_bridge converts the sequential iterator into a parallel one.
        // It pulls items from the iterator on one thread and distributes processing to others.
        next_iter.par_bridge().any(|next_layer| {
            rhombic_strip_exists(&next_layer, current_dim + 1, l, max_dim, cyclic)
        })
    } else {
        let next_iter = next_layers_non_cyclic_lazy(current_layer, l);
        next_iter.par_bridge().any(|next_layer| {
            rhombic_strip_exists(&next_layer, current_dim + 1, l, max_dim, cyclic)
        })
    }
}