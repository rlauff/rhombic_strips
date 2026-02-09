// functionality for reading in and dealing with the face lattice


//faces are contained in the face lattice which acts as an arena. The upset etc are saved as a list of indices to the faces in this arena.
//a face object alone hence makes no sense

use std::fs::read_to_string;
use std::collections::HashMap;
use itertools::Itertools;
use std::str;

#[derive(Debug)]
pub struct Face {
    pub label: String,
    pub dim: usize,
    pub upset: Vec<usize>,
    pub downset: Vec<usize>,
}

#[derive(Debug)]
pub struct Lattice {    // all usize are indices pointing into the faces vec
    pub faces: Vec<Face>,
    pub levels: Vec<Vec<usize>>,
    pub bridges_above: HashMap<(usize, usize), usize>, // for a pair in the same level, returns the bridge above
    pub bridges_below: HashMap<(usize, usize), usize>, // for a pair in the same level, returns the bridge below
    pub dim: usize,
}

//bridges are precomputed and stored in the face lattice object. Note that the keys of the bridges HashMap are the edges of the graphs on this levels

fn bridge_above(faces: &Vec<Face>, f1: usize, f2: usize) -> Option<usize> {
    for (i, face) in faces.iter().enumerate() {
        if face.downset.contains(&f1) && face.downset.contains(&f2) {
            return Some(i);
        }
    }
    None
}
fn bridge_below(faces: &Vec<Face>, f1: usize, f2: usize) -> Option<usize> {
    for (i, face) in faces.iter().enumerate() {
        if face.upset.contains(&f1) && face.upset.contains(&f2) {
            return Some(i);
        }
    }
    None
}

pub fn lattice_from_file(file: &str) -> Lattice {
    //read and store faces as in the file
    let mut faces: Vec<Face> = vec![];
    for face_str in read_to_string(file).expect("reading file failed").lines() {
        if face_str.chars().nth(0).unwrap() == '[' {
            continue;
        }

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
    let mut bridges_above = HashMap::new();
    let mut bridges_below = HashMap::new();
    for i in 0..faces.len() {
        for j in 0..faces.len() {
            if i >= j { continue };
            // if i == j {
            //     bridges.insert((i,j), None);
            // }
            let b_above = bridge_above(&faces, i, j);
            let b_below = bridge_below(&faces, i, j);
            if !b_above.is_none() {
                bridges_above.insert((i,j), b_above.unwrap());
            };
            if !b_below.is_none() {
                bridges_below.insert((i,j), b_below.unwrap());
            };
        }
    }

    Lattice {
        faces: faces,
        levels: levels,
        bridges_above: bridges_above,
        bridges_below: bridges_below,
        dim: max_dim+1,
    }
}

fn subsets<T: Clone>(items: &Vec<T>) -> Vec<Vec<T>> {
    (0..items.len())
    .map(|count| items.clone().into_iter().combinations(count))
    .flatten()
    .collect()
}

pub struct Graph {
    pub vertices: Vec<usize>,
    pub edges: Vec<[usize; 2]>,
    pub tubes: Option<Vec<Vec<usize>>>,
    //    tubings: Option<HashSet<Vec<Vec<usize>>>>,
}

impl Graph {
    fn is_connected(&self, vertices: &Vec<usize>) -> bool{
        if vertices.is_empty() {return false};
        if vertices.len() == 1 {return true};
        let mut active = Vec::new();
        let mut new = Vec::new();
        let mut found = Vec::new();
        active.push(vertices[0]);
        new.push(vertices[0]);

        loop{
            if active.is_empty() {break;};
            new.clear();

            for v in active.iter() {
                for e in self.edges.iter() {
                    if e[0] == *v && !found.contains(&e[1]) && vertices.contains(&e[1]) {found.push(e[1]); new.push(e[1])};
                    if e[1] == *v && !found.contains(&e[0]) && vertices.contains(&e[0]) {found.push(e[0]); new.push(e[0])};
                }
            }
            active = new.clone();
        }

        for v in vertices.iter() {
            if !found.contains(v) {return false;};
        }
        return true;
    }

    pub fn find_tubes(&mut self, centered: bool) {
        match self.tubes {
            Some(_) => {return;}
            None => {
                let mut tubes = vec![];
                for subset in subsets(&self.vertices).iter() {
                    // Restrict to tubes containing 0 if centered is true
                    if centered && !subset.contains(&0) {
                        continue;
                    }
                    if self.is_connected(&subset) {tubes.push(subset.to_vec());}
                }
                self.tubes = Some(tubes);
            }
        }
        return;
    }
}

fn is_above(tube1: &Vec<usize>, tube2: &Vec<usize>) -> bool {
    if !(tube1.len() == tube2.len()-1) { return false; };
    for elem in tube1.iter() {
        if !tube2.contains(elem) { return false; };
    }
    true
}

pub fn lattice_from_graph(g: &mut Graph, centered: bool) -> Lattice {
    g.find_tubes(centered);
    let tubes = g.tubes.clone().unwrap();


    let mut faces = vec![];
    for tube in tubes.iter() {
        //label
        let label = format!("{:?}", tube);

        //dim
        let dim = if centered { tube.len() - 1 } else { tube.len() - 1 };
        // Note: Logic for dim remains same for now, usually |tube|-1 for full tubing lattice.
        // If centered is effectively changing the poset structure significantly, ensure dim calculation fits definition.
        // Based on tube.len()-1 being standard for tubings.

        //upset
        let mut upset = vec![];
        for (i, other_tube) in tubes.iter().enumerate() {
            if is_above(tube, other_tube) {
                upset.push(i);
            }
        }

        //downset
        let mut downset = vec![];
        for (i, other_tube) in tubes.iter().enumerate() {
            if is_above(other_tube, tube) {
                downset.push(i);
            }
        }
        faces.push(
            Face {
                label: label,
                dim: dim,
                upset: upset,
                downset: downset
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
    let mut bridges_above = HashMap::new();
    let mut bridges_below = HashMap::new();
    for i in 0..faces.len() {
        for j in 0..faces.len() {
            if i >= j { continue };
            // if i == j {
            //     bridges.insert((i,j), None);
            // }
            let b_above = bridge_above(&faces, i, j);
            let b_below = bridge_below(&faces, i, j);
            if !b_above.is_none() {
                bridges_above.insert((i,j), b_above.unwrap());
            };
            if !b_below.is_none() {
                bridges_below.insert((i,j), b_below.unwrap());
            };
        }
    }

    Lattice {
        faces: faces,
        levels: levels,
        bridges_above: bridges_above,
        bridges_below: bridges_below,
        dim: max_dim,
    }
}
