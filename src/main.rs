
mod lattice;
use crate::lattice::*;
mod rhombic;
use crate::rhombic::*;

use std::cmp;

fn main() {
    // read in source
    let source = std::env::args().nth(1)
        .expect("Please provide a file from which to read in the lattice.");
    // check for flags
    let cyclic = std::env::args().any(|arg| arg == "--cyclic"); // restrict to cyclic rhombic strips
    let count = std::env::args().any(|arg| arg == "--count");   // find all rhombic strips and print their number
    let show = std::env::args().any(|arg| arg == "--show");     // print out the found strips (only first one)
    let split = std::env::args().any(|arg| arg == "--split");   // find all and split the amount among the hamilton cycles
    let show_all = std::env::args().any(|arg| arg == "--show-all"); // show all found strips

    process_lattice(source, cyclic, count, show, split, show_all);
}

fn process_lattice(source: String, cyclic: bool, count: bool, show: bool, split: bool, show_all: bool) {
    let l = lattice_from_file(&source, cyclic);

    let mut number_found = 0;
    for ham in l.ham.iter() {
        if !count && !split && !show {
            if rhombic_strip_exists(&ham.clone(), 0, &l, l.dim.clone(), cyclic) {
                println!("A rhombic strip was found");
                number_found = 1;
                break;
            }
        } else {
            let new = rhombic_strips_dfs_lazy(vec![ham.clone()], &l, l.dim.clone(), cyclic);
            number_found += new.len();
            if split {
                println!("{:?}: {}", ham.iter().map(|x| l.faces[*x].label.clone()).collect::<Vec<_>>(), new.len());
            }
            if show {
                for strip in new.iter() {
                    show_strip(strip, &l);
                    for layer in strip.iter() {
                        println!("{:?}", layer.iter().map(|x| l.faces[*x].label.clone()).collect::<Vec<_>>());
                    }
                    if !show_all { break; };
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

fn min_max(a: usize, b: usize) -> (usize, usize) {
    return (cmp::min(a,b), cmp::max(a,b))
}

// takes the layers of a rhombic strip and returns its edges as pairs of indices into l.faces
fn edges_non_cyclic(layers: &Vec<Vec<usize>>, l: &Lattice) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for k in 0..layers.len()-1 {   // iter over the layers, except the last one
        let layer = &layers[k];
        if layer.len() == 1 {
            for x in layers[k+1].iter() {
                edges.push((layer[0], *x));
            }
        }
        let n = layer.len();
        let bridges: Vec<_> = (0..n-1)
        .map(|x| l.bridges[&min_max(layer[x], layer[x+1])])
        .collect();
        // the bridge at index i is above elements layer[i] and layer[i+1], so we add edges from both of them to the bridge
        for i in 0..(layer.len()-1) {
            edges.push((layer[i], bridges[i]));
            edges.push((layer[i+1], bridges[i]));
        }
        // next, we need to add the edges from the layer to the faces in the gaps
        // iter over the layer and keep track of how many bridges k we have seen
        // if we then find a non-bridge face, add an edge between layer[k] and that face
        let mut bridges_seen = 0;
        for face in layers[k+1].iter() {
            if bridges.contains(face) {
                // advace by the number of times face is in bridges vector
                bridges_seen += bridges.iter().filter(|x| *x == face).count();
            } else {
                edges.push((layer[bridges_seen], *face));
            }
        }
    }
    edges
}


// takes a rhombic strip as layers and generates TikZ code to visualize it
// generates the edges of the strip first, then generates the TikZ code for the faces and edges
// the tikzpicture is automatically sized to fit the strip into a4 landscape with minimal margins of 10mm around
// the tikz code is then passed directly to the system's default TeX compiler to generate a PDF, which is then opened with the default PDF viewer
// WARNING: currently only for non-cyclic strips

// formating of the strip:
// each face is printed using \node[draw, fill=white] at (x,y) {label}; (note that this must be done after everything else is drawn, otherwise the edges will be drawn on top of the faces)
// y is the dimension of the face
// the x choordinates of the faces in a layer should be equidistant, and the first and last face of each layer should be at the same y choordinates
// first, we define all coordinates using \choordinate {label} at (x,y);
// then we draw the edges using \foreach\a/\b in {edges} { \draw (\a) -- (\b); }
// then we draw the faces using \foreach\label in {labels} { \node[draw, fill=white] at (label) {\label}; }
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::collections::HashMap;

// Assuming Lattice and edges_non_cyclic are defined in your context
// fn show_strip(layers: &Vec<Vec<usize>>, l: &Lattice) { ... }

fn show_strip(layers: &Vec<Vec<usize>>, l: &Lattice) {
    let edges = edges_non_cyclic(layers, l); // Get edges as pairs of indices into l.faces

    // 1. Calculate coordinates and bounds
    // x is centered based on layer width, y is the layer index
    let mut coords: HashMap<usize, (f64, f64)> = HashMap::new();
    let mut max_x: f64 = 0.0;
    let mut min_x: f64 = 0.0;
    let max_y: f64 = (layers.len() as f64) - 1.0;
    
    // Track the maximum label length to calculate font scaling later
    let mut max_label_len: usize = 0;

    for (y_idx, layer) in layers.iter().enumerate() {
        let y = y_idx as f64;
        let width = layer.len() as f64;
        for (x_idx, &face_idx) in layer.iter().enumerate() {
            // Center the layer around x=0
            let x = (x_idx as f64) - (width - 1.0) / 2.0;
            coords.insert(face_idx, (x, y));
            
            if x > max_x { max_x = x; }
            if x < min_x { min_x = x; }

            // Check label length
            let label_len = l.faces[face_idx].label.to_string().len();
            if label_len > max_label_len {
                max_label_len = label_len;
            }
        }
    }

    // 2. Calculate scaling factors for A4 landscape (29.7cm x 21.0cm)
    // Margins 10mm -> Available area: 27.7cm x 19.0cm
    let paper_w_cm = 27.7;
    let paper_h_cm = 17.0;  // minus 2 cm, because very tall strips look bad
    
    // Determine content dimensions
    // Add a small buffer (+1.0 in lattice units) to avoid nodes touching the paper edge
    let content_w = (max_x - min_x).max(1.0) + 1.0; 
    let content_h = max_y.max(1.0) + 1.0;
    
    // Calculate independent scales for X and Y to fill the page
    let scale_x = paper_w_cm / content_w;
    let scale_y = paper_h_cm / content_h;

    // 3. Dynamic Font Size Calculation
    // Heuristic: Standard LaTeX font (10pt) is approx 3.5mm high.
    // Average character width is approx 2.2mm.
    // We calculate how much space (in cm) a label *needs* vs. how much it *has*.
    
    let char_width_cm = 0.22; 
    let node_padding_cm = 0.4; // Padding inside the node
    
    // Space needed for the longest label at 10pt font
    let needed_width_cm = (max_label_len as f64 * char_width_cm) + node_padding_cm;
    let needed_height_cm = 0.8; // Approx height of a node at 10pt
    
    // Space available for one unit in the lattice (since nodes are 1 unit apart)
    let available_width_cm = scale_x; 
    let available_height_cm = scale_y;

    // Calculate scaling ratios
    let ratio_x = available_width_cm / needed_width_cm;
    let ratio_y = available_height_cm / needed_height_cm;

    // The font scale factor is determined by the tighter constraint
    let font_scale_factor = ratio_x.min(ratio_y).min(1.0); // Cap at 1.0 (don't make font larger than 10pt)

    let font_size_pt = (10.0 * font_scale_factor).max(1.0); // Ensure min size 1pt so LaTeX doesn't crash

    // 4. Generate LaTeX
    let mut tikz = String::new();
    tikz.push_str("\\documentclass{article}\n");
    tikz.push_str("\\usepackage{tikz}\n");
    tikz.push_str("\\usepackage[landscape, a4paper, margin=10mm]{geometry}\n");
    tikz.push_str("\\begin{document}\n");
    tikz.push_str("\\thispagestyle{empty}\n");
    
    // Begin tikzpicture with independent x and y scaling
    tikz.push_str(&format!("\\begin{{tikzpicture}}[xscale={:.4}, yscale={:.4}]\n", scale_x, scale_y));
    
    // Adjust inner separation (padding) for very small fonts to avoid the box swallowing the text
    let inner_sep = if font_size_pt < 4.0 { 0.5 } else { 2.0 };
    
    // Set global node style with calculated font size
    tikz.push_str(&format!(
        "  \\tikzset{{every node/.style={{draw, fill=white, inner sep={}pt, font=\\fontsize{{{}pt}}{{{}pt}}\\selectfont}}}}\n", 
        inner_sep, font_size_pt, font_size_pt * 1.2
    ));

    // Define coordinates
    for (face_idx, (x, y)) in &coords {
        tikz.push_str(&format!("  \\coordinate (n{}) at ({:.3},{:.3});\n", face_idx, x, y));
    }

    // Draw edges
    let edge_list: Vec<String> = edges.iter()
        .map(|(a, b)| format!("n{}/n{}", a, b))
        .collect();
    
    if !edge_list.is_empty() {
        tikz.push_str(&format!("  \\foreach \\u/\\v in {{{}}} {{\n", edge_list.join(",")));
        tikz.push_str("    \\draw (\\u) -- (\\v);\n");
        tikz.push_str("  }\n");
    }

    // Draw nodes/labels
    // We collect them first to handle formatting nicely
    let mut node_defs = Vec::new();
    for face_idx in coords.keys() {
        let label = &l.faces[*face_idx].label;
        node_defs.push(format!("n{}/{}", face_idx, label));
    }

    if !node_defs.is_empty() {
        tikz.push_str(&format!("  \\foreach \\ref/\\lbl in {{{}}} {{\n", node_defs.join(",")));
        // Note: The visual style is already applied via \\tikzset above
        tikz.push_str("    \\node at (\\ref) {\\lbl};\n");
        tikz.push_str("  }\n");
    }

    tikz.push_str("\\end{tikzpicture}\n");
    tikz.push_str("\\end{document}\n");

    // 5. Write to file and compile
    let filename = "strip_visualization";
    let tex_filename = format!("{}.tex", filename);
    let mut file = File::create(&tex_filename).expect("Unable to create .tex file");
    file.write_all(tikz.as_bytes()).expect("Unable to write data");

    // Run pdflatex
    let output = Command::new("pdflatex")
        .arg("-interaction=nonstopmode")
        .arg(&tex_filename)
        .output()
        .expect("Failed to execute pdflatex. Is LaTeX installed?");

    if !output.status.success() {
        eprintln!("LaTeX compilation failed:\n{}", String::from_utf8_lossy(&output.stdout));
        return;
    }

    // Open the PDF
    let pdf_filename = format!("{}.pdf", filename);
    
    #[cfg(target_os = "macos")]
    let open_cmd = "open";
    #[cfg(target_os = "windows")]
    let open_cmd = "cmd"; 
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let open_cmd = "xdg-open"; 

    let mut command = Command::new(open_cmd);
    
    #[cfg(target_os = "windows")]
    command.args(&["/C", "start", &pdf_filename]);
    
    #[cfg(not(target_os = "windows"))]
    command.arg(&pdf_filename);

    command.spawn().expect("Failed to open PDF viewer");
}