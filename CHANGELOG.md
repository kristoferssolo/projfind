## [0.2.0] - 2026-08-23

### 🚀 Features

- *(config)* Load project discovery settings from XDG config
- *(scan)* Follow symlinked directories while searching
- *(output)* Shorten printed paths under home to ~

### 🐛 Bug Fixes

- *(commands)* Match workspace manifests anywhere in the file
- *(bench)* Silence fixture setup during benchmark discovery
- *(config)* Expand a leading tilde in search paths

### 🚜 Refactor

- *(marker)* Replace infallible FromStr with From<&str>
- *(dependencies)* Store fd path as PathBuf
- *(errors)* Give every failure a typed variant
- *(finder)* Collapse four copies of the root ascent into one
- *(finder)* Express nested-project detection as a predicate
- *(config)* [**breaking**] Model an unlimited result count as None
- *(main)* Report the full error chain with color-eyre
- Use color-eyre in benchmarks and tests
- Trim redundant code comments
- Separate scanning from root resolution
- Load built-in defaults from TOML
- Rename project-finder to projfind

### 📚 Documentation

- *(readme)* Document root resolution and the dev workflow
- Tighten README
- *(config)* Explain marker groups and workspace files

### ⚡ Performance

- Scan repositories and markers concurrently
- Scan once and resolve project roots without quadratic rescans

### 🎨 Styling

- *(benches)* Clear the outstanding clippy warnings

### 🧪 Testing

- Replace the placeholder test with end-to-end coverage
- Expand discovery coverage and benchmarks
- Use valid git repository fixtures

### ⚙️ Miscellaneous Tasks

- *(justfile)* Add recipes for the everyday tasks
- *(justfile)* Add single-letter aliases
- Bump version number
- Modernize validation and release workflows
- Streamline project documentation and tooling
- Run tests with nextest
- Audit dependencies
- Use audit action in continuous integration
- Improve Rust build speed
## [0.1.2] - 2025-04-09

### 🚀 Features

- *(snapshot)* Add filesystem snapshot creator

### 🐛 Bug Fixes

- Remove clippy warnings
- Add 'fdfind' binary name

### 🚜 Refactor

- Separate main function
- Benchmarks
- Use anyhow::Result
- Impl Display and Default

### 📚 Documentation

- Add docstrings

### 🧪 Testing

- *(bench)* Add basic benchmark

### ⚙️ Miscellaneous Tasks

- Add fixture example
- Install fd-find
## [0.1.1] - 2025-03-21

### 🚀 Features

- Improve execution speed
## [0.1.0] - 2025-03-20

### 🚀 Features

- Implement main functionality

### 🚜 Refactor

- Mitigate some issues

### 📚 Documentation

- Add readme

### ⚙️ Miscellaneous Tasks

- Add initial CI/CD
- Fix errors
