# rhombic_strips

Rhombic strips of graded posets — a Rust library with three front ends: a CLI
(`src/main.rs`), a desktop egui explorer (`src/gui.rs`, `--interactive`), and a
browser build (`src/web.rs` → wasm, served from `www/`, built with `./build.sh`).

## Scripts

The collapsible **Scripts** panel in the sidebar runs two batch jobs, both
implemented in Rust (`src/scripts.rs`) and executed as sliceable wasm steppers
in a dedicated Web Worker:

* **Graph survey** — enumerates all connected graphs on up to *n* vertices
  (one per isomorphism class, by vertex augmentation with canonical-form
  deduplication), and checks each tube poset for rhombic strips and/or cyclic
  strips. Results are visualized as clickable graph thumbnails, optionally
  grouped by Hamiltonicity, probing the conjecture *Hamilton path ⇒ rhombic
  strip* (counterexamples would be flagged; the cyclic analogue is known to
  fail and the failing graphs are listed).
* **Strip boundaries** — enumerates every linear rhombic strip of the poset
  in the editor, reads its left and right boundary chains bottom-to-top as
  permutations (added-token order, e.g. vertex insertion order in a tube
  poset), and tallies the pairs (start&nbsp;→&nbsp;end) with multiplicities.

## Remote & native compute

The web page's strip search normally runs as wasm in the browser tab
(single-threaded). The **Browser / This machine / Cluster** switch in the
*Rhombic strips* panel adds two faster backends, both powered by the same
helper script and requiring no manual installation:

* **This machine** — a native `strip_stream` binary using all cores (rayon).
  Paste into a terminal:

  ```
  curl -fsSL https://raw.githubusercontent.com/rlauff/rhombic_strips/main/cluster/serve.sh | bash -s -- --local
  ```

* **Cluster (Slurm)** — jobs run via `srun` under *your own* account. The page
  templates the exact command from the login you enter; it is one ordinary ssh
  session that doubles as the tunnel:

  ```
  ssh -t -L 8642:127.0.0.1:8642 you@sshgate.math.tu-berlin.de \
    'curl -fsSL https://raw.githubusercontent.com/rlauff/rhombic_strips/main/cluster/serve.sh | bash -s -- --partition=math'
  ```

On first use the script installs a minimal Rust toolchain if needed, clones
this repo into `~/.cache/rhombic_strips`, and builds `strip_stream` headless
(`--no-default-features`, so no egui on login nodes). Subsequent runs start in
seconds. It then prints a **pairing code**; enter it once in the page (kept in
localStorage). Everything stops when the terminal closes — only the build
cache persists.

### How it fits together

```
browser (www/app.js) ──fetch NDJSON──► 127.0.0.1:8642 relay (cluster/relay.py)
                                            │  direct: strip_stream
                                            │  slurm:  srun … strip_stream
                        (cluster: the port is your ssh -L tunnel)
```

`src/bin/strip_stream.rs` reads one job from stdin and streams exactly the
message shapes `www/worker.js` produces, so the page treats an ssh pipe and a
Web Worker alike. Aborting the fetch kills the process group, which releases
the Slurm allocation.

Security model: the relay binds to 127.0.0.1 only; cluster access happens
through each user's own ssh login (keys/password/OTP stay in their terminal),
so nobody can spend anyone else's allocation. The pairing token prevents other
users on a shared login node — and web pages doing drive-by requests to
localhost — from submitting jobs through the relay.
