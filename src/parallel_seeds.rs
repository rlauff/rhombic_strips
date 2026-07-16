//! The parallel strip search decomposes the hamiltonian-path DFS into
//! prefix-seeded subtrees (`Lattice::ham_path_seeds`). These tests pin down
//! the invariant everything rests on: the seeds partition the sequential
//! search exactly — same paths, same strips, same counts, no duplicates —
//! for paths and cycles alike.

use std::collections::BTreeSet;

use rhombic_strips::lattice::Lattice;
use rhombic_strips::rhombic::{count_strips, strip_exists, strips, strips_parallel};
use rhombic_strips::web::api::{
    gen_cube, gen_graph, gen_graph_associahedron, gen_grid, gen_simplex, wire_to_faces,
    WireGraph,
};

fn lattice_from(json: &str) -> Lattice {
    let wire = WireGraph::parse(json).expect("generator output parses");
    Lattice::from_faces(wire_to_faces(&wire).expect("generator output is a poset"))
}

fn examples() -> Vec<(String, Lattice)> {
    let mut out = vec![
        ("cube2".into(), lattice_from(&gen_cube(2).unwrap())),
        ("cube3".into(), lattice_from(&gen_cube(3).unwrap())),
        ("simplex3".into(), lattice_from(&gen_simplex(3).unwrap())),
        ("grid221".into(), lattice_from(&gen_grid("221").unwrap())),
        ("grid33".into(), lattice_from(&gen_grid("33").unwrap())),
    ];
    // associahedron A_4 = graph associahedron of the path on 4 vertices
    let path4 = gen_graph("path", 4).unwrap();
    out.push((
        "assoc4".into(),
        lattice_from(&gen_graph_associahedron(&path4).unwrap()),
    ));
    out
}

/// Multisets must agree, so compare sorted vectors (the parallel order is
/// scheduler-dependent). Duplicate paths would show up here too.
fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v
}

#[test]
fn seeds_partition_hamiltonian_paths() {
    for (name, l) in examples() {
        for cyclic in [false, true] {
            let sequential: Vec<_> = l.ham_paths(cyclic).collect();
            for target in [1, 2, 7, 64, 4096] {
                let seeded: Vec<_> = l
                    .ham_path_seeds(cyclic, target)
                    .into_iter()
                    .flatten()
                    .collect();
                assert_eq!(
                    sorted(seeded.clone()),
                    sorted(sequential.clone()),
                    "{name} cyclic={cyclic} target={target}: seeded paths differ"
                );
                // exact partition: no duplicates either
                assert_eq!(
                    seeded.iter().collect::<BTreeSet<_>>().len(),
                    seeded.len(),
                    "{name} cyclic={cyclic} target={target}: duplicate paths across seeds"
                );
            }
        }
    }
}

#[test]
fn parallel_counts_match_sequential() {
    for (name, l) in examples() {
        for cyclic in [false, true] {
            let expected = strips(&l, cyclic).count();
            assert_eq!(
                count_strips(&l, cyclic),
                expected,
                "{name} cyclic={cyclic}: count_strips"
            );
            assert_eq!(
                strips_parallel(&l, cyclic).len(),
                expected,
                "{name} cyclic={cyclic}: strips_parallel"
            );
            assert_eq!(
                strip_exists(&l, cyclic),
                expected > 0,
                "{name} cyclic={cyclic}: strip_exists"
            );
        }
    }
}

#[test]
fn parallel_strips_match_sequential_as_sets() {
    for (name, l) in examples() {
        for cyclic in [false, true] {
            let seq = sorted(strips(&l, cyclic).collect::<Vec<_>>());
            let par = sorted(strips_parallel(&l, cyclic));
            assert_eq!(par, seq, "{name} cyclic={cyclic}: strip multisets differ");
        }
    }
}
