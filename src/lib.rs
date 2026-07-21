//! Rhombic strips of graded posets — library crate.
//!
//! This is the shared core behind three front ends:
//!
//! * the command-line tool (`src/main.rs`),
//! * the desktop egui explorer ([`gui`], native only), and
//! * the browser build ([`web`], compiled to wasm for `www/`).
//!
//! The generator and strip-search logic that the website needs lives in
//! [`web::api`] as plain `Result<String, String>` functions, so it is unit
//! tested on the host (see `tests/generators.rs`) without a browser in the
//! loop. The `#[wasm_bindgen]` layer in [`web`] is a thin shell over it.

pub mod lattice;
pub mod rhombic;

/// TikZ/pdflatex rendering plus `edges_strip` (the strip's draw edges, used by
/// both the GUI and the browser). Compiled on every target: `edges_strip` is
/// pure, and the `std::process`/`std::fs` rendering paths compile for wasm too
/// (they're simply never called in the browser). If your `plotting.rs` pulls in
/// a *native-only crate* at module level and fails to compile for wasm, gate
/// those items with `#[cfg(not(target_arch = "wasm32"))]` and keep `edges_strip`
/// ungated.
pub mod plotting;

/// Desktop egui explorer. It pulls in `eframe` and spawns worker threads, so
/// it is excluded from the wasm build (the browser gets [`web`] instead) and
/// from `--no-default-features` builds (the headless cluster binary).
#[cfg(all(not(target_arch = "wasm32"), feature = "gui"))]
pub mod gui;

/// Browser bindings plus the host-testable [`web::api`] core.
pub mod web;

/// Batch "scripts" for the browser's Scripts panel: computations over
/// *families* of objects (all small graphs, all strips of one poset) rather
/// than a single search on the drawn diagram. Same architecture as [`web`]:
/// a pure, host-testable core plus thin sliceable `#[wasm_bindgen]` steppers.
pub mod scripts;
