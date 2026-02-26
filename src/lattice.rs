use crate::MAX_FACES;
use crate::MAX_UP_DOWN;
use crate::MAX_LEVELS;

//faces are contained in the face lattice which acts as an arena. The upset etc are saved as a list of indices to the faces in this arena.
//a face object alone hence makes no sense

// Todo: Actually test if the fixes array sizes give a performance improvement
// Just from feel I cannot see a big difference
// Probably the compiler was putting the vecs on the stack already because
// we are always working with non mut Lattice objects
// The compiler can therefore tell that the vecs will never be touched
// time for CompilerExplorer I guess

use std::fs::read_to_string;
use std::str;

// unit types for stricter type checking

#[derive(Clone, Debug)]
pub struct Level{ pub faces: Vec<u8> }
impl Level {
    pub fn from_vec(vec: Vec<u8>) -> Self {
        Level{ faces: vec }
    }
    pub fn get_unchecked(&self, index: u8) -> u8 {
        self.faces[index as usize]
    }
    pub fn iter(&self) -> LevelIter<'_> {
        LevelIter::new(self)
    }
    pub fn len(&self) -> usize {
        for i in 0..MAX_UP_DOWN {
            if self.faces[i] == 255 {
                return i;
            }
        }
        MAX_UP_DOWN
    }
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty() 
    }
    pub fn contains(&self, value: &u8) -> bool {
        self.faces.contains(value)
    }
}

pub struct LevelIter<'a> {
    level: &'a Level,
    level_index: usize,
}
impl<'a> LevelIter<'a> {
    pub fn new(level: &'a Level) -> Self {
        LevelIter { level, level_index: 0 }
    }
}
impl<'a> Iterator for LevelIter<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.level_index < MAX_UP_DOWN {
            let face = self.level.faces[self.level_index];
            self.level_index += 1;
            Some(face)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct Levels{ pub levels: Vec<Level> }
impl Levels {
    pub fn new() -> Self {
        Levels{ levels: Vec::new() }
    }
    pub fn single_level_from_vec(vec: Vec<u8>) -> Self {
        let mut levels = Levels::new();
        levels.push(Level{ faces: vec });
        levels
    }
    pub fn _get_index_unchecked(&self, i: u8, j: u8) -> u8 {
        self.levels[i as usize].faces[j as usize]
    }
    pub fn get_unchecked(&self, d: u8) -> &Level {
        &self.levels[d as usize]
    }
    pub fn set_unchecked(&mut self, d: u8, value: u8) {
        // fill the first 255 by the given value
        for j in 0..MAX_UP_DOWN {
            if self.levels[d as usize].faces[j] == 255 {
                self.levels[d as usize].faces[j] = value;
                break;
            }
        }
    }
    pub fn into_iter(&self, d: u8) -> LevelIter<'_> {
        LevelIter::new(&self.levels[d as usize])
    }
    pub fn len(&self) -> usize {
        self.levels.len()
    }
    pub fn push(&mut self, level: Level) {
        self.levels.push(level);
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Bridge{ faces_indices: [u8; MAX_FACES] }

#[derive(Copy, Clone, Debug)]
pub struct Bridges{ bridges: [Bridge; MAX_FACES] }
impl Bridges {
    pub fn new() -> Self {
        Bridges{ bridges: [Bridge{ faces_indices: [255u8; MAX_FACES] }; MAX_FACES] } // Initialize with 255 (no bridge indicator)
    }
    pub fn get_unchecked(&self, i: u8, j: u8) -> u8 {
        self.bridges[i as usize].faces_indices[j as usize]
    }
    pub fn set_unchecked(&mut self, i: u8, j: u8, value: u8) {
        self.bridges[i as usize].faces_indices[j as usize] = value;
        self.bridges[j as usize].faces_indices[i as usize] = value; // Ensure symmetry
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Tunnel{ faces_indices: [u8; MAX_FACES] }

#[derive(Copy, Clone, Debug)]
pub struct Tunnels{ tunnels: [Tunnel; MAX_FACES] }
impl Tunnels {
    pub fn new() -> Self {
        Tunnels{ tunnels: [Tunnel{ faces_indices: [255u8; MAX_FACES] }; MAX_FACES] } // Initialize with 255 (no tunnel indicator)
    }
    pub fn _get_unchecked(&self, i: u8, j: u8) -> u8 {
        self.tunnels[i as usize].faces_indices[j as usize]
    }
    pub fn set_unchecked(&mut self, i: u8, j: u8, value: u8) {
        self.tunnels[i as usize].faces_indices[j as usize] = value;
        self.tunnels[j as usize].faces_indices[i as usize] = value; // Ensure symmetry
    }
}

#[derive(Clone, Debug)]
pub struct UpDownSet {
    pub faces: [u8; MAX_UP_DOWN], // fixed size arrays for better caching. 255 indicates empty
}
impl UpDownSet {
    pub fn new() -> Self {
        UpDownSet { faces: [255u8; MAX_UP_DOWN] } // Initialize with 255 (empty indicator)
    }
    pub fn _get_unchecked(&self, index: u8) -> u8 {
        self.faces[index as usize]
    }
    pub fn contains(&self, value: &u8) -> bool {
        self.faces.iter().any(|&x| x == *value)
    }
    pub fn iter(&self) -> UpDownSetIter<'_> {
        UpDownSetIter::new(self)
    }
    pub fn push(&mut self, value: u8) {
        for i in 0..MAX_UP_DOWN {
            if self.faces[i] == 255 {
                self.faces[i] = value;
                return;
            }
        }
        panic!("UpDownSet is full, cannot push more values");
    }
}
pub struct UpDownSetIter<'a> {
    set: &'a UpDownSet,
    index: usize,
}
impl<'a> UpDownSetIter<'a> {
    pub fn new(set: &'a UpDownSet) -> Self {
        UpDownSetIter { set, index: 0 }
    }
}
impl<'a> Iterator for UpDownSetIter<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        for &face in self.set.faces.iter().skip(self.index) {
            if face != 255 { // 255 indicates an empty slot
                self.index += 1;
                return Some(face);
            } else {
                break; // Since we fill from the start, we can stop at the first 255
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct Face {
    pub label: String,
    pub dim: u8,
    pub upset: UpDownSet,        // fixed size arrays for better caching. 255 indicates empty
    pub downset: UpDownSet,
}

#[derive(Clone, Debug)]
pub struct Faces{ faces: Vec<Face> }
impl Faces {
    pub fn get_unchecked(&self, index: u8) -> &Face {
        &self.faces[index as usize]
    }
    pub fn new() -> Self {
        Faces { faces: Vec::new() }
    }
    pub fn push(&mut self, face: Face) {
        self.faces.push(face);
    }
    pub fn iter(&self) -> std::slice::Iter<'_, Face> {
        self.faces.iter()
    }
    pub fn len(&self) -> usize {
        self.faces.len()
    }
}

// faces contains the face objects
// levels contains indices to faces, organized by dimension
// given two faces of the same level, they might have a bridge one level above
//      the bridges matrix contains the indeices to bridges, or 255 for no bridge
// tunnels are the same, but for the level below
// dim is the number of levels that contain an actual face

#[derive(Debug)]
pub struct Lattice {
    pub faces: Faces,
    pub levels: Levels, // fixed size arrays for better caching. 255 indicates empty
    pub bridges: Bridges, // max number of faces is 100, so we can store the bridges in a 100x100 array. 255 indicates no bridge, otherwise the value is the index of the bridge face
    pub _tunnels: Tunnels,
    pub dim: u8,
}

impl Lattice {
    pub fn get_bridge_unchecked(&self, f1: u8, f2: u8) -> u8 {
        self.bridges.get_unchecked(f1, f2)
    }
    pub fn _is_bridge(&self, f1: u8, f2: u8) -> bool {
        self.bridges.get_unchecked(f1, f2) != 255
    }
}

//bridges are precomputed and stored in the face lattice object. Note that the keys of the bridges HashMap are the edges of the graphs on this levels

fn bridge(faces: &Faces, f1: u8, f2: u8) -> Option<u8> {
    for (i, face) in faces.faces.iter().enumerate() {
        if face.downset.contains(&f1) && face.downset.contains(&f2) {
            return Some(i as u8);
        }
    }
    None
}

fn tunnel(faces: &Faces, f1: u8, f2: u8) -> Option<u8> {
    for (i, face) in faces.faces.iter().enumerate() {
        if face.upset.contains(&f1) && face.upset.contains(&f2) {
            return Some(i as u8);
        }
    }
    None
}

pub fn lattice_from_file(file: &str) -> Lattice {
    // read and store faces as in the file
    let mut faces = Faces::new(); // Initialize with an empty vector
    
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
        let mut upset = UpDownSet::new(); // Initialize with 255 (empty indicator)
        let mut upset_count = 0;     // Counter to act as a stack pointer

        if !upset_clean.is_empty() {
            for face_index_str in upset_clean.split(',') {
                let trimmed = face_index_str.trim();
                if trimmed.is_empty() { continue; }

                let val = trimmed.parse::<usize>().expect("something was not an integer");
                
                // Logic fix: "push" the value into the next available slot
                if upset_count < MAX_UP_DOWN {
                    upset.faces[upset_count] = val as u8; 
                    upset_count += 1;
                } else {
                    panic!("Upset count exceeds maximum allowed size of {}", MAX_UP_DOWN);
                }
            }
        }

        // downset
        // Remove the trailing '}' and any surrounding whitespace
        let downset_clean = set_strings[1].trim_end_matches('}').trim();
        let mut downset = UpDownSet::new(); // Initialize with 255 (empty indicator)
        let mut downset_count = 0;     // Counter to act as a stack pointer

        if !downset_clean.is_empty() {
            for face_index_str in downset_clean.split(',') {
                let trimmed = face_index_str.trim();
                if trimmed.is_empty() { continue; }

                let val = trimmed.parse::<usize>().expect("something was not an integer");

                // Logic fix: "push" the value into the next available slot
                if downset_count < MAX_UP_DOWN {
                    downset.faces[downset_count] = val as u8;
                    downset_count += 1;
                } else {
                    panic!("Downset count exceeds maximum allowed size of {}", MAX_UP_DOWN);
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
    let mut levels = Levels::new(); // Initialize with 255 (empty indicator)
    
    for (i, face) in faces.iter().enumerate() {
        let d = face.dim as usize;
        // Check bounds to prevent panics
        if d < MAX_LEVELS {
             levels.set_unchecked(d as u8, i as u8); // Store the index directly
        }
    }

    // generate and store bridges and tunnels
    let mut bridges = Bridges::new(); // Initialize with 255 (no bridge indicator)
    let mut tunnels = Tunnels::new(); // Initialize with 255 (no tunnel indicator)
    for i in 0..faces.len() {
        for j in i+1..faces.len() {
            // Check if a bridge exists between face i and face j
            if let Some(b) = bridge(&faces, i as u8, j as u8) {
                // Bounds check for the 2D array
                if i < MAX_FACES && j < MAX_FACES {
                    bridges.set_unchecked(i as u8, j as u8, b);
                }
            }
            // Check if a tunnel exists between face i and face j
            if let Some(t) = tunnel(&faces, i as u8, j as u8) {
                // Bounds check for the 2D array
                if i < MAX_FACES && j < MAX_FACES {
                    tunnels.set_unchecked(i as u8, j as u8, t);
                }
            }
        }
    }

    let l = Lattice {
        faces: faces,
        levels: levels,
        bridges: bridges,
<<<<<<< HEAD
        _tunnels: tunnels,
        dim: max_dim
=======
        tunnels: tunnels,
        dim: max_dim + 1, // since dimensions are 0-indexed, we add 1 to get the count
>>>>>>> parent of 6b32246 (removed dependency on max_dim in all the functions. The max dim is already in the Lattice struct, so no need to pass it around)
    };
    l
}

impl Lattice {

    // Create an iterator that generates hamilton paths or cycles lazily
    // This avoids allocating a massive Vec<Vec<usize>> and allows stopping early
    pub fn ham_paths(&self, cyclic: bool) -> HamiltonianIter {
        // check if the first layer exists
        // we filter out 255 (empty slots) to get actual nodes
        let nodes: Vec<u8> = self.levels
            .into_iter(0)
            .filter(|&n| n != 255u8)
            .collect();

        if nodes.is_empty() {
            return HamiltonianIter::empty();
        }

        // build adjacency list for faster lookups during iteration
        // since max faces is 100, we can use a direct vector index
        let mut adj: Vec<Vec<u8>> = vec![vec![]; MAX_FACES]; 
        let num_nodes = nodes.len();

        // populate adjacency list using the precomputed bridges matrix
        for i in 0..num_nodes {
            for j in (i + 1)..num_nodes {
                let u = nodes[i];
                let v = nodes[j];
                let connected = self.bridges.get_unchecked(u, v) != 255;

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

        if self.nodes.len() == 1 {
            // Wir prüfen, ob der Pfad noch existiert (wurde in 'new' -> 'push_start_node' gesetzt)
            if self.path.len() == 1 {
                let res = self.path.clone();
                
                // Aufräumen, damit der nächste Aufruf None zurückgibt
                self.path.clear();
                self.stack.clear();
                self.finished = true;
                
                return Some(res);
            }
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