
mod lattice;
use crate::lattice::*;
mod rhombic;
use crate::rhombic::*;
mod gui;
use crate::gui::*;
mod plotting;
use crate::plotting::*;

pub const MAX_FACES: usize = 500; // maximum number of faces in the lattice, used for fixed-size arrays
pub const MAX_UP_DOWN: usize = 100; // maximum number of faces in the upset/downset of a face, used for fixed-size arrays
pub const MAX_LEVELS: usize = 100; // maximum number of levels in the lattice, used for fixed-size arrays


fn main() {
    // read in source
    let source = std::env::args().nth(1)
        .expect("Please provide a file from which to read in the lattice.");
    // check for flags
    let cyclic = std::env::args().any(|arg| arg == "--cyclic"); // restrict to cyclic rhombic strips
    let count = std::env::args().any(|arg| arg == "--count");   // find all rhombic strips and print their number
    let show = std::env::args().any(|arg| arg == "--show");     // print out the found strips (only first one)
    let enumerate = std::env::args().any(|arg| arg == "--enumerate");   // find all and split the amount among the hamilton cycles
    let show_all = std::env::args().any(|arg| arg == "--show-all"); // show all found strips
    let show_cyclic = std::env::args().any(|arg| arg == "--show-cyclic"); // show all strips in cyclic layout
    let interactive_mode = std::env::args().any(|arg| arg == "--interactive"); // interactive mode with GUI, only shows the first found strip, but allows to interactively explore it and the lattice

    if interactive_mode {
        interactive();
        return;
    }

    process_lattice(source, cyclic, count, show, enumerate, show_all, show_cyclic);
}

fn process_lattice(source: String, cyclic: bool, count: bool, show: bool, enumerate: bool, show_all: bool, show_cyclic: bool) {
    let l = lattice_from_file(&source);

    let mut number_found = 0;
    for ham in l.ham_paths(cyclic) {
        if !count && !enumerate && !show && !show_cyclic && !show_all {
            if strip_exists(&ham.clone(), 0, &l, cyclic) {
                println!("A rhombic strip was found");
                number_found = 1;
                break;
            }
        } else if !count && !enumerate && !show_all {
            println!("Checking ham cycle: {:?}", ham.iter().map(|x| l.faces.get_unchecked(*x as u8).label.clone()).collect::<Vec<_>>());
            // show a single strip if it exists
            if let Some(strip) = find_first_rhombic_strip_lazy(vec![ham], &l, cyclic) {
                show_strip(&strip, &l, show_cyclic);
                number_found = 1;
                break;
            } else {
                continue; // No strip for this ham, try next
            }
        } else {
            let new = extensions_dfs_lazy(vec![ham.clone()], &l, cyclic);
            number_found += new.len();
            if enumerate {
                println!("{:?}: {}", ham.iter().map(|x| l.faces.get_unchecked(*x as u8).label.clone()).collect::<Vec<_>>(), new.len());
            }
            if show || show_cyclic {
                for strip in new.iter() {
                    show_strip(strip, &l, show_cyclic);
                    for layer in strip.iter() {
                        println!("{:?}", layer.iter().map(|x| l.faces.get_unchecked(*x as u8).label.clone()).collect::<Vec<_>>());
                    }
                    if !show_all { break; };
                }
            }
            if !count && !enumerate && new.len() > 0 { break };
        }
    }
    if count || enumerate {
        println!("Number of rhombic strips found: {}", number_found);
    }
    if !count && !enumerate && number_found == 0 {
        println!("No rhombic strip exists!");
    }
}
