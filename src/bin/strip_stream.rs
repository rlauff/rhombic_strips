//! strip-stream: NDJSON streaming search for remote execution.
//!
//! This is the remote twin of `www/worker.js`. It reads one job description
//! from the first line of stdin and streams newline-delimited JSON messages
//! to stdout — the exact message shapes `app.js#onWorkerMessage` already
//! understands, so the browser treats an SSH pipe and a Web Worker alike:
//!
//!   stdin  (first line): {"graph": <WireGraph>, "cyclic": bool,
//!                         "mode": "exists"|"count"|"enumerate", "cap": 512}
//!   stdout (per line):   {"type":"note","message":...}
//!                        {"type":"strips","strips":[...],"count":n}
//!                        {"type":"progress","count":n}
//!                        {"type":"done","count":n,"capped":bool}
//!                        {"type":"error","message":...}
//!
//! Native perks over the wasm build: `count` and `exists` run rayon-parallel
//! over the Hamiltonian paths of level 0 (`rhombic::count_strips` semantics,
//! reimplemented here with a live counter for progress lines). `enumerate`
//! stays sequential — it streams strips in a stable order — but on a big
//! node, with a `cap` so a one-shot HTTP relay can't be flooded.
//!
//! Cancellation: the process dies with the pipe. When ssh (or the CGI relay
//! behind it) goes away, writes fail and we exit; srun then tears down the
//! allocation. A `{"cmd":"cancel"}` line on stdin also exits, for interactive
//! bridges that keep stdin open.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use rhombic_strips::lattice::{FaceId, Lattice};
use rhombic_strips::plotting;
use rhombic_strips::rhombic::{extensions, strips};
use rhombic_strips::web::api::{wire_to_faces, WireGraph};

// -- wire messages (match worker.js) -----------------------------------------

#[derive(Deserialize)]
struct Job {
    graph: WireGraph,
    #[serde(default)]
    cyclic: bool,
    mode: String,
    /// Enumerate only: stop after this many strips (0 = unlimited).
    #[serde(default)]
    cap: usize,
}

#[derive(Serialize)]
struct StripOut {
    layers: Vec<Vec<FaceId>>,
    edges: Vec<(FaceId, FaceId)>,
    #[serde(rename = "cyclicEdges")]
    cyclic_edges: Vec<(FaceId, FaceId)>,
}

fn emit(v: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{}", v).and_then(|_| out.flush()).is_err() {
        // Reader hung up (browser cancelled, ssh died): stop computing.
        std::process::exit(0);
    }
}

fn note(msg: &str) {
    emit(&serde_json::json!({"type": "note", "message": msg}));
}

fn fail(msg: &str) -> ! {
    emit(&serde_json::json!({"type": "error", "message": msg}));
    std::process::exit(1);
}

fn main() {
    let stdin = std::io::stdin();
    let mut first = String::new();
    if stdin.lock().read_line(&mut first).unwrap_or(0) == 0 {
        fail("no job on stdin");
    }
    let job: Job = match serde_json::from_str(first.trim()) {
        Ok(j) => j,
        Err(e) => fail(&format!("bad job JSON: {}", e)),
    };

    let faces = match wire_to_faces(&job.graph) {
        Ok(f) => f,
        Err(e) => fail(&e),
    };
    let lattice = Lattice::from_faces(faces);
    let cyclic = job.cyclic;

    // stdin watcher: exit on {"cmd":"cancel"}; ignore everything else
    // (including EOF — a one-shot relay closes stdin right after the job).
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { return };
            if line.contains("\"cancel\"") {
                std::process::exit(130);
            }
        }
    });

    let threads = rayon::current_num_threads();
    match job.mode.as_str() {
        "count" => run_count(&lattice, cyclic, threads),
        "exists" => run_exists(&lattice, cyclic, threads),
        "enumerate" => run_enumerate(&lattice, cyclic, job.cap),
        m => fail(&format!("unknown mode '{}'", m)),
    }
}

/// Progress ticker: reports `counter` once a second until `done`.
/// Doubles as a keep-alive so HTTP relays in the middle don't time out.
fn spawn_ticker(counter: Arc<AtomicUsize>, done: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1000));
            if done.load(Ordering::Relaxed) {
                break;
            }
            emit(&serde_json::json!({
                "type": "progress",
                "count": counter.load(Ordering::Relaxed),
            }));
        }
    })
}

/// Parallel count: `rhombic::count_strips`, unrolled so every found strip
/// bumps a shared counter the ticker can read.
fn run_count(l: &Lattice, cyclic: bool, threads: usize) {
    note(&format!("counting on {} threads…", threads));
    let counter = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let ticker = spawn_ticker(counter.clone(), done.clone());

    let max_dim = l.dim();
    let total: usize = if max_dim == 0 {
        l.ham_paths(cyclic)
            .map(|_| {
                counter.fetch_add(1, Ordering::Relaxed);
                1
            })
            .sum()
    } else {
        l.ham_paths(cyclic)
            .par_bridge()
            .map(|path| {
                extensions(vec![path], l, max_dim, cyclic)
                    .map(|_| {
                        counter.fetch_add(1, Ordering::Relaxed);
                        1usize
                    })
                    .sum::<usize>()
            })
            .sum()
    };

    done.store(true, Ordering::Relaxed);
    let _ = ticker.join();
    emit(&serde_json::json!({"type": "done", "count": total, "capped": false}));
}

/// Parallel existence: first strip found by any thread, with its skeleton so
/// the browser can display it (native `strip_exists` only returns a bool).
fn run_exists(l: &Lattice, cyclic: bool, threads: usize) {
    note(&format!("searching on {} threads…", threads));
    let tried = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let ticker = spawn_ticker(tried.clone(), done.clone());

    let max_dim = l.dim();
    let found = l
        .ham_paths(cyclic)
        .par_bridge()
        .find_map_any(|path| {
            tried.fetch_add(1, Ordering::Relaxed);
            extensions(vec![path], l, max_dim, cyclic).next()
        });

    done.store(true, Ordering::Relaxed);
    let _ = ticker.join();

    let count = match found {
        Some(strip) => {
            let (edges, cyclic_edges) = plotting::edges_strip(&strip, l, cyclic);
            emit(&serde_json::json!({
                "type": "strips",
                "strips": [StripOut { layers: strip, edges, cyclic_edges }],
                "count": 1,
            }));
            1
        }
        None => 0,
    };
    emit(&serde_json::json!({"type": "done", "count": count, "capped": false}));
}

/// Sequential streaming enumeration, batched like the wasm worker
/// (a strips message at most every ~30 ms), capped for one-shot relays.
fn run_enumerate(l: &Lattice, cyclic: bool, cap: usize) {
    let cap = if cap == 0 { usize::MAX } else { cap };
    let mut count = 0usize;
    let mut batch: Vec<StripOut> = Vec::new();
    let mut last_flush = std::time::Instant::now();
    let mut capped = false;

    let flush = |batch: &mut Vec<StripOut>, count: usize| {
        if batch.is_empty() {
            emit(&serde_json::json!({"type": "progress", "count": count}));
        } else {
            emit(&serde_json::json!({
                "type": "strips",
                "strips": std::mem::take(batch),
                "count": count,
            }));
        }
    };

    for strip in strips(l, cyclic) {
        count += 1;
        let (edges, cyclic_edges) = plotting::edges_strip(&strip, l, cyclic);
        batch.push(StripOut { layers: strip, edges, cyclic_edges });

        if last_flush.elapsed() >= Duration::from_millis(30) || batch.len() >= 8 {
            flush(&mut batch, count);
            last_flush = std::time::Instant::now();
        }
        if count >= cap {
            capped = true;
            break;
        }
    }
    flush(&mut batch, count);
    if capped {
        note(&format!("stopped at the first {} strips (raise the cap to get more)", cap));
    }
    emit(&serde_json::json!({"type": "done", "count": count, "capped": capped}));
}
