
mod lattice;
use crate::lattice::*;

mod rhombic;
use crate::rhombic::*;

use itertools::Itertools;


fn print_layer(layer: &Vec<usize>, l: &Lattice) {
    println!("{:?}", layer.into_iter().map(|i| l.faces[*i].label.clone()).collect::<Vec<_>>());
}













fn main() {

    let l = lattice_from_file("cube3d");
    for (i, s) in rhombic_strips_dfs_simple(vec![vec![0,4,5,7,6,2,3,1]], &l, l.dim.clone()).iter().enumerate() {
        // for layer in s.iter() {
        //     print_layer(layer, &l);
        // }
        println!("{i}: {:?}", layers_to_sequence(s, &l).len());
    }


    let l = lattice_from_file("cube4d");
    // for (i, face) in l.faces.iter().enumerate() {
    //     println!("{i}: {:?}", face);
    // }
    // for b in l.bridges.iter() {
    //     println!("{:?}", b);
    // }

    for (i, s) in rhombic_strips_dfs_simple(vec![vec![0,1,3,2,6,7,5,4,12,13,15,14,10,11,9,8]], &l, l.dim.clone()).iter().enumerate() {
        // for layer in s.iter() {
        //     print_layer(layer, &l);
        // }
        println!("{i}: {:?}", layers_to_sequence(s, &l).len());
    }

}
