# Rhombic Strips in Lattices

A Rust project designed to find and analyze Rhombic Strips within a given Lattice. This tool offers both a Command Line Interface (CLI) for quick computations and a Graphical User Interface (GUI) for visual exploration.

## Usage

You can run this project in either Interactive Mode (GUI) or Command-Line Mode (CLI), depending on your needs.

### Interactive GUI Mode

To launch the graphical interface and interactively explore the lattice, run the following command. Note: This mode only shows the *first* found strip but allows you to freely explore it.

```bash
cargo run --release -- --interactive
```

### Command-Line Mode

To run the tool from the command line, pass the path to your lattice file followed by any of the desired configuration flags.

```bash
cargo run --release -- <path-to-lattice-file> [FLAGS]
```

## Available Flags

You can customize the behavior of the CLI by appending one or more of the following flags to your command:

* `--cyclic`
    Restricts the search to only cyclic rhombic strips.
* `--count`
    Finds all possible rhombic strips and prints their total number.
* `--show`
    Prints out the found strips (outputs the first one only).
* `--enumerate`
    Finds all rhombic strips and splits the total count among the Hamiltonian cycles.
* `--show-all`
    Displays all of the rhombic strips found in the lattice.
* `--show-cyclic`
    Shows all found strips formatted in a cyclic layout.
* `--interactive`
    Launches the GUI mode (as described above).
