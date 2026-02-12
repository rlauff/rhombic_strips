// functionality for reading in and dealing with the face lattice


//faces are contained in the face lattice which acts as an arena. The upset etc are saved as a list of indices to the faces in this arena.
//a face object alone hence makes no sense

use std::fs::read_to_string;
use std::collections::HashMap;
use std::str;

#[derive(Debug)]
pub struct Face {
    pub label: String,
    pub dim: usize,
    pub upset: Vec<usize>,
    pub downset: Vec<usize>,
}

#[derive(Debug)]
pub struct Lattice {
    pub faces: Vec<Face>,
    pub levels: Vec<Vec<usize>>,
    pub bridges: HashMap<(usize, usize), usize>,
    pub dim: usize,
    pub ham: Vec<Vec<usize>>,
}

//bridges are precomputed and stored in the face lattice object. Note that the keys of the bridges HashMap are the edges of the graphs on this levels

fn bridge(faces: &Vec<Face>, f1: usize, f2: usize) -> Option<usize> {
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
        let dim = face_str.split(": ").nth(0).expect("reading of a face failed, check lattice file").parse::<usize>().expect("something was not an integer");

        //upset
        let upset_str = face_str.split("{").nth(1).expect("reading of a face failed, check lattice file");
        let upset;
        if upset_str.len() == 3 {
            upset = vec![];
        } else {
            upset = upset_str[..upset_str.len()-3].split(", ")
            .map(|x| x.parse::<usize>().expect("something was not an integer")).collect();
        }

        //downset
        let downset_str = face_str.split("{").nth(2).expect("reading of a face failed, check lattice file");
        let downset;
        if downset_str.len() == 1 {
            downset = vec![];
        } else {
            downset = downset_str[..downset_str.len()-1].split(", ")
            .map(|x| x.parse::<usize>().expect("something was not an integer")).collect();
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
    let mut levels = vec![vec![]; max_dim+1];
    for (i, face) in faces.iter().enumerate() {
        levels[face.dim].push(i);
    }

    //generate and store bridges
    let mut bridges = HashMap::new();
    for i in 0..faces.len() {
        for j in 0..faces.len() {
            if i >= j { continue };
            // if i == j {
            //     bridges.insert((i,j), None);
            // }
            let b = bridge(&faces, i, j);
            if !b.is_none() {
                bridges.insert((i,j), b.unwrap());
            };
        }
    }

    let mut l = Lattice {
        faces: faces,
        levels: levels,
        bridges: bridges,
        dim: max_dim,
        ham: vec![],
    };
    l.gen_ham(cyclic);
    l
}

impl Lattice {
    // fill the ham field with the hamilton paths or cycles of the first layer
    // if cyclic is true, generate hamilton cycles, otherwise generate hamilton paths
    // written by gemini, but looks correct to me
    pub fn gen_ham(&mut self, cyclic: bool) -> Vec<Vec<usize>> {
        //check if the first layer exists and has nodes
        if self.levels.is_empty() || self.levels[0].is_empty() {
            return vec![];
        }
        
        //get the nodes of the first layer
        let nodes = self.levels[0].clone();
        let num_nodes = nodes.len();

        //build adjacency list for faster lookups during dfs
        let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
        for &node in &nodes {
            adj.insert(node, vec![]);
        }

        //populate adjacency list using the precomputed bridges
        for i in 0..num_nodes {
            for j in (i + 1)..num_nodes {
                let u = nodes[i];
                let v = nodes[j];
                
                //keys in bridges are always (smaller, larger) as generated in lattice_from_file
                let key = if u < v { (u, v) } else { (v, u) };
                
                if self.bridges.contains_key(&key) {
                    adj.get_mut(&u).unwrap().push(v);
                    adj.get_mut(&v).unwrap().push(u);
                }
            }
        }

        let mut all_paths = vec![];
        //visited array using the size of all faces so we can directly index by face id
        let mut visited = vec![false; self.faces.len()];

        if cyclic {
            //for cycles, we only need to start from one node since a cycle must include it anyway
            let start_node = nodes[0];
            let mut path = vec![start_node];
            visited[start_node] = true;
            
            self.dfs(start_node, start_node, &mut visited, &mut path, num_nodes, &adj, true, &mut all_paths);
        } else {
            //for paths, we have to try starting from every possible node
            for &start_node in &nodes {
                let mut path = vec![start_node];
                visited[start_node] = true;
                
                self.dfs(start_node, start_node, &mut visited, &mut path, num_nodes, &adj, false, &mut all_paths);
                
                //reset for the next start node
                visited[start_node] = false; 
            }
        }

        //store and return
        self.ham = all_paths.clone();
        all_paths
    }

    //dfs helper for backtracking through the graph
    fn dfs(
        &self,
        start: usize,
        current: usize,
        visited: &mut Vec<bool>,
        path: &mut Vec<usize>,
        target_len: usize,
        adj: &HashMap<usize, Vec<usize>>,
        cyclic: bool,
        all_paths: &mut Vec<Vec<usize>>,
    ) {
        //if path contains all nodes, we found a path
        if path.len() == target_len {
            if cyclic {
                //for cycles, check if the last node is connected back to the start
                if adj.get(&current).unwrap().contains(&start) {
                    all_paths.push(path.clone());
                }
            } else {
                // only push if the first node is smaller than or equal to the last node (equal if only one vertex exists)
                // this removes duplicates since we will find the same path in reverse when we start from the other end
                if path[0] <= path[path.len() - 1] {
                    all_paths.push(path.clone());
                }
            }
            return;
        }

        //explore all neighbors of the current node
        if let Some(neighbors) = adj.get(&current) {
            for &neighbor in neighbors {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    path.push(neighbor);

                    //go deeper
                    self.dfs(start, neighbor, visited, path, target_len, adj, cyclic, all_paths);

                    //backtrack
                    path.pop();
                    visited[neighbor] = false;
                }
            }
        }
    }
}


// fn subsets<T: Clone>(items: &Vec<T>) -> Vec<Vec<T>> {
//     (0..items.len())
//     .map(|count| items.clone().into_iter().combinations(count))
//     .flatten()
//     .collect()
// }

// pub struct Graph {
//     pub vertices: Vec<usize>,
//     pub edges: Vec<[usize; 2]>,
//     pub tubes: Option<Vec<Vec<usize>>>,
// //    tubings: Option<HashSet<Vec<Vec<usize>>>>,
// }

// impl Graph {
//     fn is_connected(&self, vertices: &Vec<usize>) -> bool{
//         if vertices.is_empty() {return false};
//         if vertices.len() == 1 {return true};
//         let mut active = Vec::new();
//         let mut new = Vec::new();
//         let mut found = Vec::new();
//         active.push(vertices[0]);
//         new.push(vertices[0]);

//         loop{
//             if active.is_empty() {break;};
//             new.clear();

//             for v in active.iter() {
//                 for e in self.edges.iter() {
//                     if e[0] == *v && !found.contains(&e[1]) && vertices.contains(&e[1]) {found.push(e[1]); new.push(e[1])};
//                     if e[1] == *v && !found.contains(&e[0]) && vertices.contains(&e[0]) {found.push(e[0]); new.push(e[0])};
//                 }
//             }
//             active = new.clone();
//         }

//         for v in vertices.iter() {
//             if !found.contains(v) {return false;};
//         }
//         return true;
//     }

//     pub fn find_tubes(&mut self, centered: bool) {
//         match self.tubes {
//             Some(_) => {return;}
//             None => {
//                 let mut tubes = vec![];
//                 for subset in subsets(&self.vertices).iter() {
//                     if self.is_connected(&subset) && (subset.contains(&0) || !centered) {tubes.push(subset.to_vec());}
//                 }
//                 self.tubes = Some(tubes);
//             }
//         }
//         return;
//     }

//     pub fn ham_cycles(&self) -> Vec<Vec<usize>> {
//         let mut active_paths = vec![vec![self.vertices[0]]];
//         let mut new_paths = vec![];
//         for _i in 0..(self.vertices.len()-1) {
//             for path in active_paths.iter() {
//                 for [a, b] in self.edges.iter() {
//                     if *a == path[path.len()-1] && !path.contains(&b) {
//                         let mut new = path.clone();
//                         new.push(*b);
//                         new_paths.push(new);
//                     }
//                     if *b == path[path.len()-1] && !path.contains(&a) {
//                         let mut new = path.clone();
//                         new.push(*a);
//                         new_paths.push(new);
//                     }
//                 }
//             }
//             active_paths.clear();
//             active_paths = new_paths.clone();
//             new_paths.clear();
//         }
//         active_paths.into_iter().filter(|x| self.edges.contains(&[x[x.len()-1], self.vertices[0]]) || self.edges.contains(&[self.vertices[0], x[x.len()-1]])).collect()
//     }

//     pub fn ham_paths_centered(&self) -> Vec<Vec<usize>> {
//         let V:Vec<_> = self.edges.iter().filter(|x| x[0]==0).collect();
//         let n = V.len();
//         V.into_iter().map(|x| x[1]).permutations(n).collect()
//     }
// }

// fn is_above(tube1: &Vec<usize>, tube2: &Vec<usize>) -> bool {
//     if !(tube1.len() == tube2.len()-1) { return false; };
//     for elem in tube1.iter() {
//         if !tube2.contains(elem) { return false; };
//     }
//     true
// }

// pub fn lattice_from_graph(g: &mut Graph, centered: bool) -> Lattice {
//     g.find_tubes(centered);
//     let tubes = g.tubes.clone().unwrap();
//     let hcs = if centered { g.ham_paths_centered() } else { g.ham_cycles() };

//     let mut faces = vec![];
//     for tube in tubes.iter() {
//         //label
//         let label = format!("{:?}", tube);

//         //dim
//         let dim = tube.len()-1;

//         //upset
//         let mut upset = vec![];
//         for (i, other_tube) in tubes.iter().enumerate() {
//             if is_above(tube, other_tube) {
//                 upset.push(i);
//             }
//         }

//         //downset
//         let mut downset = vec![];
//         for (i, other_tube) in tubes.iter().enumerate() {
//             if is_above(other_tube, tube) {
//                 downset.push(i);
//             }
//         }
//         faces.push(
//             Face {
//                 label: label,
//                 dim: dim,
//                 upset: upset,
//                 downset: downset
//             }
//         );
//     }
//     //make levels
//     let max_dim = faces.iter().map(|x| x.dim).max().unwrap();
//     let mut levels = vec![vec![]; max_dim+1];
//     for (i, face) in faces.iter().enumerate() {
//         levels[face.dim].push(i);
//     }

//     //generate and store bridges
//     let mut bridges = HashMap::new();
//     for i in 0..faces.len() {
//         for j in 0..faces.len() {
//             if i >= j { continue };
//             // if i == j {
//             //     bridges.insert((i,j), None);
//             // }
//             let b = bridge(&faces, i, j);
//             if !b.is_none() {
//                 bridges.insert((i,j), b.unwrap());
//             };
//         }
//     }

//     let mut ham_cycles = vec![];
//     if !centered {
//         //ham_cycles
//         for hc in hcs.into_iter() {
//             let mut c = vec![];
//             for elem in hc.iter() {
//                 for (i, face) in faces.iter().enumerate() {
//                     if face.label == format!("{:?}", vec![elem]) { c.push(i) };
//                 }
//             }
//             ham_cycles.push(c);
//         }
//     } else {
//         for hc in hcs.into_iter() {
//             let mut c = vec![];
//             for v in hc.iter() {
//                 for (i, face) in faces.iter().enumerate() {
//                     if face.label == format!("[0, {}]", v) { c.push(i) };
//                 }
//             }
//             ham_cycles.push(c);
//         }
//     }

//     Lattice {
//         faces: faces,
//         levels: levels,
//         bridges: bridges,
//         dim: max_dim,
//         ham: ham_cycles,
//     }
// }

// pub fn get_jobs(source: &str) {
//     let mut lattices: Vec<Lattice> = vec![];
//     for job_str in read_to_string(file).expect("reading file failed").split("%%") {
//         if job_str.chars().nth(0).unwrap() == 'G' { //we have a graph
//             let edges_str = job_str.split(": ").nth(1).unwrap();
//
//         } else { //we have a lattice
//
//         }
//     }
// }


