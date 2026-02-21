use eframe::egui;
use std::collections::HashMap;
use std::fs;

// Assumed constants from your crate
use crate::MAX_FACES;
use crate::MAX_UP_DOWN;
use crate::MAX_LEVELS;

// Assumed Imports from your crate
use crate::lattice::*;
use crate::rhombic::*;
use crate::plotting::*;

// Internal representation of a node in the GUI editor
#[derive(Clone)]
struct GuiNode {
    id: usize,          // Unique ID for the node
    label: String,      // Display label
    pos: egui::Pos2,    // Position in World Space
    dragged: bool,      // State tracking for drag operations
}

// The application state
pub struct LatticeApp {
    // Graph Data
    nodes: Vec<GuiNode>,
    edges: Vec<(usize, usize)>, // Adjacency list (from_id, to_id), representing from < to

    // UI State
    node_counter: usize,        // To assign unique IDs
    new_node_name: String,      // Input buffer for new node dialog
    show_new_node_dialog: bool, // Toggle for the single add dialog
    show_multi_add_dialog: bool,// Toggle for the multi add dialog
    edge_start_node: Option<usize>, // Stores the ID of the first node clicked when creating an edge
    msg_log: String,            // Displays results (Count, Existence)
    
    // Grid Generation State
    grid_gen_input: String,

    // Viewport State (Pan & Zoom)
    view_offset: egui::Vec2,
    view_scale: f32,

    // Algorithm Settings
    cyclic: bool,               // Toggle for cyclic vs linear strips

    // Visualization State
    active_strip: Option<Vec<Vec<u8>>>,
    active_strip_edges: Option<Vec<(usize, usize)>>,
    
    num_strips_displayed: usize // track the number of strips displayed already
}

impl LatticeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            node_counter: 0,
            new_node_name: String::new(),
            show_new_node_dialog: false,
            show_multi_add_dialog: false,
            edge_start_node: None,
            msg_log: String::from("Welcome. Create vertices and edges."),
            grid_gen_input: String::new(), // Initializing new field
            view_offset: egui::Vec2::ZERO,
            view_scale: 1.0,
            cyclic: false,
            active_strip: None,
            active_strip_edges: None,
            num_strips_displayed: 0
        }
    }

    // --- Helper Functions ---

    fn reset(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.node_counter = 0;
        self.new_node_name.clear();
        self.show_new_node_dialog = false;
        self.show_multi_add_dialog = false;
        self.edge_start_node = None;
        self.msg_log = String::from("System Reset.");
        self.active_strip = None;
        self.active_strip_edges = None;
        self.view_offset = egui::Vec2::ZERO;
        self.view_scale = 1.0;
        // Keep grid_gen_input as is for convenience
    }

    // Transform World Coordinate -> Screen Coordinate
    fn to_screen(&self, world_pos: egui::Pos2) -> egui::Pos2 {
        world_pos * self.view_scale + self.view_offset
    }

    // Transform Screen Coordinate -> World Coordinate
    fn to_world(&self, screen_pos: egui::Pos2) -> egui::Pos2 {
        (screen_pos - self.view_offset) / self.view_scale
    }

    fn to_lattice(&self) -> Lattice {
        let mut faces: Vec<Face> = Vec::new();
        let dims = self.compute_dimensions();

        let id_to_index: HashMap<usize, usize> = self.nodes.iter()
            .enumerate()
            .map(|(i, node)| (node.id, i))
            .collect();

        for node in &self.nodes {
            let mut upset = [255u8; MAX_UP_DOWN];
            let mut downset = [255u8; MAX_UP_DOWN];
            let mut u_count = 0;
            let mut d_count = 0;

            for (from, to) in &self.edges {
                if *from == node.id {
                    if let Some(&to_idx) = id_to_index.get(to) {
                        if u_count < 50 {
                            upset[u_count] = to_idx as u8;
                            u_count += 1;
                        }
                    }
                }
                if *to == node.id {
                    if let Some(&from_idx) = id_to_index.get(from) {
                        if d_count < 50 {
                            downset[d_count] = from_idx as u8;
                            d_count += 1;
                        }
                    }
                }
            }

            faces.push(Face {
                label: node.label.clone(),
                dim: *dims.get(&node.id).unwrap_or(&0),
                upset,
                downset,
            });
        }

        let max_dim = faces.iter().map(|f| f.dim).max().unwrap_or(0);
        let mut levels = [[255u8; MAX_UP_DOWN]; MAX_LEVELS];
        let mut count_per_dim = [0u8; MAX_LEVELS];

        for (i, face) in faces.iter().enumerate() {
            let d = face.dim as usize;
            if d < MAX_LEVELS && (count_per_dim[d] as usize) < MAX_UP_DOWN {
                levels[d][count_per_dim[d] as usize] = i as u8;
                count_per_dim[d] += 1;
            }
        }

        let mut bridges = [[255u8; MAX_FACES]; MAX_FACES];
        let mut tunnels = [[255u8; MAX_FACES]; MAX_FACES];
        for i in 0..faces.len() {
            for j in 0..faces.len() {
                if i >= j { continue };

                let mut bridge_idx = None;
                let mut tunnel_idx = None;
                for (k, face) in faces.iter().enumerate() {
                    let i_u8 = i as u8;
                    let j_u8 = j as u8;

                    let covers_i = face.downset.iter().take_while(|&&x| x != 255).any(|&x| x == i_u8);
                    let covers_j = face.downset.iter().take_while(|&&x| x != 255).any(|&x| x == j_u8);

                    let is_covered_by_i = face.upset.iter().take_while(|&&x| x != 255).any(|&x| x == i_u8);
                    let is_covered_by_j = face.upset.iter().take_while(|&&x| x != 255).any(|&x| x == j_u8);

                    if covers_i && covers_j {
                        bridge_idx = Some(k as u8);
                        break;
                    }

                    if is_covered_by_i && is_covered_by_j {
                        tunnel_idx = Some(k as u8);
                        break;
                    }
                }

                if let Some(b) = bridge_idx {
                    if i < MAX_FACES && j < MAX_FACES {
                        bridges[i][j] = b;
                        bridges[j][i] = b;
                    }
                }
                if let Some(t) = tunnel_idx {
                    if i < MAX_FACES && j < MAX_FACES {
                        tunnels[i][j] = t;
                        tunnels[j][i] = t;
                    }
                }
            }
        }

        Lattice {
            faces,
            levels,
            bridges,
            tunnels,
            dim: max_dim,
        }
    }

    fn compute_dimensions(&self) -> HashMap<usize, u8> {
        let mut dims: HashMap<usize, u8> = HashMap::new();

        for node in &self.nodes {
            dims.insert(node.id, 0);
        }

        for _ in 0..self.nodes.len() {
            let mut changed = false;
            for (from, to) in &self.edges {
                let d_from = *dims.get(from).unwrap_or(&0);
                let d_to = *dims.get(to).unwrap_or(&0);

                if d_from >= d_to {
                    dims.insert(*to, d_from + 1);
                    changed = true;
                }
            }
            if !changed { break; }
        }

        for val in dims.values_mut() {
            if *val > 29 { *val = 29; }
        }

        dims
    }

    fn apply_strip_layout(&mut self, strip: &Vec<Vec<u8>>) {
        let center = egui::pos2(500.0, 400.0);

        for (layer_idx, layer) in strip.iter().enumerate() {
            let count = layer.len() as f32;

            for (i, &face_idx) in layer.iter().enumerate() {
                if face_idx as usize >= self.nodes.len() { continue; }

                let node_id = self.nodes[face_idx as usize].id;
                let new_pos;

                
                let x_spacing = 80.0;
                let y_spacing = 100.0;
                
                let x = center.x + ((i as f32) - (count - 1.0)/2.0) * x_spacing;
                let y = 600.0 - (layer_idx as f32 * y_spacing);
                new_pos = egui::pos2(x, y);

                if let Some(node) = self.nodes.iter_mut().find(|n| n.id == node_id) {
                    node.pos = new_pos;
                }
            }
        }
    }
}

impl eframe::App for LatticeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        // --- LEFT SIDEBAR (Controls) ---
        egui::SidePanel::left("controls").show(ctx, |ui| {
            ui.heading("Lattice Builder");
            ui.separator();

            // 1. Add Vertex Button
            if ui.button("Add Nodes").clicked() {
                self.num_strips_displayed = 0; // reset strip displayer count when generating new grid
                self.show_multi_add_dialog = true;
                self.show_new_node_dialog = false;
                self.new_node_name.clear();
            }

            ui.separator();

            // 2. Grid Generator Logic
            ui.label("Grid Generator:");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.grid_gen_input).on_hover_text("e.g. 211");
                if ui.button("Create").clicked() {
                    self.num_strips_displayed = 0; // reset strip displayer count when generating new grid
                    // 1. Parse Input "211" -> [2, 1, 1]
                    let dims: Vec<u32> = self.grid_gen_input.chars()
                        .filter_map(|c| c.to_digit(10))
                        .collect();
                    
                    if !dims.is_empty() {
                        self.reset();
                        self.msg_log = format!("Generating grid for dims: {:?}", dims);

                        // 2. Generate Nodes
                        // Recursive closure to generate strings
                        let mut results = Vec::new();
                        fn generate_recursive(
                            idx: usize, 
                            current_str: String, 
                            current_sum: u32,
                            dims: &[u32], 
                            results: &mut Vec<(String, u32)> // Label, Sum
                        ) {
                            if idx == dims.len() {
                                results.push((current_str, current_sum));
                                return;
                            }
                            for i in 0..=dims[idx] {
                                let mut next_str = current_str.clone();
                                next_str.push_str(&i.to_string());
                                generate_recursive(idx + 1, next_str, current_sum + i, dims, results);
                            }
                        }

                        generate_recursive(0, String::new(), 0, &dims, &mut results);

                        // Group by rank (sum) for layout
                        let mut rank_groups: HashMap<u32, Vec<usize>> = HashMap::new();

                        for (i, (label, sum)) in results.iter().enumerate() {
                            self.nodes.push(GuiNode {
                                id: i,
                                label: label.clone(),
                                pos: egui::Pos2::ZERO, // calculated below
                                dragged: false,
                            });
                            self.node_counter += 1;
                            rank_groups.entry(*sum).or_default().push(i);
                        }

                        // 3. Layout (Center X, Y based on rank)
                        let center_x = 500.0;
                        let start_y = 700.0;
                        let y_step = 80.0;
                        let x_step = 80.0;

                        for (rank, indices) in rank_groups {
                            let row_count = indices.len() as f32;
                            let row_width = (row_count - 1.0) * x_step;
                            let y = start_y - (rank as f32 * y_step);

                            for (k, &node_idx) in indices.iter().enumerate() {
                                let x = center_x - (row_width / 2.0) + (k as f32 * x_step);
                                self.nodes[node_idx].pos = egui::pos2(x, y);
                            }
                        }

                        // 4. Generate Edges for Grid
                        // A covers B if B = A + e_i
                        for i in 0..self.nodes.len() {
                            for j in 0..self.nodes.len() {
                                if i == j { continue; }
                                
                                let s1 = &self.nodes[i].label; // Lower (potential)
                                let s2 = &self.nodes[j].label; // Upper (potential)

                                // Check grid covering relation specifically
                                let mut diff_idx = None;
                                let mut is_cover = true;

                                let chars1: Vec<char> = s1.chars().collect();
                                let chars2: Vec<char> = s2.chars().collect();

                                if chars1.len() != chars2.len() { continue; }

                                for k in 0..chars1.len() {
                                    if chars1[k] != chars2[k] {
                                        if diff_idx.is_none() {
                                            // Must be exactly one greater
                                            let d1 = chars1[k].to_digit(10).unwrap() as i32;
                                            let d2 = chars2[k].to_digit(10).unwrap() as i32;
                                            if d2 == d1 + 1 {
                                                diff_idx = Some(k);
                                            } else {
                                                is_cover = false;
                                                break;
                                            }
                                        } else {
                                            // More than one difference
                                            is_cover = false;
                                            break;
                                        }
                                    }
                                }

                                if is_cover && diff_idx.is_some() {
                                    self.edges.push((self.nodes[i].id, self.nodes[j].id));
                                }
                            }
                        }
                    }
                }
            });

            ui.separator();

            // 3. Infer Button (UPDATED)
            if ui.button("Infer Edges").clicked() {
                self.num_strips_displayed = 0; // reset strip displayer count when generating new grid
                let mut new_edges_count = 0;
                let mut new_relations = Vec::new();

                for i in 0..self.nodes.len() {
                    for j in 0..self.nodes.len() {
                        if i == j { continue; }

                        let label_a = &self.nodes[i].label; // Potential lower
                        let label_b = &self.nodes[j].label; // Potential upper

                        // UPDATED LOGIC:
                        // 1. Same Length
                        // 2. All Digits
                        // 3. Sum(B) = Sum(A) + 1
                        // 4. Differ in exactly one position
                        
                        let is_cover = if label_a.len() == label_b.len() {
                            // Check if only digits
                            let a_is_digit = label_a.chars().all(|c| c.is_ascii_digit());
                            let b_is_digit = label_b.chars().all(|c| c.is_ascii_digit());

                            if a_is_digit && b_is_digit {
                                let sum_a: i32 = label_a.chars().map(|c| c.to_digit(10).unwrap() as i32).sum();
                                let sum_b: i32 = label_b.chars().map(|c| c.to_digit(10).unwrap() as i32).sum();

                                if sum_b == sum_a + 1 {
                                    // Sum condition met, now check position difference count
                                    let diff_count = label_a.chars().zip(label_b.chars())
                                        .filter(|(ca, cb)| ca != cb)
                                        .count();
                                    
                                    diff_count == 1
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if is_cover {
                            new_relations.push((self.nodes[i].id, self.nodes[j].id));
                        }
                    }
                }

                for (from_id, to_id) in new_relations {
                    if !self.edges.contains(&(from_id, to_id)) {
                        self.edges.push((from_id, to_id));
                        new_edges_count += 1;
                    }
                }

                self.msg_log = format!("Inferred {} new relations.", new_edges_count);
            }

            ui.separator();

            // 4. To Distributive Button
            if ui.button("To Distributed").clicked() {
                self.num_strips_displayed = 0; // reset strip displayer count when generating new grid
                let lattice = self.to_lattice();
                let num_faces = lattice.faces.len();

                // Safety guard for computational complexity
                if num_faces > 16 {
                    self.msg_log = format!("Lattice too large ({}) for interactive distribution.", num_faces);
                } else {
                    let mut ideals: Vec<(u64, Vec<String>)> = Vec::new();

                    // Generate all ideals
                    let max_mask = 1u64 << num_faces;
                    for mask in 0..max_mask {
                        let mut is_ideal = true;
                        let mut current_subset_labels = Vec::new();

                        for i in 0..num_faces {
                            if (mask >> i) & 1 == 1 {
                                current_subset_labels.push(lattice.faces[i].label.clone());
                                let downset = &lattice.faces[i].downset;
                                for &d in downset.iter() {
                                    if d == 255 { break; }
                                    if (mask >> d) & 1 == 0 {
                                        is_ideal = false;
                                        break;
                                    }
                                }
                            }
                            if !is_ideal { break; }
                        }

                        if is_ideal {
                            current_subset_labels.sort();
                            ideals.push((mask, current_subset_labels));
                        }
                    }

                    // Reset and populate
                    self.reset();
                    self.msg_log = format!("Generated Distributive Lattice J(L) with {} elements.", ideals.len());

                    let mut size_groups: HashMap<u32, Vec<usize>> = HashMap::new();

                    for (idx, (mask, labels)) in ideals.iter().enumerate() {
                        let size = mask.count_ones();
                        size_groups.entry(size).or_default().push(idx);

                        let label_str = if labels.is_empty() {
                            "?".to_string()
                        } else {
                            format!("{{{}}}", labels.join(","))
                        };

                        self.nodes.push(GuiNode {
                            id: idx,
                            label: label_str,
                            pos: egui::Pos2::ZERO,
                            dragged: false,
                        });
                        self.node_counter += 1;
                    }

                    // Layout
                    let center_x = 500.0;
                    let start_y = 650.0;
                    let layer_height = 80.0;

                    for (size, indices) in &size_groups {
                        let count = indices.len() as f32;
                        let width_spacing = 90.0;
                        let row_width = (count - 1.0) * width_spacing;
                        let y = start_y - (*size as f32 * layer_height);

                        for (i, &node_idx) in indices.iter().enumerate() {
                            let x = center_x - (row_width / 2.0) + (i as f32 * width_spacing);
                            self.nodes[node_idx].pos = egui::pos2(x, y);
                        }
                    }

                    // Edges
                    for i in 0..ideals.len() {
                        let (mask_a, _) = ideals[i];
                        let size_a = mask_a.count_ones();

                        for j in 0..ideals.len() {
                            if i == j { continue; }
                            let (mask_b, _) = ideals[j];
                            let size_b = mask_b.count_ones();

                            if size_b == size_a + 1 {
                                if (mask_a & mask_b) == mask_a {
                                    self.edges.push((i, j));
                                }
                            }
                        }
                    }
                }
            }

            ui.separator();
            ui.checkbox(&mut self.cyclic, "Cyclic Mode");

            ui.separator();
            ui.label("Algorithms:");

            let lattice = self.to_lattice();

            // 5. Existence Check
            if ui.button("Existence").clicked() {
                self.num_strips_displayed = 0; // reset strip displayer count when generating new grid
                let mut found = false;
                for ham in lattice.ham_paths(self.cyclic) {
                    if rhombic_strip_exists(&ham, 0, &lattice, lattice.dim as usize, self.cyclic) {
                        self.msg_log = "Result: A rhombic strip EXISTS!".to_string();
                        found = true;
                        break;
                    }
                }
                if !found {
                    self.msg_log = "Result: No rhombic strip found.".to_string();
                }
                self.active_strip = None;
                self.active_strip_edges = None;
            }

            // 6. Count Strips
            if ui.button("Count").clicked() {
                self.num_strips_displayed = 0; // reset strip displayer count when generating new grid
                let mut count = 0;
                for ham in lattice.ham_paths(self.cyclic) {
                    let new_strips = rhombic_strips_dfs_lazy(vec![ham], &lattice, lattice.dim as usize, self.cyclic);
                    count += new_strips.len();
                }
                self.msg_log = format!("Number of rhombic strips found: {}", count);
                self.active_strip = None;
                self.active_strip_edges = None;
            }

            // 7. Show Strip
            if ui.button("Show").clicked() {
                // the logic to cycle through the found strips is very crude
                // it recomputes the strips each time
                // really, there should be a lazy iterator that the show button advances
                // a job for another time
                let mut found_strip = None;
                let mut num_found = 0;
                let num_needed = self.num_strips_displayed + 1; // Show one more strip each time

                for ham in lattice.ham_paths(self.cyclic) {
                    // if we have not yet displayed a strip, we use the faster find_first function to display the first one
                    // after that, we will use the crude compute all method
                    if self.num_strips_displayed == 0 {
                        if let Some(strip) = find_first_rhombic_strip_lazy(vec![ham], &lattice, lattice.dim as usize, self.cyclic) {
                            found_strip = Some(strip);
                            self.num_strips_displayed += 1; // Increment for next time
                            break;
                        } else {
                            continue; // No strip for this ham, try next
                        }
                    }
                    let strips = rhombic_strips_dfs_lazy(vec![ham], &lattice, lattice.dim as usize, self.cyclic);
                    
                    if !strips.is_empty() {
                        if num_found + strips.len() >= num_needed {
                            found_strip = Some(strips[num_needed - num_found - 1].clone());
                            self.num_strips_displayed += 1; // Increment for next time
                            break;
                        }
                        num_found += strips.len();
                    }
                }

                if let Some(strip) = found_strip {
                    self.msg_log = format!("Displaying found strip number {}.", self.num_strips_displayed);
                    self.apply_strip_layout(&strip);

                    let strip_usize: Vec<Vec<usize>> = strip.iter()
                        .map(|layer| layer.iter().map(|&x| x as usize).collect())
                        .collect();

                    // cyclic edges not used atm, might draw them eventually somehow
                    let (edge_indices, _cyclic_edges_indices) = edges_strip(&strip_usize, &lattice, self.cyclic);

                    let mapped_edges: Vec<(usize, usize)> = edge_indices.into_iter()
                        .filter_map(|(u_idx, v_idx)| {
                            if u_idx < self.nodes.len() && v_idx < self.nodes.len() {
                                Some((self.nodes[u_idx].id, self.nodes[v_idx].id))
                            } else {
                                None
                            }
                        })
                        .collect();

                    self.active_strip_edges = Some(mapped_edges);
                    self.active_strip = Some(strip);
                } else {
                    self.msg_log = "No strip found to display.".to_string();
                    self.active_strip = None;
                    self.active_strip_edges = None;
                    self.num_strips_displayed = 0; // reset so we can cycle through strips again if desired
                }
            }

            if ui.button("Reset View").clicked() {
                self.active_strip = None;
                self.active_strip_edges = None;
                self.msg_log = "View reset. You can edit nodes now.".to_string();
            }

            ui.separator();

            // 8. Export TeX
            if ui.button("Export TeX").clicked() {
                let visible_node_ids: Vec<usize> = if let Some(strip) = &self.active_strip {
                    strip.iter().flatten().map(|&x| {
                          if (x as usize) < self.nodes.len() { self.nodes[x as usize].id } else { usize::MAX }
                    }).collect()
                } else {
                    self.nodes.iter().map(|n| n.id).collect()
                };

                let edges_to_export = if let Some(ref specific_edges) = self.active_strip_edges {
                    specific_edges
                } else {
                    &self.edges
                };

                let mut tex = String::new();
                tex.push_str("\\documentclass[tikz, border=1cm]{standalone}\n");
                tex.push_str("\\begin{document}\n");
                tex.push_str("\\begin{tikzpicture}[y=-1cm]\n\n");

                let scale = 0.02;

                tex.push_str("% Coordinates\n");
                for node in &self.nodes {
                    if !visible_node_ids.contains(&node.id) { continue; }
                    let safe_label = node.label.replace("{", "").replace("}", "").replace(",", "_").replace("?", "empty");
                    // Export using World Position (preserves relative layout regardless of zoom)
                    tex.push_str(&format!("\\coordinate ({}) at ({:.2}, {:.2});\n",
                        safe_label,
                        node.pos.x * scale,
                        node.pos.y * scale
                    ));
                }
                tex.push_str("\n");

                tex.push_str("% Edges\n");
                tex.push_str("\\foreach \\a/\\b in {");

                let mut edge_strings = Vec::new();
                for (from, to) in edges_to_export {
                    if !visible_node_ids.contains(from) || !visible_node_ids.contains(to) { continue; }

                    let l1 = self.nodes.iter().find(|n| n.id == *from).unwrap().label.replace("{", "").replace("}", "").replace(",", "_").replace("?", "empty");
                    let l2 = self.nodes.iter().find(|n| n.id == *to).unwrap().label.replace("{", "").replace("}", "").replace(",", "_").replace("?", "empty");
                    edge_strings.push(format!("{}/{}", l1, l2));
                }
                tex.push_str(&edge_strings.join(", "));

                tex.push_str("} {\n    \\draw \\a -- \\b;\n}\n\n");

                tex.push_str("% Nodes\n");
                tex.push_str("\\foreach \\v/\\l in {");

                let mut label_strings = Vec::new();
                for node in &self.nodes {
                    if !visible_node_ids.contains(&node.id) { continue; }
                    let safe_label = node.label.replace("{", "").replace("}", "").replace(",", "_").replace("?", "empty");
                    label_strings.push(format!("{}/{{{}}}", safe_label, node.label));
                }
                tex.push_str(&label_strings.join(", "));

                tex.push_str("} {\n    \\node[draw, circle, fill=white, inner sep=2pt] at (\\v) {\\footnotesize \\l};\n}\n");

                tex.push_str("\\end{tikzpicture}\n");
                tex.push_str("\\end{document}\n");

                match fs::write("lattice_output.tex", tex) {
                    Ok(_) => self.msg_log = "Exported to lattice_output.tex".to_string(),
                    Err(e) => self.msg_log = format!("Export failed: {}", e),
                }
            }

            // 9. Restart Button
            if ui.button("Restart").clicked() {
                self.num_strips_displayed = 0; // reset strip displayer count when generating new grid
                self.reset();
            }

            ui.separator();
            ui.heading("Log:");
            ui.label(&self.msg_log);

            ui.add_space(20.0);
            ui.small("Controls:\n- Drag background to Pan\n- Mouse Wheel to Zoom");
        });

        // --- FLOATING WINDOWS ---
        if self.show_multi_add_dialog {
            egui::Window::new("Add Nodes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -100.0))
                .show(ctx, |ui| {
                    ui.label("Type label and press Enter to add.");
                    let response = ui.text_edit_singleline(&mut self.new_node_name);
                    response.request_focus();

                    let mut add_pressed = false;

                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && response.has_focus() {
                        add_pressed = true;
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            add_pressed = true;
                        }
                        if ui.button("Done").clicked() {
                            self.show_multi_add_dialog = false;
                        }
                    });

                    if add_pressed {
                        let id = self.node_counter;
                        self.nodes.push(GuiNode {
                            id,
                            label: if self.new_node_name.is_empty() { format!("{}", id) } else { self.new_node_name.clone() },
                            // Place new nodes relative to current center view
                            pos: egui::pos2(400.0 + (id as f32 * 20.0), 200.0),
                            dragged: false,
                        });
                        self.node_counter += 1;
                        self.new_node_name.clear();
                        self.msg_log = format!("Added node {}", id);
                    }
                });
        }


        // --- CENTRAL PANEL (Canvas) ---
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(egui::Color32::WHITE))
            .show(ctx, |ui| {
            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());

            // --- INPUT HANDLING: PAN & ZOOM ---

            // 1. Zooming (Mouse Wheel)
            if response.hovered() {
                 let scroll_delta = ui.input(|i| i.raw_scroll_delta);
                 if scroll_delta.y != 0.0 {
                     let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                     let new_zoom = self.view_scale * zoom_factor;

                     // Zoom towards mouse pointer
                     if let Some(mouse_pos) = response.hover_pos() {
                         let world_mouse = self.to_world(mouse_pos);
                         // new_offset = mouse - world * new_zoom
                         self.view_offset = mouse_pos.to_vec2() - (world_mouse.to_vec2() * new_zoom);
                     }
                     self.view_scale = new_zoom;
                 }
            }

            // 2. Determine if nodes are being dragged to block Pan
            let mut any_node_dragged = false;

            // Determine visible nodes and edges
            let visible_node_ids: Vec<usize> = if let Some(strip) = &self.active_strip {
                strip.iter().flatten().map(|&x| {
                      if (x as usize) < self.nodes.len() { self.nodes[x as usize].id } else { usize::MAX }
                }).collect()
            } else {
                self.nodes.iter().map(|n| n.id).collect()
            };

            // Draw Edges (transformed)
            let stroke_default = egui::Stroke::new(2.0, egui::Color32::BLACK);
            let edges_to_draw = if let Some(ref specific_edges) = self.active_strip_edges {
                specific_edges
            } else {
                &self.edges
            };

            for (from, to) in edges_to_draw {
                if !visible_node_ids.contains(from) || !visible_node_ids.contains(to) { continue; }

                let p1_opt = self.nodes.iter().find(|n| n.id == *from).map(|n| n.pos);
                let p2_opt = self.nodes.iter().find(|n| n.id == *to).map(|n| n.pos);

                if let (Some(p1), Some(p2)) = (p1_opt, p2_opt) {
                      painter.line_segment([self.to_screen(p1), self.to_screen(p2)], stroke_default);
                }
            }

            // Creation Preview Line
            if let Some(start_id) = self.edge_start_node {
                if let Some(start_node) = self.nodes.iter().find(|n| n.id == start_id) {
                    if let Some(pointer_pos) = response.hover_pos() {
                        painter.line_segment(
                            [self.to_screen(start_node.pos), pointer_pos],
                            egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(255, 0, 0, 128))
                        );
                    }
                }
            }

            // Draw Nodes & Handle Interactions
            for i in 0..self.nodes.len() {
                let node_id = self.nodes[i].id;
                if !visible_node_ids.contains(&node_id) { continue; }

                let node_pos_world = self.nodes[i].pos;
                let node_pos_screen = self.to_screen(node_pos_world);
                let label = self.nodes[i].label.clone();

                // Interaction Area (in Screen Space)
                let node_radius = 20.0;
                let node_rect = egui::Rect::from_center_size(node_pos_screen, egui::vec2(node_radius*2.0, node_radius*2.0));
                let node_response = ui.interact(node_rect, egui::Id::new(node_id), egui::Sense::click_and_drag());

                if node_response.dragged() {
                    // Update World Position: Delta screen / Scale
                    self.nodes[i].pos += node_response.drag_delta() / self.view_scale;
                    self.nodes[i].dragged = true;
                    any_node_dragged = true;
                } else {
                    self.nodes[i].dragged = false;
                }

                // Node Click Logic
                if node_response.clicked() && self.active_strip.is_none() {
                    match self.edge_start_node {
                        None => {
                            self.edge_start_node = Some(node_id);
                            self.msg_log = format!("Selected {}. Click target...", label);
                        }
                        Some(start_id) => {
                            if start_id != node_id {
                                if !self.edges.contains(&(start_id, node_id)) && !self.edges.contains(&(node_id, start_id)) {
                                    self.edges.push((start_id, node_id));
                                    self.msg_log = format!("Relation added: {} < {}",
                                        self.nodes.iter().find(|n| n.id == start_id).unwrap().label,
                                        label);
                                }
                            }
                            self.edge_start_node = None;
                        }
                    }
                }

                let fill_color = if self.edge_start_node == Some(node_id) {
                    egui::Color32::LIGHT_RED
                } else {
                    egui::Color32::WHITE
                };

                painter.circle(node_pos_screen, node_radius, fill_color, egui::Stroke::new(1.0, egui::Color32::BLACK));
                painter.text(node_pos_screen, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(14.0), egui::Color32::BLACK);
            }

            // 3. Panning (Background Drag)
            // Only pan if we dragged the background AND we didn't drag any node
            if response.dragged_by(egui::PointerButton::Primary) && !any_node_dragged {
                self.view_offset += response.drag_delta();
            }

            // Right click cancel
            if response.clicked_by(egui::PointerButton::Secondary) {
                self.edge_start_node = None;
                self.msg_log = "Edge creation cancelled.".to_string();
            }
        });
    }
}

pub fn interactive() {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Lattice Interactive Mode",
        options,
        Box::new(|cc| Ok(Box::new(LatticeApp::new(cc)))),
    );
}