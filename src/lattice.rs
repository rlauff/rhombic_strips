// functionality for reading in and dealing with the face lattice


//faces are contained in the face lattice which acts as an arena. The upset etc are saved as a list of indices to the faces in this arena.
//a face object alone hence makes no sense

use std::fs::read_to_string;
use std::str;

#[derive(Debug)]
pub struct Face {
    pub label: String,
    pub dim: u8,
    pub upset: [u8; 50],        // fixed size arrays for better caching. 255 indicates empty
    pub downset: [u8; 50],
}

#[derive(Debug)]
pub struct Lattice {
    pub faces: Vec<Face>,
    pub levels: [[u8; 50]; 30], // fixed size arrays for better caching. 255 indicates empty
    pub bridges: [u8; 100*100], // max number of faces is 100, so we can store the bridges in a 100x100 array. 255 indicates no bridge, otherwise the value is the index of the bridge face
    pub dim: u8,
    // pub ham: Vec<Vec<u8>>, // to be exchanged for a impl iter that generates the hamilton paths on the fly, since storing them can be very memory intensive for large lattices
}

//bridges are precomputed and stored in the face lattice object. Note that the keys of the bridges HashMap are the edges of the graphs on this levels

fn bridge(faces: &Vec<Face>, f1: u8, f2: u8) -> Option<u8> {
    for (i, face) in faces.iter().enumerate() {
        if face.downset.contains(&f1) && face.downset.contains(&f2) {
            return Some(i as u8);
        }
    }
    None
}

pub fn lattice_from_file(file: &str) -> Lattice {
    // read and store faces as in the file
    let mut faces: Vec<Face> = vec![];
    
    // We expect the file to exist at the path provided
    let content = read_to_string(file).expect("reading file failed");

    for face_str in content.lines() {
        // skip empty lines
        if face_str.trim().is_empty() { continue; }

        // Split the line into parts: Dim, Label, and the Sets
        // Example Line: "0: 000: {16, 10, 8}, {}"
        // We limit the split to 3 parts to keep the sets string intact
        let parts: Vec<&str> = face_str.splitn(3, ": ").collect();
        
        // Safety check to ensure the line format is correct
        if parts.len() < 3 { continue; } 

        // dimension
        let dim = parts[0].trim().parse::<u8>().expect("something was not an integer");
        let label = parts[1].trim().to_string();

        // The third part contains the sets string: "{...}, {...}"
        let sets_part = parts[2];

        // We split upset and downset using the separator "}, {"
        // This is more robust than fixed indices.
        let set_strings: Vec<&str> = sets_part.split("}, {").collect();
        if set_strings.len() < 2 { 
            panic!("reading of a face failed, check lattice file sets format"); 
        }

        // upset
        // Remove the leading '{' and any surrounding whitespace
        let upset_clean = set_strings[0].trim_start_matches('{').trim();
        let mut upset = [255u8; 50]; // Initialize with 255 (empty indicator)
        let mut upset_count = 0;     // Counter to act as a stack pointer

        if !upset_clean.is_empty() {
            for face_index_str in upset_clean.split(',') {
                let trimmed = face_index_str.trim();
                if trimmed.is_empty() { continue; }

                let val = trimmed.parse::<usize>().expect("something was not an integer");
                
                // Logic fix: "push" the value into the next available slot
                if upset_count < 50 {
                    upset[upset_count] = val as u8; 
                    upset_count += 1;
                } else {
                    panic!("Upset count exceeds maximum allowed size of 50");
                }
            }
        }

        // downset
        // Remove the trailing '}' and any surrounding whitespace
        let downset_clean = set_strings[1].trim_end_matches('}').trim();
        let mut downset = [255u8; 50]; // Initialize with 255 (empty indicator)
        let mut downset_count = 0;     // Counter to act as a stack pointer

        if !downset_clean.is_empty() {
            for face_index_str in downset_clean.split(',') {
                let trimmed = face_index_str.trim();
                if trimmed.is_empty() { continue; }

                let val = trimmed.parse::<usize>().expect("something was not an integer");

                // Logic fix: "push" the value into the next available slot
                if downset_count < 50 {
                    downset[downset_count] = val as u8;
                    downset_count += 1;
                } else {
                    panic!("Downset count exceeds maximum allowed size of 50");
                }
            }
        }

        faces.push(
            Face {
                label: label,
                dim: dim,
                upset: upset,
                downset: downset,
            }
        );
    }

    // make levels
    let max_dim = faces.iter().map(|x| x.dim).max().unwrap_or(0);
    let mut levels = [[255u8; 50]; 30]; // Initialize with 255 (empty indicator)
    let mut count_per_dim = [0u8; 30]; // To keep track of how many faces we have added to each dimension level
    
    for (i, face) in faces.iter().enumerate() {
        let d = face.dim as usize;
        // Check bounds to prevent panics
        if d < 30 && (count_per_dim[d] as usize) < 50 {
             levels[d][count_per_dim[d] as usize] = i as u8; // Store the index directly
             count_per_dim[d] += 1;
        }
    }

    // generate and store bridges
    let mut bridges = [255u8; 100*100]; // Initialize with 255 (no bridge indicator)
    for i in 0..faces.len() {
        for j in 0..faces.len() {
            if i >= j { continue };
            
            // Check if a bridge exists between face i and face j
            if let Some(b) = bridge(&faces, i as u8, j as u8) {
                // Bounds check for the flattened array
                if i * 100 + j < bridges.len() {
                    bridges[i * 100 + j] = b;
                    bridges[j * 100 + i] = b;
                }
            }
        }
    }

    let l = Lattice {
        faces: faces,
        levels: levels,
        bridges: bridges,
        dim: max_dim,
    };
    l
}

impl Lattice {
    // Create an iterator that generates hamilton paths or cycles lazily
    // This avoids allocating a massive Vec<Vec<usize>> and allows stopping early
    pub fn ham_paths(&self, cyclic: bool) -> HamiltonianIter {
        // check if the first layer exists
        // we filter out 255 (empty slots) to get actual nodes
        let nodes: Vec<u8> = self.levels[0]
            .iter()
            .filter(|&&n| n != 255)
            .cloned()
            .collect();

        if nodes.is_empty() {
            return HamiltonianIter::empty();
        }

        // build adjacency list for faster lookups during iteration
        // since max faces is 100, we can use a direct vector index
        let mut adj: Vec<Vec<u8>> = vec![vec![]; 100]; 
        let num_nodes = nodes.len();

        // populate adjacency list using the precomputed bridges matrix
        for i in 0..num_nodes {
            for j in (i + 1)..num_nodes {
                let u = nodes[i];
                let v = nodes[j];
                
                // check bridges array (flattened 100x100)
                // we check both directions u->v and v->u just to be safe
                let idx1 = (u as usize) * 100 + (v as usize);
                let idx2 = (v as usize) * 100 + (u as usize);
                
                let connected = (idx1 < self.bridges.len() && self.bridges[idx1] != 255) ||
                                (idx2 < self.bridges.len() && self.bridges[idx2] != 255);

                if connected {
                    adj[u as usize].push(v);
                    adj[v as usize].push(u);
                }
            }
        }

        HamiltonianIter::new(nodes, adj, cyclic)
    }
}

// Iterator struct to hold the state of the DFS
pub struct HamiltonianIter {
    nodes: Vec<u8>,         // List of valid nodes in the layer
    adj: Vec<Vec<u8>>,      // Adjacency list
    cyclic: bool,           // Mode: cycle or path
    
    // DFS State
    stack: Vec<(u8, usize)>, // (Current Node, Index of next neighbor to try in adj list)
    path: Vec<u8>,           // Current path being built
    visited: Vec<bool>,      // Lookup for visited nodes (size 100)
    
    // Loop control
    start_node_index: usize, // Which node in 'nodes' are we currently starting from?
    finished: bool,
}

impl HamiltonianIter {
    fn new(nodes: Vec<u8>, adj: Vec<Vec<u8>>, cyclic: bool) -> Self {
        let mut iter = HamiltonianIter {
            nodes,
            adj,
            cyclic,
            stack: Vec::with_capacity(100),
            path: Vec::with_capacity(100),
            visited: vec![false; 100],
            start_node_index: 0,
            finished: false,
        };
        
        // initialize the first start node
        iter.push_start_node();
        iter
    }

    fn empty() -> Self {
        HamiltonianIter {
            nodes: vec![], adj: vec![], cyclic: false, 
            stack: vec![], path: vec![], visited: vec![], 
            start_node_index: 0, finished: true 
        }
    }

    // Helper to reset state and push a new start node
    fn push_start_node(&mut self) {
        if self.start_node_index >= self.nodes.len() {
            self.finished = true;
            return;
        }

        // for cycles, we only need to try starting from the very first node
        // because a cycle is a loop (A-B-C-A is same as B-C-A-B)
        if self.cyclic && self.start_node_index > 0 {
            self.finished = true;
            return;
        }

        let start_node = self.nodes[self.start_node_index];
        
        self.path.clear();
        self.stack.clear();
        // clear visited array
        self.visited.fill(false);

        self.visited[start_node as usize] = true;
        self.path.push(start_node);
        self.stack.push((start_node, 0)); // start with 0th neighbor
    }
}

impl Iterator for HamiltonianIter {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        // Iterative DFS loop
        loop {
            // if stack is empty, we are done with the current start_node
            // move to the next possible start node
            if self.stack.is_empty() {
                self.start_node_index += 1;
                self.push_start_node();
                if self.finished {
                    return None;
                }
                continue;
            }

            // peek at the current node and neighbor index
            let (u, neighbor_idx) = *self.stack.last().unwrap();
            let u_idx = u as usize;
            
            // if we have explored all neighbors of u, backtrack
            if neighbor_idx >= self.adj[u_idx].len() {
                self.stack.pop();
                self.path.pop();
                self.visited[u_idx] = false;
                continue;
            }

            // prepare to look at the next neighbor next time
            self.stack.last_mut().unwrap().1 += 1;

            let v = self.adj[u_idx][neighbor_idx];
            let v_idx = v as usize;

            if !self.visited[v_idx] {
                // move forward
                self.visited[v_idx] = true;
                self.path.push(v);
                self.stack.push((v, 0));

                // check if path is complete
                if self.path.len() == self.nodes.len() {
                    let mut result = None;

                    if self.cyclic {
                        // check if last node connects back to start
                        let start = self.path[0];
                        if self.adj[v_idx].contains(&start) {
                            result = Some(self.path.clone());
                        }
                    } else {
                        // break symmetry for paths: start <= end
                        if self.path[0] <= self.path[self.path.len() - 1] {
                            result = Some(self.path.clone());
                        }
                    }

                    // CRITICAL: We must backtrack immediately to allow finding the next solution
                    // otherwise the loop would get stuck at max depth
                    self.stack.pop();
                    self.path.pop();
                    self.visited[v_idx] = false;

                    // if we found a valid result, return it
                    if result.is_some() {
                        return result;
                    }
                }
            }
        }
    }
}