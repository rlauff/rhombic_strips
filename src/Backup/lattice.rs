// functionality for reading in and dealing with the face lattice


//faces are contained in the face lattice which acts as an arena. The upset etc are saved as a list of indices to the faces in this arena.
//a face object alone hence makes no sense

use std::fs::read_to_string;
use std::collections::HashMap;
use std::collections::HashSet;
use itertools::Itertools;
use std::iter::FromIterator;
use std::fs::read_to_string;
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

pub fn lattice_from_file(file: &str) -> Lattice{
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

    //dim
    let max_dim = (0..faces.len()).map(|x| faces[x].dim).max().unwrap();

    Lattice {
        faces: faces,
        levels: levels,
        bridges: bridges,
        dim: max_dim
    }
}

fn subsets<T: Clone>(items: &Vec<T>) -> Vec<Vec<T>> {
    (0..items.len())
    .map(|count| items.clone().into_iter().combinations(count))
    .flatten()
    .collect()
}

fn are_neighboors(tubing1: &Vec<Vec<usize>>, tubing2: &Vec<Vec<usize>>) -> bool {
    let mut count = 0;
    for tube in tubing1.iter() {
        if !tubing2.contains(tube) {
            count += 1;
        }
    }
    return count == 1;
}

fn are_compatible(tube1: &Vec<usize>, tube2: &Vec<usize>, g: &Graph) -> bool {
    let t1: std::collections::HashSet<&usize> = HashSet::from_iter(tube1);
    let t2: std::collections::HashSet<&usize> = HashSet::from_iter(tube2);

    if t1 == t2 {return false;};

    if t1.is_subset(&t2) || t2.is_subset(&t1) {return true};
    if t1.is_disjoint(&t2) {
        for e in g.edges.iter() {
            if (t1.contains(&e[0]) && t2.contains(&e[1])) || (t1.contains(&e[1]) && t2.contains(&e[0])) {return false;};
        }
    } else {return false;};
    return true;
}

fn compatible_with(partial_tubing: &Vec<Vec<usize>>, g: &mut Graph) -> HashSet<Vec<Vec<usize>>> {
    let mut result = HashSet::new();
    let mut to_add;

    //println!("What is compatible with {}?", format!("{:?}", partial_tubing));

    if g.tubes == None {g.find_tubes();};

    for tube1 in g.tubes.as_ref().unwrap().iter() {
        to_add = true;
        for tube2 in partial_tubing.iter() {
            if !are_compatible(&tube1, &tube2, g) {to_add = false;};
        }
        if to_add {
            let mut temp = partial_tubing.clone();
            temp.push(tube1.clone());
            result.insert(temp);
        };
    }
    //println!("returning {}", format!("{:?}", result));
    return result;
}

fn new_tube<'a> (v1: &'a Vec<Vec<usize>>, v2: &'a Vec<Vec<usize>>) -> Option<&'a Vec<usize>> {
    for tube in v1.iter() {
        if !v2.contains(tube) {return Some(&tube);};
    }
    return None;
}

struct Fhp<'a> {
    end: usize,
    start: usize,
    path: Vec<&'a Vec<Vec<usize>>>,
    alr_seen: HashSet<&'a Vec<usize>>,
}

impl Fhp<'_> {
    fn show(&self) {
        for tubing in &self.path {
            println!("{:?}", tubing);
        }
    }
}

struct Flipgraph<'a>{
    g: &'a Graph,
    vertices: Vec<&'a Vec<Vec<usize>>>,
    neighboorlist: Vec<Vec<&'a Vec<Vec<usize>>>>,
}
impl Flipgraph<'_> {

    fn find_fhp_rand(&self, tries: usize) -> Option<Fhp> {
        let res: Result<Vec<usize>, Fhp> =
        (0..tries)
        .collect::<Vec<_>>()
        .par_iter()
        .map(|_|
        self.try_for_one_fhp())
        .collect();
        match res {
            Ok(_) => None,
            Err(path) => Some(path),
        }
    }

    fn try_for_one_fhp(&self) -> Result<usize, Fhp> {
        let mut flips = (0..self.g.vertices.len()-1).collect::<Vec<_>>();
        let mut flipped: bool;
        flipped = true;
        let start = rand::thread_rng().gen_range(0..self.vertices.len());
        let mut path = Fhp {
            end: start,
            start: start,
            path: Vec::new(),
            alr_seen: HashSet::new(),
        };
        for tube in self.vertices[start].iter() {
            path.alr_seen.insert(tube);
        }
        path.path.push(&self.vertices[path.start]);
        while flipped {
            if path.alr_seen.len() == self.g.tubes.as_ref().unwrap().len() {
                return Err(path);
            }
            flipped = false;
            flips.shuffle(&mut rand::thread_rng());
            for flip in flips.iter() {
                let newtube = new_tube(&self.neighboorlist[path.end][*flip], &self.vertices[path.end]).unwrap();
                if !path.alr_seen.contains(newtube) {
                    path.alr_seen.insert(newtube);

                    path.end = self.vertices.iter().position(|&r| r == self.neighboorlist[path.end][*flip]).unwrap();
                    path.path.push(self.vertices[path.end]);
                    flipped = true;
                    break;
                }
            }
        }

        Ok(0)
    }

    fn find_fhc_rand(&self, tries: usize) -> Option<Fhp> {
        let res: Result<Vec<usize>, Fhp> =
        (0..tries)
        .collect::<Vec<_>>()
        .par_iter()
        .map(|_|
        self.try_for_one_fhc())
        .collect();
        match res {
            Ok(_) => None,
            Err(path) => Some(path),
        }
    }

    fn try_for_one_fhc(&self) -> Result<usize, Fhp> {
        let mut flips = (0..self.g.vertices.len()-1).collect::<Vec<_>>();
        let mut flipped: bool;
        flipped = true;
        let start = rand::thread_rng().gen_range(0..self.vertices.len());
        let mut path = Fhp {
            end: start,
            start: start,
            path: Vec::new(),
            alr_seen: HashSet::new(),
        };
        path.path.push(&self.vertices[path.start]);
        while flipped {
            if path.alr_seen.len() == self.g.tubes.clone().unwrap().len() && path.start == path.end {
                return Err(path);
            }
            flipped = false;
            flips.shuffle(&mut rand::thread_rng());
            for flip in flips.iter() {
                let newtube = new_tube(&self.neighboorlist[path.end][*flip], &self.vertices[path.end]).unwrap();
                if !path.alr_seen.contains(newtube) {
                    path.alr_seen.insert(newtube);

                    path.end = self.vertices.iter().position(|&r| r == self.neighboorlist[path.end][*flip]).unwrap();
                    path.path.push(self.vertices[path.end]);
                    flipped = true;
                    break;
                }
            }
        }
        Ok(0)
    }
}
struct Graph {
    vertices: Vec<usize>,
    edges: Vec<[usize; 2]>,
    tubes: Option<HashSet<Vec<usize>>>,
    tubings: Option<HashSet<Vec<Vec<usize>>>>,
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
    fn find_tubes(&mut self) {
        match self.tubes {
            Some(_) => {return;}
            None => {
                let mut tubes = HashSet::new();
                for subset in subsets(&self.vertices).iter() {
                    if self.is_connected(&subset) {tubes.insert(subset.to_vec());}
                }
                self.tubes = Some(tubes);
            }
        }
        return;
    }
    fn find_tubings(&mut self) {
        match self.tubings {
            Some(_) => return,
            None => {
                let mut tubings = HashSet::new();
                for v in self.vertices.iter() {
                    tubings.insert(vec![vec![*v]]);
                }
                let mut new = HashSet::new();
                let mut to_add;
                for _i in 2..self.vertices.len() {
                    for partial_tubing in tubings {
                        for new_partial_tubing in compatible_with(&partial_tubing, self).iter() {
                            to_add = new_partial_tubing.clone();
                            to_add.sort();
                            new.insert(to_add);
                        }
                    }
                    tubings = new.clone();
                    new.clear();
                }
                self.tubings = Some(tubings);
            }
        }
    }
}

pub fn get_jobs(source: &str) {

}

fn main() {
    let args: Vec<String> = env::args().collect();
    let source = &args[1];
    let paths_or_cycles = &args[2];
    //    let n = args[3].parse::<usize>().unwrap();
    let tries = args[3].parse::<usize>().unwrap();
    //    let save = &args[4];
    let talk = &args[4];

    //    let mut file = OpenOptions::new().write(true).truncate(true).open(save).expect("failed to open file");

    let mut graphs: Vec<Graph> = Vec::new();

    for edgelist in read_to_string(source).expect("Read failed.").lines() {
        let edges_as_strings: Vec<&str> = edgelist[2..edgelist.len()-2].split("), (").collect();
        let mut edges: Vec<[usize; 2]> = Vec::new();
        for edge_str in edges_as_strings.iter() {
            let edge_vec: Vec<usize> = edge_str.split(", ").map(|r| r.parse::<usize>().unwrap()).collect();
            edges.push([edge_vec[0], edge_vec[1]]);
        }
        let mut vertices = Vec::new();
        for [a, b] in edges.iter() {
            if !vertices.contains(a) {
                vertices.push(*a);
            }
            if !vertices.contains(b) {
                vertices.push(*b);
            }
        }
        graphs.push(
            Graph {
                vertices: vertices,
                edges: edges,
                tubes: None,
                tubings: None,
            })
    }
