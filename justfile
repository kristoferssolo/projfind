alias a := audit
alias c := check
alias f := fmt
alias i := install
alias r := run
alias t := test

# List the available recipes.
default:
    @just --list

# Run the checks CI runs, in the order that fails fastest.
check: fmt-check clippy test doc

# Format the workspace.
fmt:
    cargo fmt --all

# Fail if anything is unformatted.
fmt-check:
    cargo fmt --all -- --check

# Lint with the pedantic and nursery groups denied, as CI does.
clippy:
    cargo clippy --workspace --all-features --all-targets -- --deny warnings

# Run the test suite.
test *ARGS:
    cargo nextest run --all-features {{ ARGS }}

# Build the documentation, private items included.
doc *ARGS:
    cargo doc --workspace --all-features --document-private-items --no-deps {{ ARGS }}

# Run the binary against PATHS, forwarding any flags.
run *ARGS:
    cargo run --release -- {{ ARGS }}

# Install the binary from this checkout.
install:
    cargo install --path . --locked

# Benchmark. The discovery group runs a real scan against temporary trees.
bench *ARGS:
    cargo bench --bench benchmark -- {{ ARGS }}

# Show what could move, including majors held back by the ranges in Cargo.toml.
outdated:
    cargo update --dry-run --verbose

# Update dependencies within the ranges in Cargo.toml.
update:
    cargo update

# Audit dependencies for known advisories. Needs `cargo install cargo-audit`.
audit:
    cargo audit

# Remove build artifacts.
clean:
    cargo clean

setup:
    cargo install cargo-nextest bacon cargo-audit
