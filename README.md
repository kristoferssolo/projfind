# mekle

The name `mekle` comes from Latvian *meklē* – "search" or "look for".

Find coding projects below one or more directories. `mekle` recognizes Git
repositories and common markers such as `Cargo.toml`, `package.json`, and
`pyproject.toml`, then prints project roots ordered by recent and frequent use.
A repository is either a directory holding `.git`, or a worktree or submodule
whose `.git` file redirects to an existing Git directory.

It respects ignore files, skips `.git` contents, and needs nothing installed
beyond the binary itself.

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
- `--json` prints one JSON object per line.
- `-0, --null` prints uncontracted paths separated by NUL bytes.
- `--exclude <PATTERN>` skips entries matching a gitignore-style pattern,
  relative to each search directory. Repeatable, and appended to the
  configured `exclude` list.
- `PATHS` replaces configured search directories (default: `.`).

The default output prints one path per line and shortens paths under `$HOME` to
`~/...`. JSON and NUL output do not shorten paths.

```bash
mekle
mekle --depth 3 ~/src
mekle --verbose ~/src ~/work
mekle --json ~/src
mekle -0 ~/src | xargs -0 -n1 printf '%s\n'
mekle --exclude target/ --exclude '**/vendor/' ~/src
mekle add ~/src/mekle
mekle pin ~/src/mekle
```

JSON output is newline-delimited. Each record has `path`, `score`, `frecency`,
`last_used`, `pinned`, and `markers`. `last_used` is a Unix timestamp, or
`null` for a project that is not in the history. Untracked projects have a
score and frecency of `0`, and a pinned directory that discovery never
classified has an empty `markers` list.

## Project ranking

`mekle add <PATH>` records a project visit. The normal project list puts pinned
projects first, then the remaining recorded projects ordered by frecency.
Untracked projects remain sorted by path.

`mekle init <SHELL>` defines `m`, which lists projects in `fzf`, changes to the
selected directory, and records the visit. Add the command for your shell to
its startup file:

```bash
# Bash and Zsh
eval "$(mekle init bash)"
eval "$(mekle init zsh)"

# Fish
mekle init fish | source

# Elvish
eval (mekle init elvish | slurp)
```

`m` accepts the same paths and options as `mekle`, for example `m ~/src`.
The integration requires [`fzf`](https://github.com/junegunn/fzf).

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

### Pinned projects

Frecency drifts, so a project you rely on but have not opened this week sinks.
Pinning holds it in place instead.

```bash
mekle pin ~/src/mekle
mekle unpin ~/src/mekle
```

Both commands take any directory inside a project and resolve it to the same
root `mekle add` would record, so `mekle pin .` works from anywhere in the
tree. A pinned project ranks above every unpinned one whatever its frecency,
pinned projects rank against each other by frecency, and aging never drops a
pinned project. Pinning a project mekle has not seen records it with a score of
`1`; unpinning leaves the score and the last visit alone.

A pin is listed whether or not the search would have found it. That covers a
project outside every search directory, and a plain directory that holds no
marker file at all, so `mekle pin ~/notes` puts `~/notes` at the top of the
list. Such a directory is reported with an empty `markers` list, since nothing
classified it as a project. A pin whose directory no longer exists is left out
of the listing rather than reported; `mekle history prune` clears it for good,
and `mekle history remove` drops a pinned entry like any other.

Because an explicit pin outranks a pattern, a pinned project is listed even
when `exclude` would have skipped it.

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
mekle history prune
mekle history clear
```

`list` and `show` print tab-separated raw score, weighted score, time since the
last visit, `pinned` or `-`, and path. `set` creates a missing entry. `adjust` requires an
existing entry and removes it when the result falls below `1`. `prune` removes
entries whose project paths no longer exist. `clear` removes the entire
history.

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
exclude = ["target/", "**/vendor/", "/archive/", "*.generated.toml"]
```

`exclude` holds gitignore-style patterns interpreted relative to each search
directory. Following gitignore rules, a pattern without a slash, such as
`*.generated.toml`, matches at any depth, while a pattern containing a slash,
such as `skip/Cargo.toml` or `/archive/`, matches only relative to the search
directory (`**/skip/Cargo.toml` matches at any depth). Excluded directories
are pruned and excluded files are skipped, on top of ignore files, so
exclusions can only remove results.

## Root resolution

`Cargo.toml`, `package.json`, and Deno manifests resolve to their workspace or
Git root. Build files resolve to the highest matching ancestor before a Git
boundary. Other markers resolve to their enclosing Git repository, or their
own directory when none exists.

Direct children of a project remain separate results; deeper descendants are
folded into their ancestor.
