# Justfile — dev-cycle automation for falkordb-mcp.
#
# Run `just` (or `just --list`) to see every available recipe. Every check CI runs has a
# matching recipe here so the exact command can be reproduced locally.
#
# The unit/integration suite is HERMETIC: tools are exercised through a fake `FalkorBackend`,
# so `just test` needs no FalkorDB server. The opt-in `test-integration` recipe (and the
# `db-*` helpers) talk to a real server and are never a CI gate.

set shell := ["bash", "-uc"]

# --- Configuration (override on the CLI, e.g. `just port=6380 test-integration`) ---

# Host/port the opt-in live-integration recipes target.
host := env_var_or_default("FALKORDB_HOST", "127.0.0.1")
port := env_var_or_default("FALKORDB_PORT", "6379")

# Connection URL the live recipes pass to the tests/binary. Respects a `FALKORDB_URL` from the
# environment (so you can point at a remote server, or one needing credentials/TLS); otherwise it is
# built from host/port. Override any of these on the CLI, e.g. `just port=6380 test-integration`.
url := env_var_or_default("FALKORDB_URL", "falkor://" + host + ":" + port)

# Docker image and container name used by the `db-*` helpers.
image := "falkordb/falkordb:edge"
container := "falkordb-mcp-dev"

# Default recipe: list everything.
default:
    @just --list

# === Format ==================================================================

# Format all code in place.
fmt:
    cargo fmt --all

# Check formatting without modifying files (CI gate).
fmt-check:
    cargo fmt --all --check

# === Lint ====================================================================

# Clippy over the whole workspace (matches the `check-clippy` CI gate).
clippy:
    cargo clippy --all

# Strict clippy over all targets, warnings denied.
clippy-all:
    cargo clippy --all-targets -- -D warnings

# Dependency/license/advisory audit (matches the `check-deny` CI gate).
deny:
    cargo deny check bans licenses sources

# === Build ===================================================================

# Default build (matches the `check-build` CI gate).
build:
    cargo build

# Build every target.
build-all:
    cargo build --all-targets

# Build the API docs (matches the `check-doc` CI gate).
doc:
    cargo doc --all

# Build docs (no deps) and open them in a browser.
doc-open:
    cargo doc --all --no-deps --open

# === Test (hermetic — no server required) ====================================

# Run the full unit + integration suite (nextest). Uses the fake backend; no database needed.
test:
    cargo nextest run --all

# Run a single test by name filter, e.g. `just test-one query_read_caps_rows`.
test-one filter:
    cargo nextest run --all {{filter}}

# Run the doctests (nextest does not run them).
test-doc:
    cargo test --doc

# === Coverage (needs a reachable FalkorDB server — runs hermetic + live tests) ===============

# Generate Codecov JSON coverage (matches the `coverage` CI job): the hermetic suite PLUS the live
# integration tests, so it covers the real FalkorDB backend and needs a reachable server
# (set `FALKORDB_URL`, or `FALKORDB_HOST`/`FALKORDB_PORT`). Use `just coverage-local` for Docker.
coverage:
    FALKORDB_URL="{{url}}" \
        cargo llvm-cov nextest --all --run-ignored all --codecov --output-path codecov.json

# Generate an HTML coverage report and open it (also needs a reachable FalkorDB server).
coverage-html:
    FALKORDB_URL="{{url}}" \
        cargo llvm-cov nextest --all --run-ignored all --open

# Spin up a server, collect coverage (hermetic + live), then tear it down.
coverage-local: db-up
    @just coverage || (just db-down && exit 1)
    @just db-down

# === Spellcheck ==============================================================

# Spellcheck the Markdown docs (CI gate). Requires `pyspelling` and `aspell` locally.
spellcheck:
    pyspelling -c .github/spellcheck-settings.yml -n Markdown

# Spellcheck a pull-request title exactly as the Spellcheck CI gate does. Catches technical
# words (e.g. a new tool name) that release-plz would later copy verbatim into the changelog
# and fail the release PR. Set PR_TITLE first, e.g.
# `PR_TITLE='feat: add query_read tool' just spellcheck-pr-title`.
spellcheck-pr-title:
    printf '# %s\n' "${PR_TITLE:?set PR_TITLE to the pull-request title}" > .pr-title.md && pyspelling -c .github/spellcheck-settings.yml -n PRTitle && rm -f .pr-title.md || { rm -f .pr-title.md; exit 1; }

# === Opt-in live integration (NOT a CI gate) =================================
# These talk to a real FalkorDB server. Use `just test-integration-local` to manage one.

# Run the `#[ignore]`-marked tests that require a live FalkorDB server (set `FALKORDB_URL`, or
# `FALKORDB_HOST`/`FALKORDB_PORT`).
test-integration:
    FALKORDB_URL="{{url}}" cargo nextest run --all --run-ignored ignored-only

# Start a FalkorDB server in Docker on the configured port.
db-up:
    docker rm -f {{container}} >/dev/null 2>&1 || true
    docker run -d --name {{container}} -p {{port}}:6379 {{image}} >/dev/null
    @echo "Waiting for FalkorDB on {{host}}:{{port}}..."
    @for i in $(seq 1 30); do \
        if docker exec {{container}} redis-cli ping >/dev/null 2>&1; then \
            echo "FalkorDB is ready."; exit 0; \
        fi; sleep 1; \
    done; echo "FalkorDB did not become ready in time" >&2; exit 1

# Stop and remove the FalkorDB container.
db-down:
    docker rm -f {{container}} >/dev/null 2>&1 || true

# Spin up a server, run the live integration tests, then tear it down.
test-integration-local: db-up
    @just test-integration || (just db-down && exit 1)
    @just db-down

# === Aggregates ==============================================================

# Fast pre-commit loop: format, lint and build.
check: fmt clippy build

# Run every required CI gate locally (hermetic).
ci: fmt-check clippy build doc deny test

# Full post-task gate: strict clippy-all plus every CI gate. Must be green before a task
# is declared done.
done:
    ./scripts/post-checks.sh

# Full local validation: CI gates plus coverage.
verify: ci coverage

# === Housekeeping ============================================================

# Remove build artifacts and the generated coverage file.
clean:
    cargo clean
    rm -f codecov.json
