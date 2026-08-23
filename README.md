# Project Finder

A command-line tool to discover coding projects in specified directories.
It identifies projects based on common marker files (e.g., `package.json`, `Cargo.toml`, `.git` directories).

## Goal

The goal of this project is to quickly and efficiently locate coding projects within a directory structure.
This is particularly useful for developers working in large codebases or managing multiple repositories.

## Features

* **Fast project discovery:** Quickly scans directories to identify potential projects.
* **Multiple project types:** Recognizes projects based on various marker files for different languages and build systems.
* **Configurable search depth:** Limits the search depth to improve performance.
* **Verbose output:** Provides detailed information about the search process.
* **Workspace Awareness:** Detects and handles workspace configurations correctly, such as Javascript and Rust workspaces.
* **Concurrency:** Uses asynchronous tasks to process multiple directories in parallel, improving performance.

## Requirements

To use Project Finder, you need the following dependencies installed on your system:

* **fd:** A simple, fast, and user-friendly alternative to `find`.
  * Installation instructions: [https://github.com/sharkdp/fd#installation](https://github.com/sharkdp/fd#installation)

`fd` must be available in your system's PATH.
Debian and Ubuntu package it as `fdfind`, which is also recognised.

## Installation

```bash
cargo install project-finder
```

## Usage

```bash
project-finder [OPTIONS] [PATHS]
```

### Options

* **-d, --depth <DEPTH>**: Maximum search depth (default: 5)
* **-n, --max-results <MAX_RESULTS>**: Maximum number of results to return (default: unlimited)
* **-v, --verbose**: Show verbose output
* **PATHS**: Directories to search for projects (default: ".")

### Examples

* Find projects in the current directory with the default depth:

  ```bash
  project-finder
  ```

* Find projects in a specific directory with a maximum depth of 3:

  ```bash
  project-finder --depth 3 /path/to/search
  ```

* Find projects in multiple directories with verbose output:

```bash
project-finder --verbose /path/to/search1 /path/to/search2
```

* Limit the number of results to 10:

```bash
project-finder --max-results 10
```

## How a project root is chosen

Project Finder reports the directory that owns a marker, not the directory the
marker sits in. Starting from the marker, it climbs the ancestors and stops at
the first of:

* a workspace root, for `package.json`, `deno.json` and `Cargo.toml` markers.
  A `pnpm-workspace.yaml`, `lerna.json`, `yarn.lock`, `.yarnrc.yml` or
  `workspace.json` marks one by existing; `package.json`, `deno.json`,
  `deno.jsonc`, `bunfig.toml`, `Cargo.toml`, `rush.json`, `nx.json` and
  `turbo.json` mark one when their contents say so.
* a directory holding `.git`.

Build files (`Makefile`, `CMakeLists.txt`, `justfile`, `Justfile`) instead
resolve to the highest directory holding the same build file, bounded by the
enclosing repository. Anything else resolves to the enclosing repository, or to
the marker's own directory when there is none.

A project nested one level inside another is reported separately, since a crate
inside a JavaScript monorepo is usually a project in its own right. Anything
deeper is treated as part of its parent.

## Use Cases

* **Quickly locating projects:** Easily find all projects within a large directory structure.
* **Managing multiple repositories:** Discover all repositories in a directory.
* **Automated scripting:** Integrate project discovery into scripts for build automation, testing, or deployment.
* **Workspace management:** Identify workspace roots for managing multiple related projects.

## Development

The [justfile](justfile) wraps the common tasks. `just` on its own lists them.

```bash
just check      # formatting, clippy, tests and docs, as CI runs them
just test       # tests only
just run -d 3 ~/src
just bench      # builds the release binary, then benchmarks against a fixture
```

Benchmarks replay a directory tree captured in `benches/fixtures`. Capture a
new one with `just snapshot <DIRECTORY>...`.

## License

This project is dual-licensed under either:

* MIT License ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

at your option.
