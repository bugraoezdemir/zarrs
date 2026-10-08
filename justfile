TOOLCHAIN := "stable"
export RUST_BACKTRACE := "0"

# Display the available recipes
help:
    @just --list --unsorted

# Build (cargo check) with all / default / no default features
build:
    cargo +{{TOOLCHAIN}} check
    cargo +{{TOOLCHAIN}} check --all-features
    cargo +{{TOOLCHAIN}} check --no-default-features

# Test with all features
test *args:
    cargo +{{TOOLCHAIN}} test --all-features {{args}}
    cargo +{{TOOLCHAIN}} test --all-features --examples {{args}}

# Run ignored tests
test_ignored:
    cargo +{{TOOLCHAIN}} test --all-features -- --ignored

# Format with rustfmt
fmt:
    cargo +nightly fmt

# Lint with clippy
clippy:
    cargo +{{TOOLCHAIN}} clippy --all-features --all-targets -- -D warnings -A clippy::pedantic

# Generate documentation
doc:
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +nightly doc -Z unstable-options -Z rustdoc-scrape-examples --all-features --no-deps

# Build/test/clippy/doc/check formatting - recommended before a PR
check: build test clippy doc
    cargo +nightly fmt --all -- --check

# Build (WASM)
build_wasm:
    cargo check -p zarrs --target wasm32-unknown-unknown --no-default-features --features "ndarray crc32c gzip transpose async"

# Build/clippy (WASM)
check_wasm: build_wasm
    cargo clippy -p zarrs --target wasm32-unknown-unknown --no-default-features --features "ndarray crc32c gzip transpose async" -- -A clippy::arc_with_non_send_sync

# Run clippy with extra lints
_clippy_extra:
    cargo +{{TOOLCHAIN}} clippy --all-features -- -D warnings -W clippy::nursery -A clippy::significant-drop-tightening -A clippy::significant-drop-in-scrutinee

_miri:
    MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks -Zmiri-tree-borrows" cargo +{{TOOLCHAIN}} miri test -p zarrs --all-features

_coverage_install:
    cargo install cargo-llvm-cov --locked

_coverage_report:
    cargo +{{TOOLCHAIN}} llvm-cov --all-features --doctests --html

_coverage_file:
    cargo +{{TOOLCHAIN}} llvm-cov --all-features --doctests --lcov --output-path lcov.info

# Test data compatibility with the latest zarrs release (as in CI)
regression *args:
    cargo +{{TOOLCHAIN}} run -p zarrs_regression_testing -- {{args}}

# Determine how far back data compatibility extends across all tested zarrs releases (requires a nightly toolchain to resolve dependencies as of each release)
regression_all *args:
    cargo +{{TOOLCHAIN}} run -p zarrs_regression_testing -- --all {{args}}

# Write an HTML report of data compatibility testing (pass --all to include all releases) and open it
regression_report *args:
    -cargo +{{TOOLCHAIN}} run -p zarrs_regression_testing -- --html target/zarrs_regression_testing/report.html {{args}}
    {{ if os() == "macos" { "open" } else { "xdg-open" } }} target/zarrs_regression_testing/report.html

# Remove regression testing helpers and work directories
regression_clean:
    rm -rf target/zarrs_regression_testing

