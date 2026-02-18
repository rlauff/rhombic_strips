use eframe::egui;
use std::collections::HashMap;
use std::f32::consts::PI;

// Imports as requested
use crate::lattice::*;
use crate::rhombic::*;
use crate::plotting::*;

// Internal representation of a node in the GUI editor
#[derive(Clone)]
struct GuiNode {
    id: usize,          // Unique ID for the node
    label: String,      // Display label
    pos: egui::Pos2,    // Position on the canvas
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
    
    // Algorithm Settings
    cyclic: bool,               // Toggle for cyclic vs linear strips
    
    // Visualization State
    // If Some, we are in "Show" mode and display this specific strip.
    // The strip is a list of layers, where each layer is a list of face indices.
    active_strip: Option<Vec<Vec<u8>>>,
    // Stores the edges specifically returned by edges_strip for the active view
    active_strip_edges: Option<Vec<(usize, usize)>>,
}

impl LatticeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize with a clean state
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            node_counter: 0,
            new_node_name: String::new(),
            show_new_node_dialog: false,
            show_multi_add_dialog: false,
            edge_start_node: None,
            msg_log: String::from("Welcome. Create vertices and edges."),
            cyclic: false,
            active_strip: None,
            active_strip_edges: None,
        }
    }

    // --- Helper Functions ---

    /// Converts the current GUI graph state into your `Lattice` struct.
    /// This allows us to feed the visual graph into your existing algorithms.
    fn to_lattice(&self) -> Lattice {
        let mut faces: Vec<Face> = Vec::new();
        
        // We need to determine the dimension (level) of each face.
        // In a lattice, this is usually the length of the longest chain to the bottom.
        let dims = self.compute_dimensions();

        // Create Face objects
        // We map GUI node IDs to Face indices based on the order of `self.nodes`.
        // A map is created to look up the array index for a given Node ID.
        let id_to_index: HashMap<usize, usize> = self.nodes.iter()
            .enumerate()
            .map(|(i, node)| (node.id, i))
            .collect();

        for node in &self.nodes {
            let mut upset = [255u8; 50];
            let mut downset = [255u8; 50];
            let mut u_count = 0;
            let mut d_count = 0;

            // Populate upset (edges pointing AWAY from this node: node < other)
            for (from, to) in &self.edges {
                if *from == node.id {
                    if let Some(&to_idx) = id_to_index.get(to) {
                        if u_count < 50 {
                            upset[u_count] = to_idx as u8;
                            u_count += 1;
                        }
                    }
                }
                // Populate downset (edges pointing TOWARDS this node: other < node)
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

        // Generate Levels (buckets of faces by dimension)
        let max_dim = faces.iter().map(|f| f.dim).max().unwrap_or(0);
        let mut levels = [[255u8; 50]; 30];
        let mut count_per_dim = [0u8; 30];

        for (i, face) in faces.iter().enumerate() {
            let d = face.dim as usize;
            if d < 30 && (count_per_dim[d] as usize) < 50 {
                levels[d][count_per_dim[d] as usize] = i as u8;
                count_per_dim[d] += 1;
            }
        }

        // Generate Bridges
        // We replicate the bridge logic here to avoid dependency issues if `bridge()` isn't pub.
        // A bridge between face i and j exists if there is a face k in downset(i) AND downset(j).
        let mut bridges = [255u8; 100*100]; 
        for i in 0..faces.len() {
            for j in 0..faces.len() {
                if i >= j { continue };
                
                let mut bridge_idx = None;
                for (k, face) in faces.iter().enumerate() {
                    let i_u8 = i as u8;
                    let j_u8 = j as u8;
                    
                    // Check if k is in downset of i AND downset of j
                    let covers_i = face.downset.iter().take_while(|&&x| x != 255).any(|&x| x == i_u8);
                    let covers_j = face.downset.iter().take_while(|&&x| x != 255).any(|&x| x == j_u8);

                    if covers_i && covers_j {
                        bridge_idx = Some(k as u8);
                        break; 
                    }
                }

                if let Some(b) = bridge_idx {
                    if i * 100 + j < bridges.len() {
                        bridges[i * 100 + j] = b;
                        bridges[j * 100 + i] = b;
                    }
                }
            }
        }

        Lattice {
            faces,
            levels,
            bridges,
            dim: max_dim,
        }
    }

    /// Computes the 'dimension' (vertical level) of each node using Longest Path in DAG.
    fn compute_dimensions(&self) -> HashMap<usize, u8> {
        let mut dims: HashMap<usize, u8> = HashMap::new();
        
        // Initialize all nodes to 0
        for node in &self.nodes {
            dims.insert(node.id, 0);
        }

        // Relax edges repeatedly (Bellman-Ford style). 
        // Max N iterations where N is node count ensures we find longest paths.
        for _ in 0..self.nodes.len() {
            let mut changed = false;
            for (from, to) in &self.edges {
                let d_from = *dims.get(from).unwrap_or(&0);
                let d_to = *dims.get(to).unwrap_or(&0);
                
                // If source level >= target level, push target up
                if d_from >= d_to {
                    dims.insert(*to, d_from + 1);
                    changed = true;
                }
            }
            if !changed { break; }
        }
        
        // Clamp to avoid array overflow in Lattice struct (max 30)
        for val in dims.values_mut() {
            if *val > 29 { *val = 29; }
        }
        
        dims
    }

    /// Re-arranges nodes on the canvas according to the calculated Rhombic Strip.
    /// Mimics the logic in `show_strip` (polar for cyclic, layered for linear).
    fn apply_strip_layout(&mut self, strip: &Vec<Vec<u8>>) {
        let center = egui::pos2(500.0, 400.0);
        
        for (layer_idx, layer) in strip.iter().enumerate() {
            let count = layer.len() as f32;
            
            // Determine Radius / Spacing
            let radius = if self.cyclic {
                 if layer_idx == 0 { 
                     if count <= 1.0 { 0.0 } else { (count * 1.2 / (2.0 * PI)).max(1.0) * 50.0 } 
                 } else {
                     100.0 + (layer_idx as f32 * 60.0) 
                 }
            } else {
                 0.0 
            };

            for (i, &face_idx) in layer.iter().enumerate() {
                // Map the lattice face index back to our GuiNode ID.
                // Since to_lattice preserves order: face_idx == index in self.nodes
                if face_idx as usize >= self.nodes.len() { continue; }
                
                let node_id = self.nodes[face_idx as usize].id;
                let new_pos;

                if self.cyclic {
                    // Cyclic: Polar Coordinates
                    let angle = 2.0 * PI * (i as f32) / count;
                    let x = center.x + radius * angle.cos();
                    let y = center.y + radius * angle.sin();
                    new_pos = egui::pos2(x, y);
                } else {
                    // Linear: Cartesian
                    // x based on index, y based on layer
                    let x_spacing = 80.0;
                    let y_spacing = 100.0;
                    
                    let x = center.x + ((i as f32) - (count - 1.0)/2.0) * x_spacing;
                    let y = 600.0 - (layer_idx as f32 * y_spacing); 
                    new_pos = egui::pos2(x, y);
                }

                // Update node position
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

            // 1. New Vertex Creation (Single)
            if ui.button("New Vertex").clicked() {
                self.show_new_node_dialog = true;
                self.show_multi_add_dialog = false;
                self.new_node_name.clear();
            }
            
            // 1b. Multi Add Mode
            if ui.button("Multi Add").clicked() {
                self.show_multi_add_dialog = true;
                self.show_new_node_dialog = false;
                self.new_node_name.clear();
            }

            ui.separator();
            ui.checkbox(&mut self.cyclic, "Cyclic Mode");
            
            ui.separator();
            ui.label("Algorithms:");

            // Convert GUI graph to Lattice struct
            let lattice = self.to_lattice();

            // 2. Existence Check
            if ui.button("Existence").clicked() {
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
                self.active_strip = None; // Reset view
                self.active_strip_edges = None;
            }

            // 3. Count Strips
            if ui.button("Count").clicked() {
                let mut count = 0;
                for ham in lattice.ham_paths(self.cyclic) {
                    // Use lazy dfs to find all strips for this path
                    let new_strips = rhombic_strips_dfs_lazy(vec![ham], &lattice, lattice.dim as usize, self.cyclic);
                    count += new_strips.len();
                }
                self.msg_log = format!("Number of rhombic strips found: {}", count);
                self.active_strip = None;
                self.active_strip_edges = None;
            }

            // 4. Show Strip
            if ui.button("Show").clicked() {
                let mut found_strip = None;
                
                // Find first valid strip
                for ham in lattice.ham_paths(self.cyclic) {
                    let strips = rhombic_strips_dfs_lazy(vec![ham], &lattice, lattice.dim as usize, self.cyclic);
                    if !strips.is_empty() {
                        found_strip = Some(strips[0].clone());
                        break;
                    }
                }

                if let Some(strip) = found_strip {
                    self.msg_log = "Displaying first found strip.".to_string();
                    self.apply_strip_layout(&strip);
                    
                    // Convert the strip to usize for edges_strip call
                    let strip_usize: Vec<Vec<usize>> = strip.iter()
                        .map(|layer| layer.iter().map(|&x| x as usize).collect())
                        .collect();
                    
                    // Call the requested function to get exact edges
                    let edge_indices = edges_strip(&strip_usize, &lattice, self.cyclic);
                    
                    // Convert indices back to GUI node IDs for the renderer
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
                }
            }

            if ui.button("Reset View").clicked() {
                self.active_strip = None;
                self.active_strip_edges = None;
                self.msg_log = "View reset. You can edit nodes now.".to_string();
            }

            ui.separator();
            ui.heading("Log:");
            ui.label(&self.msg_log);
            
            ui.add_space(20.0);
            ui.small("Instructions:\n- Drag nodes to move\n- Click Node A then Node B to create cover relation A < B\n- 'Show' locks editing and hides non-strip edges.");
        });

        // --- FLOATING WINDOWS ---
        
        // 1. Single Add Dialog
        if self.show_new_node_dialog {
            egui::Window::new("Enter Label")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -100.0))
                .show(ctx, |ui| {
                    ui.text_edit_singleline(&mut self.new_node_name);
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            let id = self.node_counter;
                            self.nodes.push(GuiNode {
                                id,
                                label: if self.new_node_name.is_empty() { format!("{}", id) } else { self.new_node_name.clone() },
                                pos: egui::pos2(400.0 + (id as f32 * 20.0), 200.0), // Slight offset
                                dragged: false,
                            });
                            self.node_counter += 1;
                            self.show_new_node_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_node_dialog = false;
                        }
                    });
                });
        }
        
        // 2. Multi Add Dialog (Corrected)
        if self.show_multi_add_dialog {
            egui::Window::new("Multi Add Mode")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -100.0))
                .show(ctx, |ui| {
                    ui.label("Type label and press Enter to add.");
                    let response = ui.text_edit_singleline(&mut self.new_node_name);
                    
                    // Request focus immediately to ensure user can keep typing
                    response.request_focus();
                    
                    let mut add_pressed = false;

                    // Add Logic
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
                            pos: egui::pos2(400.0 + (id as f32 * 20.0), 200.0),
                            dragged: false,
                        });
                        self.node_counter += 1;
                        self.new_node_name.clear(); // Clear text box for next entry
                        self.msg_log = format!("Added node {}", id);
                    }
                });
        }


        // --- CENTRAL PANEL (Canvas) ---
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(egui::Color32::WHITE))
            .show(ctx, |ui| {
            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            
            // Determine which nodes to draw
            let visible_node_ids: Vec<usize> = if let Some(strip) = &self.active_strip {
                strip.iter().flatten().map(|&x| {
                     // Map index back to ID safely
                     if (x as usize) < self.nodes.len() {
                         self.nodes[x as usize].id
                     } else {
                         usize::MAX 
                     }
                }).collect()
            } else {
                self.nodes.iter().map(|n| n.id).collect()
            };

            // Draw Edges
            let stroke_default = egui::Stroke::new(2.0, egui::Color32::BLACK);
            
            // Choose source of edges based on mode
            let edges_to_draw = if let Some(ref specific_edges) = self.active_strip_edges {
                specific_edges
            } else {
                &self.edges
            };

            for (from, to) in edges_to_draw {
                // If node is not visible, don't draw edge
                if !visible_node_ids.contains(from) || !visible_node_ids.contains(to) {
                    continue; 
                }

                // Find positions
                let p1_opt = self.nodes.iter().find(|n| n.id == *from).map(|n| n.pos);
                let p2_opt = self.nodes.iter().find(|n| n.id == *to).map(|n| n.pos);

                if let (Some(p1), Some(p2)) = (p1_opt, p2_opt) {
                     painter.line_segment([p1, p2], stroke_default);
                }
            }
            
            // Draw creation preview line
            if let Some(start_id) = self.edge_start_node {
                if let Some(start_node) = self.nodes.iter().find(|n| n.id == start_id) {
                    if let Some(pointer_pos) = response.hover_pos() {
                        painter.line_segment(
                            [start_node.pos, pointer_pos], 
                            egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(255, 0, 0, 128))
                        );
                    }
                }
            }

            // Draw Nodes
            for i in 0..self.nodes.len() {
                let node_id = self.nodes[i].id;
                
                // Skip if hidden
                if !visible_node_ids.contains(&node_id) { continue; }

                let node_pos = self.nodes[i].pos;
                let label = self.nodes[i].label.clone();
                
                // Interaction Area
                let node_radius = 20.0;
                let node_rect = egui::Rect::from_center_size(node_pos, egui::vec2(node_radius*2.0, node_radius*2.0));
                let node_response = ui.interact(node_rect, egui::Id::new(node_id), egui::Sense::click_and_drag());

                // Dragging Logic
                if node_response.dragged() {
                    self.nodes[i].pos += node_response.drag_delta();
                    self.nodes[i].dragged = true;
                } else {
                    self.nodes[i].dragged = false;
                }

                // Click Logic (Edge Creation)
                if node_response.clicked() && self.active_strip.is_none() {
                    match self.edge_start_node {
                        None => {
                            self.edge_start_node = Some(node_id);
                            self.msg_log = format!("Selected {}. Click target for cover relation...", label);
                        }
                        Some(start_id) => {
                            if start_id != node_id {
                                // Add Edge v1 -> v2 (v1 < v2)
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

                // Drawing the Node
                let fill_color = if self.edge_start_node == Some(node_id) {
                    egui::Color32::LIGHT_RED // Highlight selected source
                } else {
                    egui::Color32::WHITE
                };
                
                painter.circle(node_pos, node_radius, fill_color, egui::Stroke::new(1.0, egui::Color32::BLACK));
                painter.text(node_pos, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(14.0), egui::Color32::BLACK);
            }
            
            // Right click to cancel edge creation
            if response.clicked_by(egui::PointerButton::Secondary) {
                self.edge_start_node = None;
                self.msg_log = "Edge creation cancelled.".to_string();
            }
        });
    }
}

// Entry point function
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