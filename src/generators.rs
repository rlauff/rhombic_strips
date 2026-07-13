use rhombic_strips::web::api::*;
use serde_json::Value;

fn parse(s: &str) -> Value { serde_json::from_str(s).unwrap() }
fn counts(v: &Value) -> (usize, usize) {
    (v["labels"].as_array().unwrap().len(), v["edges"].as_array().unwrap().len())
}

#[test]
fn generator_invariants() {
    let g = parse(&gen_grid("211").unwrap());
    assert_eq!(counts(&g), (12, 20));
    assert_eq!(g["ranks"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap()).max(), Some(4));

    assert_eq!(counts(&parse(&gen_cube(2).unwrap())), (9, 12));
    assert_eq!(counts(&parse(&gen_cube(3).unwrap())).0, 27);
    assert_eq!(counts(&parse(&gen_simplex(2).unwrap())).0, 7);
    assert_eq!(counts(&parse(&gen_simplex(3).unwrap())).0, 15);

    // J(antichain_3) = B_3
    let anti = r#"{"labels":["0","1","2"],"edges":[]}"#;
    assert_eq!(counts(&parse(&gen_distributive(anti).unwrap())), (8, 12));

    // tubes / tubings
    let path3 = gen_graph("path", 3).unwrap();
    let tp = parse(&gen_tube_poset(&path3).unwrap());
    assert_eq!(counts(&tp), (6, 6));
    let pent = parse(&gen_graph_associahedron(&path3).unwrap());
    assert_eq!(counts(&pent).0, 11);
    let hex = parse(&gen_graph_associahedron(&gen_graph("complete", 3).unwrap()).unwrap());
    assert_eq!(counts(&hex).0, 13);
    let assoc3 = parse(&gen_graph_associahedron(&gen_graph("path", 4).unwrap()).unwrap());
    assert_eq!(counts(&assoc3).0, 45);
    let perm3 = parse(&gen_graph_associahedron(&gen_graph("complete", 4).unwrap()).unwrap());
    assert_eq!(counts(&perm3).0, 75);
    assert_eq!(perm3["ranks"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap()).max(), Some(3));

    // file round-trip
    let file = to_lattice_file(&gen_cube(2).unwrap()).unwrap();
    let back = parse(&from_lattice_file(&file).unwrap());
    assert_eq!(counts(&back), (9, 12));

    // cycle rejection
    let cyc = r#"{"labels":["a","b"],"edges":[[0,1],[1,0]]}"#;
    assert!(poset_ranks(cyc).is_err());
}
