
mod lattice;
use crate::lattice::*;
mod rhombic;
use crate::rhombic::*;

use std::cmp;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::collections::HashMap;
use std::f64::consts::PI;

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
    let show_cyclic = std::env::args().any(|arg| arg == "--show-cyclic"); // show all strips in cyclic layout

    process_lattice(source, cyclic, count, show, split, show_all, show_cyclic);
}

fn process_lattice(source: String, cyclic: bool, count: bool, show: bool, split: bool, show_all: bool, show_cyclic: bool) {
    let l = lattice_from_file(&source);

    let mut number_found = 0;
    for ham in l.ham_paths(cyclic) {
        if !count && !split && !show && !show_cyclic {
            if rhombic_strip_exists(&ham.clone(), 0, &l, l.dim.clone() as usize, cyclic) {
                println!("A rhombic strip was found");
                number_found = 1;
                break;
            }
        } else {
            let new = rhombic_strips_dfs_lazy(vec![ham.clone()], &l, l.dim.clone() as usize, cyclic);
            number_found += new.len();
            if split {
                println!("{:?}: {}", ham.iter().map(|x| l.faces[*x as usize].label.clone()).collect::<Vec<_>>(), new.len());
            }
            if show || show_cyclic {
                for strip in new.iter() {
                    show_strip(strip, &l, show_cyclic);
                    for layer in strip.iter() {
                        println!("{:?}", layer.iter().map(|x| l.faces[*x as usize].label.clone()).collect::<Vec<_>>());
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

// takes the layers of a rhombic strip and returns its edges as pairs of indices into l.faces
fn edges_strip(layers: &Vec<Vec<usize>>, l: &Lattice, cyclic: bool) -> Vec<(usize, usize)> {
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
        .map(|x| l.bridges[layer[x] as usize *100 + layer[x + 1] as usize])
        .collect();
        // the bridge at index i is above elements layer[i] and layer[i+1], so we add edges from both of them to the bridge
        for i in 0..(layer.len()-1) {
            edges.push((layer[i], bridges[i] as usize));
            edges.push((layer[i+1], bridges[i] as usize));
        }
        // next, we need to add the edges from the layer to the faces in the gaps
        // iter over the layer and keep track of how many bridges k we have seen
        // if we then find a non-bridge face, add an edge between layer[k] and that face
        let mut bridges_seen = 0;
        for face in layers[k+1].iter() {
            if bridges.contains(&(*face as u8)) {
                // advace by the number of times face is in bridges vector
                bridges_seen += bridges.iter().filter(|x| **x == *face as u8).count();
            } else {
                edges.push((layer[bridges_seen], *face));
            }
        }
    }
    // add cyclic edges if needed
    if cyclic {
        for i in 0..(layers.len()-1) {
            // b    d
            // |    |
            // a    c
            // wither have the edge a--d or b--c. At least one always exists if its a cyclic strip
            let a = layers[i][0];
            let b = layers[i+1][0];
            let c = layers[i][layers[i].len()-1];
            let d = layers[i+1][layers[i+1].len()-1];
            if l.faces[a as usize].upset.contains(&(d as u8)) {
                edges.push((a, d));
            } else if l.faces[b as usize].downset.contains(&(c as u8)) {
                edges.push((b, c));
            } else {
                // only print and not panic so we get the stip shown for debugging
                println!("Invalid cyclic strip: edge missing between first and last layer");
            }
        }
    }
    edges
}

// written by gemini 3, cause non critical and fumbly
// takes a rhombic strip as layers and generates TikZ code to visualize it
// generates the edges of the strip first, then generates the TikZ code for the faces and edges
// the tikzpicture is automatically sized to fit the strip into a4 landscape with minimal margins of 10mm around
// the tikz code is then passed directly to the system's default TeX compiler to generate a PDF, which is then opened with the default PDF viewer

// formating of the strip:
// each face is printed using \node[draw, fill=white] at (x,y) {label}; (note that this must be done after everything else is drawn, otherwise the edges will be drawn on top of the faces)
// y is the dimension of the face
// the x choordinates of the faces in a layer should be equidistant, and the first and last face of each layer should be at the same y choordinates
// first, we define all coordinates using \choordinate {label} at (x,y);
// then we draw the edges using \foreach\a/\b in {edges} { \draw (\a) -- (\b); }
// then we draw the faces using \foreach\label in {labels} { \node[draw, fill=white] at (label) {\label}; }

fn show_strip(layers_in: &Vec<Vec<u8>>, l: &Lattice, cyclic: bool) {let layers: Vec<Vec<usize>> = layers_in
    .iter()
    .map(|layer| layer.iter().map(|x| *x as usize).collect())
    .collect();
    let edges = edges_strip(&layers, l, cyclic); 

    // 1. Calculate Layout (Coordinates)
    let mut coords: HashMap<usize, (f64, f64)> = HashMap::new();
    
    // Bounds for viewport calculation
    let mut max_x: f64 = f64::NEG_INFINITY;
    let mut min_x: f64 = f64::INFINITY;
    let mut max_y: f64 = f64::NEG_INFINITY;
    let mut min_y: f64 = f64::INFINITY;
    
    // Map face_idx -> layer_idx for quick lookup
    let mut face_to_layer_idx: HashMap<usize, usize> = HashMap::new();
    for (i, layer) in layers.iter().enumerate() {
        for &face in layer {
            face_to_layer_idx.insert(face, i);
        }
    }

    // Helper to normalize angle to (-PI, PI]
    let normalize_angle = |mut a: f64| -> f64 {
        while a <= -PI { a += 2.0 * PI; }
        while a > PI { a -= 2.0 * PI; }
        a
    };

    // Store calculated angles (radians)
    let mut face_angles: HashMap<usize, f64> = HashMap::new();
    let mut layer_radii: Vec<f64> = Vec::with_capacity(layers.len());

    // --- Iterative Layer Processing ---
    for (layer_idx, layer) in layers.iter().enumerate() {
        let count = layer.len() as f64;
        
        // --- Step A: Determine Angles ---
        for (i, &face_idx) in layer.iter().enumerate() {
            let angle: f64;

            if !cyclic {
                angle = 0.0; 
            } else if layer_idx == 0 {
                if count == 1.0 {
                    angle = 0.0;
                } else {
                    angle = 2.0 * PI * (i as f64) / count;
                }
            } else {
                // Optimization: Place face based on centroid of neighbors in PREVIOUS layer
                let prev_layer_idx = layer_idx - 1;
                let mut sum_x = 0.0;
                let mut sum_y = 0.0;
                let mut parent_count = 0;

                for (u, v) in &edges {
                    let neighbor = if *u == face_idx {
                        if face_to_layer_idx.get(v) == Some(&prev_layer_idx) { Some(v) } else { None }
                    } else if *v == face_idx {
                        if face_to_layer_idx.get(u) == Some(&prev_layer_idx) { Some(u) } else { None }
                    } else {
                        None
                    };

                    if let Some(n_idx) = neighbor {
                        if let Some(&theta) = face_angles.get(n_idx) {
                             sum_x += theta.cos();
                             sum_y += theta.sin();
                             parent_count += 1;
                        }
                    }
                }

                if parent_count > 0 {
                    angle = sum_y.atan2(sum_x);
                } else {
                    angle = 2.0 * PI * (i as f64) / count;
                }
            }
            
            face_angles.insert(face_idx, angle);
        }

        // --- Step B: Determine Radius ---
        let radius: f64;

        if !cyclic {
            radius = layer_idx as f64;
        } else if layer_idx == 0 {
            if count == 1.0 {
                radius = 0.0;
            } else {
                let node_separation_arc = 1.2;
                radius = ((count * node_separation_arc) / (2.0 * PI)).max(1.0);
            }
        } else {
            let prev_radius = layer_radii[layer_idx - 1];
            
            // 1. Geometric separation (Additive)
            let min_layer_dist = 2.0; 
            let mut min_r = prev_radius + min_layer_dist;

            // 2. Geometric separation (Multiplicative Growth)
            let growth_factor = 2.0; 
            let min_r_factor = prev_radius * growth_factor;
            min_r = min_r.max(min_r_factor);

            // 3. Arc separation (Circumference)
            let node_separation_arc = 1.2;
            let r_circ = (count * node_separation_arc) / (2.0 * PI);
            
            radius = min_r.max(r_circ);
        }

        layer_radii.push(radius);

        // --- Step C: Assign Final Coordinates ---
        for (i, &face_idx) in layer.iter().enumerate() {
            let x: f64;
            let y: f64;
            
            if cyclic {
                let theta = face_angles[&face_idx];
                x = radius * theta.cos();
                y = radius * theta.sin();
            } else {
                y = layer_idx as f64;
                x = (i as f64) - (count - 1.0) / 2.0;
            }
            
            coords.insert(face_idx, (x, y));

            if x > max_x { max_x = x; }
            if x < min_x { min_x = x; }
            if y > max_y { max_y = y; }
            if y < min_y { min_y = y; }
        }
    }

    // Safety fallback
    if min_x == f64::INFINITY { min_x = -1.0; max_x = 1.0; min_y = -1.0; max_y = 1.0; }

    // --- Prepare Edge Staggering (Cyclic Mode) ---
    // Maps (source, target) -> (out_angle, in_angle) in degrees
    let mut edge_draw_angles: HashMap<(usize, usize), (f64, f64)> = HashMap::new();

    if cyclic {
        // Build Adjacency Lists
        // outgoing: u -> [v1, v2...] (where v is in next layer)
        // incoming: v -> [u1, u2...] (where u is in prev layer)
        let mut outgoing: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut incoming: HashMap<usize, Vec<usize>> = HashMap::new();

        for (u, v) in &edges {
            let u_layer = face_to_layer_idx[u];
            let v_layer = face_to_layer_idx[v];

            if u_layer + 1 == v_layer {
                outgoing.entry(*u).or_default().push(*v);
                incoming.entry(*v).or_default().push(*u);
            } else if v_layer + 1 == u_layer {
                outgoing.entry(*v).or_default().push(*u);
                incoming.entry(*u).or_default().push(*v);
            }
        }

        // 1. Calculate Outgoing Angles (Staggered)
        for (u, targets) in &outgoing {
            let u_angle = face_angles[u];
            let u_is_center = coords[u].0.abs() < 0.001 && coords[u].1.abs() < 0.001;
            
            // Sort targets by relative angle to ensure lines don't cross locally
            let mut sorted_targets = targets.clone();
            sorted_targets.sort_by(|&a, &b| {
                let da = normalize_angle(face_angles[&a] - u_angle);
                let db = normalize_angle(face_angles[&b] - u_angle);
                da.partial_cmp(&db).unwrap()
            });

            let n = sorted_targets.len() as f64;
            let spread = 25.0; // Total spread in degrees
            let start_offset = if n > 1.0 { -spread / 2.0 } else { 0.0 };
            let step = if n > 1.0 { spread / (n - 1.0) } else { 0.0 };

            for (i, &v) in sorted_targets.iter().enumerate() {
                let out_deg: f64;
                if u_is_center {
                    // Center node: Exit directly towards target
                    out_deg = face_angles[&v].to_degrees();
                } else {
                    // Radial out + stagger
                    let base_deg = u_angle.to_degrees();
                    out_deg = base_deg + start_offset + (i as f64 * step);
                }
                
                // Store partially
                edge_draw_angles.insert((*u, v), (out_deg, 0.0));
            }
        }

        // 2. Calculate Incoming Angles (Staggered)
        for (v, sources) in &incoming {
            let v_angle = face_angles[v];
            
            // Sort sources by relative angle
            let mut sorted_sources = sources.clone();
            sorted_sources.sort_by(|&a, &b| {
                let da = normalize_angle(face_angles[&a] - v_angle);
                let db = normalize_angle(face_angles[&b] - v_angle);
                da.partial_cmp(&db).unwrap()
            });

            let n = sorted_sources.len() as f64;
            let spread = 25.0;
            let start_offset = if n > 1.0 { -spread / 2.0 } else { 0.0 };
            let step = if n > 1.0 { spread / (n - 1.0) } else { 0.0 };

            for (i, &u) in sorted_sources.iter().enumerate() {
                // Radial IN (pointing to center) is angle + 180
                let base_deg = v_angle.to_degrees() + 180.0;
                let in_deg = base_deg + start_offset + (i as f64 * step);

                // Update the tuple
                if let Some(entry) = edge_draw_angles.get_mut(&(u, *v)) {
                    entry.1 = in_deg;
                }
            }
        }
    }

    // --- Standard Visualization Code ---

    // Calculate max label length
    let mut max_label_len: usize = 0;
    for face_idx in coords.keys() {
        let label_len = l.faces[*face_idx].label.to_string().len();
        if label_len > max_label_len { max_label_len = label_len; }
    }

    // Calculate Scaling
    let paper_w_cm = 27.7; 
    let paper_h_cm = 17.0; 
    
    let content_w = (max_x - min_x).max(1.0) + 2.0; 
    let content_h = (max_y - min_y).max(1.0) + 2.0;
    
    let mut scale_x = paper_w_cm / content_w;
    let mut scale_y = paper_h_cm / content_h;

    if cyclic {
        let min_scale = scale_x.min(scale_y);
        scale_x = min_scale;
        scale_y = min_scale;
    }

    // Calculate Font Size
    let char_width_cm = 0.22; 
    let node_padding_cm = 0.4;
    let needed_width_cm = (max_label_len as f64 * char_width_cm) + node_padding_cm;
    let needed_height_cm = 0.8; 
    
    let available_width_cm = scale_x; 
    let available_height_cm = scale_y;
    let ratio_x = available_width_cm / needed_width_cm;
    let ratio_y = available_height_cm / needed_height_cm;

    let font_scale_factor = ratio_x.min(ratio_y).min(1.0);
    let font_size_pt = (10.0 * font_scale_factor).max(2.0);

    // Generate LaTeX
    let mut tikz = String::new();
    tikz.push_str("\\documentclass{article}\n");
    tikz.push_str("\\usepackage{tikz}\n");
    tikz.push_str("\\usepackage[landscape, a4paper, margin=10mm]{geometry}\n");
    tikz.push_str("\\begin{document}\n");
    tikz.push_str("\\thispagestyle{empty}\n");
    
    tikz.push_str(&format!("\\begin{{tikzpicture}}[xscale={:.4}, yscale={:.4}]\n", scale_x, scale_y));
    
    let inner_sep = if font_size_pt < 4.0 { 1.0 } else { 2.5 };
    
    tikz.push_str(&format!(
        "  \\tikzset{{every node/.style={{draw, fill=white, inner sep={}pt, font=\\fontsize{{{}pt}}{{{}pt}}\\selectfont}}}}\n", 
        inner_sep, font_size_pt, font_size_pt * 1.2
    ));

    // Define Coordinates
    for (face_idx, (x, y)) in &coords {
        tikz.push_str(&format!("  \\coordinate (n{}) at ({:.3},{:.3});\n", face_idx, x, y));
    }

    // Draw Edges
    if cyclic {
        // Iterate over edge_draw_angles to ensure we only draw processed edges in the correct direction
        // (edges are usually stored undirected, but here we keyed them (u,v) where u is layer k, v is k+1)
        for ((u, v), (out_deg, in_deg)) in &edge_draw_angles {
            tikz.push_str(&format!("  \\draw (n{}) to[out={:.1}, in={:.1}] (n{});\n", u, out_deg, in_deg, v));
        }
    } else {
        // Non-cyclic: Straight lines
        let edge_list: Vec<String> = edges.iter()
            .map(|(a, b)| format!("n{}/n{}", a, b))
            .collect();
        
        if !edge_list.is_empty() {
            tikz.push_str(&format!("  \\foreach \\u/\\v in {{{}}} {{\n", edge_list.join(",")));
            tikz.push_str("    \\draw (\\u) -- (\\v);\n");
            tikz.push_str("  }\n");
        }
    }

    // Draw Nodes
    let mut node_defs = Vec::new();
    for face_idx in coords.keys() {
        let label = &l.faces[*face_idx].label;
        let safe_label = label.to_string().replace("_", "\\_"); 
        node_defs.push(format!("n{}/{}", face_idx, safe_label));
    }

    if !node_defs.is_empty() {
        tikz.push_str(&format!("  \\foreach \\ref/\\lbl in {{{}}} {{\n", node_defs.join(",")));
        tikz.push_str("    \\node at (\\ref) {\\lbl};\n");
        tikz.push_str("  }\n");
    }

    tikz.push_str("\\end{tikzpicture}\n");
    tikz.push_str("\\end{document}\n");

    // Output and Compile
    let filename = if cyclic { "strip_visualization_cyclic" } else { "strip_visualization" };
    let tex_filename = format!("{}.tex", filename);
    let _ = File::create(&tex_filename).and_then(|mut f| f.write_all(tikz.as_bytes()));

    let output = Command::new("pdflatex")
        .arg("-interaction=nonstopmode")
        .arg(&tex_filename)
        .output()
        .expect("Failed to execute pdflatex");

    if !output.status.success() {
        eprintln!("LaTeX error:\n{}", String::from_utf8_lossy(&output.stdout));
        return;
    }

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
    
    let _ = command.spawn();
}