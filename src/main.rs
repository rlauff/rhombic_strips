
mod lattice;
use crate::lattice::*;

mod rhombic;
use crate::rhombic::*;

fn main() {
    // read in source
    let source = std::env::args().nth(1)
        .expect("Please provide a file from which to read in the lattice.");
    // check for flags
    let cyclic = std::env::args().any(|arg| arg == "--cyclic"); // restrict to cyclic rhombic strips
    let count = std::env::args().any(|arg| arg == "--count");   // find all rhombic strips and print their number
    let show = std::env::args().any(|arg| arg == "--show");     // print out the found strips
    let split = std::env::args().any(|arg| arg == "--split");   // find all and split the amount among the hamilton cycles

    let l = lattice_from_file(&source, cyclic);

    let mut number_found = 0;
    for ham in l.ham.iter() {
        if !count && !split {
            if rhombic_strip_exists(&ham.clone(), 0, &l, l.dim.clone(), cyclic) {
                println!("A rhombic strip was found");
                number_found = 1;
                break;
            }
        } else {
            let new = rhombic_strips_dfs_simple(vec![ham.clone()], &l, l.dim.clone(), cyclic);
            number_found += new.len();
            if split {
                println!("{:?}: {}", ham.iter().map(|x| l.faces[*x].label.clone()).collect::<Vec<_>>(), new.len());
            }
            if show {
                for strip in new.iter() {
                    for layer in strip.iter() {
                        println!("{:?}", layer.iter().map(|x| l.faces[*x].label.clone()).collect::<Vec<_>>());
                    }
                    println!();
                    if !count && !split { break; };
                }
            }
            if !count && !split && new.len() > 0 { break };
        }
    }
    if count || split {
        println!("Number of rhombic strips found: {}", number_found);
    }
    if !count && !split && number_found == 0 {
        println!("No rhombic strip exists!");
    }
}
