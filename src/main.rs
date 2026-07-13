use rhombic_strips::{gui, lattice, plotting, rhombic};

use crate::lattice::Lattice;
use crate::rhombic::{count_strips, extensions, strip_exists, strips};

fn main() {
    let interactive_mode = std::env::args().any(|arg| arg == "--interactive");
    if interactive_mode {
        gui::interactive();
        return;
    }

    let source = std::env::args()
        .nth(1)
        .expect("Please provide a file from which to read in the lattice.");

    let cyclic = std::env::args().any(|arg| arg == "--cyclic"); // restrict to cyclic rhombic strips
    let count = std::env::args().any(|arg| arg == "--count"); // find all rhombic strips and print their number
    let show = std::env::args().any(|arg| arg == "--show"); // render the first found strip
    let enumerate = std::env::args().any(|arg| arg == "--enumerate"); // split the count among the hamilton paths/cycles
    let show_all = std::env::args().any(|arg| arg == "--show-all"); // render all found strips
    let show_cyclic = std::env::args().any(|arg| arg == "--show-cyclic"); // render in cyclic layout

    process_lattice(
        &source,
        cyclic,
        count,
        show,
        enumerate,
        show_all,
        show_cyclic,
    );
}

fn process_lattice(
    source: &str,
    cyclic: bool,
    count: bool,
    show: bool,
    enumerate: bool,
    show_all: bool,
    show_cyclic: bool,
) {
    let l = match Lattice::from_file(source) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let labels = |layer: &[lattice::FaceId]| -> Vec<String> {
        layer
            .iter()
            .map(|&x| l.face(x).label().to_string())
            .collect()
    };

    if enumerate {
        // split the total count among the hamilton paths/cycles of level 0
        let mut total = 0;
        for ham in l.ham_paths(cyclic) {
            let n = extensions(vec![ham.clone()], &l, l.dim(), cyclic).count();
            println!("{:?}: {}", labels(&ham), n);
            total += n;
        }
        println!("Number of rhombic strips found: {}", total);
        return;
    }

    if count {
        println!(
            "Number of rhombic strips found: {}",
            count_strips(&l, cyclic)
        );
        return;
    }

    if show || show_cyclic || show_all {
        let mut found = 0;
        for strip in strips(&l, cyclic) {
            plotting::show_strip(&strip, &l, show_cyclic);
            for layer in &strip {
                println!("{:?}", labels(layer));
            }
            found += 1;
            if !show_all {
                break;
            }
            println!();
        }
        if found == 0 {
            println!("No rhombic strip exists!");
        }
        return;
    }

    // default: existence check with early exit
    if strip_exists(&l, cyclic) {
        println!("A rhombic strip was found");
    } else {
        println!("No rhombic strip exists!");
    }
}
