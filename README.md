# mekle

The name `mekle` comes from Latvian *meklē* – "search" or "look for".

Find coding projects below one or more directories. `mekle` recognizes Git
repositories and common markers such as `Cargo.toml`, `package.json`, and
`pyproject.toml`, then prints project roots ordered by recent and frequent use.

It respects ignore files, skips `.git` contents, and requires
[`fd`](https://github.com/sharkdp/fd) (`fdfind` on Debian and Ubuntu).

## Install

```bash
cargo install mekle
```

## Usage

```text
mekle [OPTIONS] [PATHS]... [COMMAND]
```

- `-d, --depth <DEPTH>` limits traversal depth (default: `5`).
- `-n, --max-results <MAX_RESULTS>` limits output.
- `-v, --verbose` prints progress to stderr.
- `PATHS` replaces configured search directories (default: `.`).

Paths under `$HOME` are printed as `~/...`.

```bash
mekle
mekle --depth 3 ~/src
mekle --verbose ~/src ~/work
mekle add ~/src/mekle
```

## Project ranking

`mekle add <PATH>` records a project visit. The normal project list puts
recorded projects first, ordered by frecency. Untracked projects remain sorted
by path. Shell and picker integrations should call `add` after selecting a
project because `mekle` does not observe directory changes itself.

Each project starts with a score of `1`, and every later visit adds `1`. The
time since the last visit adjusts that score:

| Last visit | Weight |
| --- | ---: |
| Less than one hour ago | `score * 4` |
| Less than one day ago | `score * 2` |
| Less than one week ago | `score / 2` |
| One week ago or older | `score / 4` |

When the sum of stored scores exceeds `10,000`, mekle reduces all scores to
about 90 percent of that limit and removes entries that fall below `1`.

History is stored at `$XDG_DATA_HOME/mekle/history.toml`, falling back to
`$HOME/.local/share/mekle/history.toml`.

### Managing history

```bash
mekle history list
mekle history show ~/src/mekle
mekle history set ~/src/mekle 20
mekle history adjust ~/src/mekle 5
mekle history adjust ~/src/mekle -5
mekle history remove ~/src/mekle
mekle history clear
```

`list` and `show` print tab-separated raw score, weighted score, time since the
last visit, and path. `set` creates a missing entry. `adjust` requires an
existing entry and removes it when the result falls below `1`. `clear` removes
the entire history.

## Shell completions

`mekle completions <SHELL>` generates completions for Bash, Elvish, Fish, or
Zsh. With `bash-completion` installed, add the Bash script to its per-user
completion directory:

```bash
mekle completions bash
```

Start a new shell to load it.

## Configuration

Built-in defaults come from [`config/config.toml`](config/config.toml). User
configuration is read from `$XDG_CONFIG_HOME/mekle/config.toml`, falling back
to `$HOME/.config/mekle/config.toml`.

Configured fields replace their defaults; command-line options override both.
A leading `~` in `search_dirs` expands to `$HOME`.

```toml
search_dirs = ["~/src", "/home/me/work"]
depth = 5
marker_files = ["Cargo.toml", "package.json", "pyproject.toml"]
workspace_files = ["pnpm-workspace.yaml", "lerna.json"]
```

## Root resolution

`Cargo.toml`, `package.json`, and Deno manifests resolve to their workspace or
Git root. Build files resolve to the highest matching ancestor before a Git
boundary. Other markers resolve to their enclosing Git repository, or their
own directory when none exists.

Direct children of a project remain separate results; deeper descendants are
folded into their ancestor.
