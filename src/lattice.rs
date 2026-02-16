// functionality for reading in and dealing with the face lattice


//faces are contained in the face lattice which acts as an arena. The upset etc are saved as a list of indices to the faces in this arena.
//a face object alone hence makes no sense

use std::fs::read_to_string;
use std::collections::HashMap;
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

fn bridge(faces: &Vec<Face>, f1: u8, f2: u8) -> Option<usize> {
    for (i, face) in faces.iter().enumerate() {
        if face.downset.contains(&f1) && face.downset.contains(&f2) {
            return Some(i);
        }
    }
    None
}

pub fn lattice_from_file(file: &str, cyclic: bool) -> Lattice {
    //read and store faces as in the file
    let mut faces: Vec<Face> = vec![];
    for face_str in read_to_string(file).expect("reading file failed").lines() {
        
        //dimension
        let dim = face_str.split(": ").nth(0).expect("reading of a face failed, check lattice file").parse::<u8>().expect("something was not an integer");

        //upset
        let upset_str = face_str.split("{").nth(1).expect("reading of a face failed, check lattice file");
        let mut upset = [255u8; 50]; // Initialize with 255 (empty indicator)
        for face_index in upset_str[..upset_str.len()-3].split(", ") {
            let index = face_index.parse::<usize>().expect("something was not an integer");
            if index < 50 {
                upset[index] = index as u8; // Store the index directly
            } else {
                panic!("Upset index exceeds maximum allowed value of 49");
            }
        }

        //downset
        let downset_str = face_str.split("{").nth(2).expect("reading of a face failed, check lattice file");
        let mut downset = [255u8; 50]; // Initialize with 255 (empty indicator)
        for face_index in downset_str[..downset_str.len()-3].split(", ") {
            let index = face_index.parse::<usize>().expect("something was not an integer");
            if index < 50 {
                downset[index] = index as u8; // Store the index directly
            } else {
                panic!("Downset index exceeds maximum allowed value of 49");
            }
        }

        faces.push(
            Face {
                label: face_str.split(": ").nth(1).expect("reading of a face failed, check lattice file").to_string(),
                dim: dim,
                upset: upset,
                downset: downset,
            }
        );
    }

    //make levels
    let max_dim = faces.iter().map(|x| x.dim).max().unwrap();
    let mut levels = [[255u8; 50]; 30]; // Initialize with 255 (empty indicator)
    let mut count_per_dim = [0u8; 30]; // To keep track of how many faces we have added to each dimension level
    for (i, face) in faces.iter().enumerate() {
        levels[face.dim as usize][count_per_dim[face.dim as usize] as usize] = i as u8; // Store the index directly
        count_per_dim[face.dim as usize] += 1;
    }

    //generate and store bridges
    let mut bridges = [255u8; 100*100]; // Initialize with 255 (no bridge indicator)
    for i in 0..faces.len() {
        for j in 0..faces.len() {
            if i >= j { continue };
           if let Some(b) = bridge(&faces, i as u8, j as u8) {
                bridges[i*100 + j] = b as u8;
                bridges[j*100 + i] = b as u8;
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