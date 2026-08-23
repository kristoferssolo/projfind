# projfind

Find coding projects below one or more directories. `projfind` recognizes Git
repositories and common markers such as `Cargo.toml`, `package.json`, and
`pyproject.toml`, then prints sorted project roots.

It respects ignore files, skips `.git` contents, and requires
[`fd`](https://github.com/sharkdp/fd) (`fdfind` on Debian and Ubuntu).

## Install

```bash
cargo install projfind
```

## Usage

```text
projfind [OPTIONS] [PATHS]...
```

- `-d, --depth <DEPTH>` limits traversal depth (default: `5`).
- `-n, --max-results <MAX_RESULTS>` limits output.
- `-v, --verbose` prints progress to stderr.
- `PATHS` replaces configured search directories (default: `.`).

Paths under `$HOME` are printed as `~/...`.

```bash
projfind
projfind --depth 3 ~/src
projfind --verbose ~/src ~/work
```

## Configuration

Built-in defaults come from [`config/config.toml`](config/config.toml). User
configuration is read from `$XDG_CONFIG_HOME/projfind/config.toml`, falling back
to `$HOME/.config/projfind/config.toml`.

Configured fields replace their defaults; command-line options override both.
A leading `~` in `search_dirs` expands to `$HOME`.

```toml
search_dirs = [ "~/src", "/home/me/work" ]
depth = 5
marker_files = [ "Cargo.toml", "package.json", "pyproject.toml" ]
workspace_files = [ "pnpm-workspace.yaml", "lerna.json" ]
```

## Root resolution

`Cargo.toml`, `package.json`, and Deno manifests resolve to their workspace or
Git root. Build files resolve to the highest matching ancestor before a Git
boundary. Other markers resolve to their enclosing Git repository, or their
own directory when none exists.

Direct children of a project remain separate results; deeper descendants are
folded into their ancestor.
