# justfile for the hsmc workspace (hsmc + hsmc-macros + verification)
# High-level development and CI workflows.
#
# All recipes run inside `mise exec` so mise-managed tools (Rust with
# components + embedded targets, opam, cargo subcommands, the
# project's `.venv/`) are automatically on PATH. No `mise activate`
# needed; no per-recipe wrappers; no shebangs.
#
# Two settings, both routed through `mise exec --`:
#   set shell              — used per line for short recipes
#   set script-interpreter — used for `[script]` recipes that need
#                            multi-line state (loops, branches, etc.)

set shell              := ["mise", "exec", "--", "sh", "-cu"]
set script-interpreter := ["mise", "exec", "--", "sh", "-eu"]

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
[script]
dev:
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
[script]
pre-release: check test coverage audit mutants examples
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
    # `tokio,journal` so journal_* and det_* tests (gated on both
    # features) actually run — they're what catches mutants in
    # JournalSink and the codegen-emitted journal hooks.
    cargo mutants \
        --no-shuffle \
        --test-tool nextest \
        --jobs 3 \
        --features tokio,journal \
        -vV

# Security audit + dependency checks
audit:
    @echo "🔒 Running security checks..."
    cargo audit
    @echo "✅ Security checks passed"

# Run Miri for UB detection (auto-installs nightly + miri component if missing).
[script]
miri:
    if ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
        echo "  → installing nightly toolchain..."
        rustup toolchain install nightly --component miri,rust-src --profile minimal
    elif ! rustup component list --toolchain nightly --installed 2>/dev/null | grep -q '^miri'; then
        echo "  → adding miri component to nightly..."
        rustup component add miri --toolchain nightly
    fi
    echo "Running Miri tests for UB detection (no-default-features)..."
    # `--exclude hsmc-macros`: the macro crate's only integration test
    # is `tests/ui.rs` which uses `trybuild` (compiles fixture files,
    # globs the filesystem). That hits Miri's isolation sandbox and
    # tests proc-macro behavior, not UB. Skip it.
    # `--exclude hsmc-verification`: this crate is built with the
    # nightly pinned in verification/mise.toml (nightly-2025-11-13);
    # mixing toolchains under cargo +nightly miri test breaks builds.
    cargo +nightly miri test --workspace --no-default-features \
        --exclude hsmc-macros --exclude hsmc-verification

# Run Creusot deductive verification (self-installs opam, Why3, SMT solvers, cargo-creusot on first run).
[script]
verify:
    printf '🔍 Setting up Creusot verification stack...\n'

    # All the heavy machinery lives in `verification/`: a pinned nightly
    # with rustc-dev, an opam switch, a Python venv with tomli, etc.
    # `verification/mise.toml` declares it all. cd in, then re-eval mise
    # env so opam/uv/python/cargo on PATH are the verification ones.
    cd verification
    mise install
    eval "$(mise env -s bash)"
    PINNED_NIGHTLY="nightly-2025-11-13"

    # ── 1. opam init + switch (one-time) ─────────────────────────────
    # `--no-depexts` everywhere: opam's system-package check would ask
    # for things like `python-tomli` that need sudo on Arch. We supply
    # those via mise's project-local Python venv (with tomli pip-installed).
    if [ ! -d "$HOME/.opam" ]; then
        printf '  → initializing opam (one-time, ~2 min)...\n'
        opam init --yes --disable-sandboxing --bare --no-depexts
    fi
    if ! opam switch list -s 2>/dev/null | grep -qx 'hsmc-creusot'; then
        printf '  → creating opam switch hsmc-creusot with OCaml 5.x (~5 min)...\n'
        opam switch create hsmc-creusot 5.2.0 --yes --no-depexts
    fi
    eval "$(opam env --switch=hsmc-creusot --set-switch)"

    # ── 2. Python deps for opam build scripts (`tomli`) ──────────────
    # mise's `_.python.venv` auto-created `.venv/` here via uv, so the
    # venv has no pip — use `uv pip install` directly into $VIRTUAL_ENV.
    printf '  → uv pip install -r requirements.txt...\n'
    uv pip install --quiet -r requirements.txt

    # ── 3. Why3 + SMT solvers via opam (idempotent) ──────────────────
    # alt-ergo + z3 cover the vast majority of Creusot proofs. cvc5
    # opam build needs system GMP (sudo pacman -S gmp on Arch).
    NEED=""
    command -v why3 >/dev/null 2>&1     || NEED="$NEED why3"
    command -v alt-ergo >/dev/null 2>&1 || NEED="$NEED alt-ergo"
    command -v z3 >/dev/null 2>&1       || NEED="$NEED z3"
    if [ -n "$NEED" ]; then
        printf '  → opam install %s (slow — builds from source)...\n' "$NEED"
        # shellcheck disable=SC2086
        opam install --yes --no-depexts $NEED
    fi

    # ── 4. why3 config detect (registers solvers with Why3) ──────────
    if [ ! -f "$HOME/.why3.conf" ]; then
        printf '  → registering solvers with Why3...\n'
        why3 config detect
    fi

    # ── 5. Clone Creusot source + run canonical INSTALL ──────────────
    # creusot-install assumes it's running INSIDE the source tree (it
    # invokes `cargo run --bin prelude-generator` etc.). Clone, cd, run.
    # The pinned nightly comes from verification/mise.toml so cargo here
    # resolves to it directly — no RUSTUP_TOOLCHAIN gymnastics needed.
    CREUSOT_PIN="v0.9.0"
    CREUSOT_SRC="$HOME/.local/share/hsmc-verify/creusot-${CREUSOT_PIN}"
    CREUSOT_INSTALLED="$HOME/.local/share/creusot/why3find.json"
    if [ ! -d "$CREUSOT_SRC/.git" ]; then
        printf '  → git clone creusot @ %s...\n' "$CREUSOT_PIN"
        mkdir -p "$(dirname "$CREUSOT_SRC")"
        git clone --depth 1 --branch "$CREUSOT_PIN" \
            https://github.com/creusot-rs/creusot "$CREUSOT_SRC"
    fi
    if [ ! -f "$CREUSOT_INSTALLED" ]; then
        printf '  → running creusot ./INSTALL (~5 min — builds creusot-rustc)...\n'
        # `RUSTUP_TOOLCHAIN` forces the cloned source's cargo to use the
        # exact nightly mise pinned for verification/, regardless of what
        # the source's own `rust-toolchain` file says (they should match,
        # but be explicit).
        ( cd "$CREUSOT_SRC" && RUSTUP_TOOLCHAIN="$PINNED_NIGHTLY" cargo run --release --bin creusot-install )
    fi

    # ── 6. Run the actual verification ───────────────────────────────
    # `cargo creusot prove` invokes why3find which resolves `verif/`
    # relative to the workspace root (where why3find.json lives). cd
    # back so the path resolution is correct.
    #
    # Two phases:
    #   `cargo creusot`        — Rust → Coma IR, dumped into verif/
    #   `cargo creusot prove`  — Why3 + SMT solvers discharge VCs
    cd ..
    if [ ! -f why3find.json ]; then
        printf '  → cargo creusot init (one-time, populates why3find.json)...\n'
        cargo creusot init || true   # exits non-zero in workspaces but writes the file
    fi
    printf '\n🚀 cargo creusot (generate Coma IR)...\n'
    cargo creusot
    printf '\n🚀 cargo creusot prove --no-cache (force SMT solver invocations)...\n'
    # `--no-cache` skips why3find's per-Coma-file proof cache and
    # actually invokes alt-ergo + z3 + cvc4 for every VC. The wall-
    # clock vs CPU-time gap (`time` output) is the parallel SMT work.
    set +e
    time cargo creusot prove --no-cache
    PROVE_RC=$?
    set -e
    if [ "$PROVE_RC" -eq 0 ]; then
        printf '\n✅ All VCs discharged. See verification/INVARIANTS.md for the rule mapping.\n'
    else
        printf '\n⚠ Some VCs unproven (cargo creusot prove exit %d).\n' "$PROVE_RC"
        printf '  Iterate on contracts in verification/src/event_queue.rs / src/timer_table.rs.\n'
        printf '  For a tighter loop: cargo creusot prove (from workspace root).\n'
    fi

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
