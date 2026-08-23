# projfind

`projfind` scans directories for Git repositories and common project
files such as `Cargo.toml`, `package.json`, and `pyproject.toml`. It resolves
markers to their repository or workspace root and prints a sorted list of
paths. It scans multiple search directories concurrently.

Ignore rules are respected, so paths excluded by `.gitignore` (or `.ignore`)
are skipped, and the contents of `.git` are never walked.

## Install

projfind requires [`fd`](https://github.com/sharkdp/fd). Debian and Ubuntu
package it as `fdfind`, which projfind also recognizes.

```bash
cargo install projfind
```

## Usage

```text
projfind [OPTIONS] [PATHS]...
```

- `-d, --depth <DEPTH>` sets the maximum search depth. The built-in value is
  `5`.
- `-n, --max-results <MAX_RESULTS>` limits the number of printed paths. The
  built-in value is unlimited.
- `-v, --verbose` prints search progress.
- `PATHS` replaces the configured search directories. The built-in path is the
  current directory.

Printed paths under your home directory are shortened to `~/...`; paths
elsewhere stay absolute.

```bash
# Search the current directory.
projfind

# Search one directory to a depth of three.
projfind --depth 3 ~/src

# Search several directories and print progress.
projfind --verbose ~/src ~/work

# Print at most ten projects.
projfind --max-results 10
```

## Configuration

The binary embeds [`config/config.toml`](config/config.toml) as its built-in
configuration. At startup, it checks
`$XDG_CONFIG_HOME/projfind/config.toml`, or
`$HOME/.config/projfind/config.toml` when `XDG_CONFIG_HOME` is unset.

Every field is optional. A field in the user file replaces the corresponding
built-in value. Lists are replaced, not extended. Command-line arguments
override both. A leading `~` in `search_dirs` expands to `$HOME`.

```toml
search_dirs = [ "~/src", "/home/me/work" ]
depth = 5
verbose = false

# Omit this field for unlimited results.
# max_results = 100

marker_files = [
  "Cargo.toml",
  "package.json",
  "pyproject.toml",
]

workspace_files = [
  "pnpm-workspace.yaml",
  "lerna.json",
  "yarn.lock",
  ".yarnrc.yml",
  "workspace.json",
]
```

## Project roots

The directory containing a marker is not always the project root. Project
Finder uses these rules to choose the path it prints:

- `package.json`, `deno.json`, and `deno.jsonc` climb toward the filesystem root
  until they reach a JavaScript or Deno workspace, or a Git repository.
- `Cargo.toml` climbs until it reaches a manifest containing `[workspace]`, or a
  Git repository.
- `Makefile`, `CMakeLists.txt`, `justfile`, and `Justfile` resolve to the highest
  directory containing the same build file, without crossing a repository
  boundary.
- Other markers resolve to their enclosing Git repository. Without one, they
  stay in the marker's directory.

JavaScript and Deno workspace detection checks the configured
`workspace_files`. It also reads `package.json`, `deno.json`, `deno.jsonc`,
`bunfig.toml`, `Cargo.toml`, `rush.json`, `nx.json`, and `turbo.json` for known
workspace declarations.

projfind keeps a direct child of a discovered project as a separate
result. It treats anything nested more deeply as part of its parent.

## Development

The [justfile](justfile) contains the development commands. Run `just` to list
them.

```bash
just check # Run formatting, Clippy, tests, and docs as CI does.
just test  # Run tests only.
just run -d 3 ~/src
just bench # Benchmark; pass criterion arguments through, e.g. `just bench dedup`.
```

Benchmarks build their trees in code, so a run measures the same shapes on
every machine. `discovery` times a whole search, `fd` included; the rest
isolate projfind's own work: `root_resolution` compares a cold cache against a
warm one, `deep_ascent` times a single resolution against how far it has to
climb, and `dedup` checks that folding roots into the result set stays linear.
