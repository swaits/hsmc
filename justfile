# justfile for the hsmc workspace (hsmc + hsmc-macros)
# High-level development and CI workflows

# =============================================================================
# Configuration
# =============================================================================

# Treat all warnings as errors
rustflags := "-D warnings"

# Code coverage target percentage (fail if below this threshold)
coverage_target := "80"

# Timeout for coverage analysis in seconds
coverage_timeout := "600"

# =============================================================================

# Default recipe - show available recipes
default:
    @just --list

# =============================================================================
# Primary Development Workflows
# =============================================================================

# Quick development loop: format + lint + fast tests (default features only)
dev:
    #!/usr/bin/env bash
    set -e
    echo "🔄 Running development checks..."
    cargo fmt
    RUSTFLAGS="{{rustflags}}" cargo clippy --workspace --all-targets &
    RUSTFLAGS="{{rustflags}}" cargo nextest run --workspace &
    wait
    echo "✅ Development checks passed"

# Pre-commit checks across all feature combinations (tokio/embassy are mutually exclusive)
check:
    @echo "Running format check..."
    cargo fmt --all -- --check
    @echo ""
    @echo "Running clippy on all feature combinations..."
    RUSTFLAGS="{{rustflags}}" cargo clippy --workspace --all-targets --no-default-features
    RUSTFLAGS="{{rustflags}}" cargo clippy --workspace --all-targets --features tokio
    RUSTFLAGS="{{rustflags}}" cargo clippy --workspace --all-targets --features embassy
    @echo ""
    @echo "Verifying compilation on all feature combinations..."
    RUSTFLAGS="{{rustflags}}" cargo test --workspace --no-default-features --no-run
    RUSTFLAGS="{{rustflags}}" cargo test --workspace --features tokio --no-run
    RUSTFLAGS="{{rustflags}}" cargo test --workspace --features embassy --no-run
    @echo ""
    @echo "✅ All checks passed"

# Run all tests with comprehensive feature coverage
test:
    @echo "Running tests with all feature combinations..."
    RUSTFLAGS="{{rustflags}}" cargo nextest run --workspace --no-default-features
    RUSTFLAGS="{{rustflags}}" cargo nextest run --workspace --features tokio
    RUSTFLAGS="{{rustflags}}" cargo nextest run --workspace --features embassy
    @echo ""
    @echo "Running doctests..."
    cargo test --doc --workspace
    @echo "✅ All feature combinations tested"

# Before pushing: check + test + coverage
pre-push: check test coverage
    @echo ""
    @echo "✅ Pre-push checks passed - safe to push!"

# Comprehensive pre-release validation
pre-release: check test coverage audit mutants examples
    #!/usr/bin/env bash
    set -e

    # Verify documentation version matches crate version
    echo ""
    echo "📋 Verifying documentation version..."
    VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
    MAJOR_MINOR=$(echo $VERSION | cut -d. -f1-2)

    # README uses the braced form: `hsmc = { version = "X.Y", ... }`
    if ! grep -Eq "hsmc = \{[^}]*version = \"$MAJOR_MINOR(\.[0-9]+)?\"" README.md; then
        echo "❌ ERROR: README.md should reference version $MAJOR_MINOR (crate is $VERSION)"
        echo "   Update the install snippet in README.md"
        exit 1
    fi
    echo "✅ Documentation version correct: $MAJOR_MINOR"

    # Workspace publish dry-run: hsmc-macros must publish before hsmc
    echo ""
    echo "📦 Verifying packages..."
    cargo publish --dry-run -p hsmc-macros
    cargo publish --dry-run -p hsmc
    cargo package --list -p hsmc-macros > /tmp/hsmc-macros-files.txt
    cargo package --list -p hsmc         > /tmp/hsmc-files.txt
    echo "✅ Packages verified"

    echo ""
    echo "✅ All pre-release checks passed!"
    echo "  - Code formatted and linted"
    echo "  - All tests passing on {default, tokio, embassy}"
    echo "  - Code coverage ≥ {{coverage_target}}%"
    echo "  - No security vulnerabilities"
    echo "  - Mutation testing complete"
    echo "  - All examples run / build successfully"
    echo "  - Documentation version verified"
    echo "  - Packages ready for publication"
    echo ""
    echo "📦 Ready to publish! Run (in order):"
    echo "     cargo publish -p hsmc-macros"
    echo "     cargo publish -p hsmc"

# =============================================================================
# Quality & Security
# =============================================================================

# Run code coverage analysis (tokio feature covers the widest surface area)
coverage:
    @echo "Running code coverage analysis (target: {{coverage_target}}%)..."
    cargo llvm-cov nextest --workspace --features tokio --html --output-dir coverage --fail-under-lines {{coverage_target}}
    @echo "Coverage report: coverage/html/index.html"

# Mutation testing with cargo-mutants
mutants:
    @echo "Running mutation testing..."
    @echo "⚠️  This will use significant CPU and memory resources!"
    cargo mutants \
        --no-shuffle \
        --test-tool nextest \
        --jobs 3 \
        --features tokio \
        -vV

# Security audit + dependency checks
audit:
    @echo "🔒 Running security checks..."
    cargo audit
    @echo "✅ Security checks passed"

# Run Miri for UB detection (default features only; tokio/embassy rely on runtime primitives Miri does not support)
miri:
    @echo "Running Miri tests for UB detection (no-default-features)..."
    cargo +nightly miri test --workspace --no-default-features

# =============================================================================
# Documentation & Utilities
# =============================================================================

# Build documentation and open in browser
doc:
    cargo doc --no-deps --workspace --features tokio --open

# Format code
fmt:
    cargo fmt --all

# Show project information
info:
    @echo "hsmc workspace"
    @echo "  hsmc         - Hierarchical state machines (statecharts) with a declarative proc macro"
    @echo "  hsmc-macros  - Procedural macro implementation for the hsmc crate"
    @echo ""
    @echo "Rust: $(rustc --version)"
    @echo "Cargo: $(cargo --version)"
    @echo ""
    @echo "Dependencies:"
    @cargo tree --depth 1 -p hsmc

# Run all examples (tokio examples execute; embassy_full is a lib crate, build-only)
examples:
    @echo "Running tokio examples..."
    cargo run --example microwave     --features tokio
    cargo run --example during_radio  --features tokio
    @echo ""
    @echo "Building embassy_full example (lib crate)..."
    cargo build --example embassy_full --features embassy
    @echo "✅ All examples completed successfully"

# Clean build artifacts
clean:
    cargo clean
    rm -rf mutants.out/ mutants.out.old/ mutation_history.json
    rm -rf coverage/
