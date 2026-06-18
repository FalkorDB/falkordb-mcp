# Copilot / AI agent instructions for `falkordb-mcp`

Guidance for GitHub Copilot and other AI agents working in this repository. It encodes the
engineering conventions so changes land clean on the first try. Human contributors should follow it
too. These mirror the conventions of the sister repo [`falkordb-rs`](https://github.com/FalkorDB/falkordb-rs).

`falkordb-mcp` is a **Model Context Protocol (MCP) server** (a binary crate) that lets AI assistants
explore a *live* FalkorDB graph database. It is built on the `rmcp` SDK over stdio and the async
[`falkordb`](https://crates.io/crates/falkordb) client. **v1 is read-only**; guarded writes are a
later, opt-in addition.

## Golden rule: drive everything through `just`

For **any** action that CI performs (format, lint, build, docs, deny, tests, coverage, spellcheck, …),
run the **exact same `just` recipe CI uses** — never a raw `cargo …` command. If a check needs
changing, update the `just` recipe **and** the CI workflow together so they stay identical. Run
`just --list` to see every recipe.

| Recipe | Purpose |
| --- | --- |
| `just check` | Fast pre-commit loop: `fmt clippy build`. |
| `just ci` | Required gates: `fmt-check clippy build doc deny test`. |
| `just done` | Definition-of-done gates (`fmt-check`, `clippy`, `clippy-all`, `build`, `doc`, `deny`, `test`). Must be green before declaring a task done. |
| `just verify` | `ci` **plus** `coverage`. |
| `just test` | Run the test suite (nextest). **Hermetic** — unit tests use a fake backend, no database needed. |
| `just coverage` | Codecov JSON coverage (matches the `coverage` CI job). |
| `just spellcheck` | Spellcheck the Markdown docs. |
| `just spellcheck-pr-title` | Spellcheck a PR title (`PR_TITLE='…' just spellcheck-pr-title`). |

The tests are **hermetic** (no FalkorDB server required): tools are tested through the
[`FalkorBackend`](src/backend.rs) trait with a fake implementation. Any test that talks to a real
database must be **opt-in and skippable**, never a default CI gate (see "Flaky tests").

## Definition of done for a change

1. **Design first** for non-trivial work, and **rubber-duck review** the design before coding.
2. **Implement** the change with: code **+ tests + docs** (doc-comments, an example where it helps)
   **+ a `CHANGELOG.md` entry**. On every change, **check and align all documentation** — see "Keep
   documentation in sync".
3. **Validate locally via `just`** — all relevant gates green (`just done`, plus `just coverage` and
   `just spellcheck`).
4. Open a PR on a `feat:` / `fix:` / `ci:` / `docs:` branch.
5. **Resolve every AI review thread** (Copilot **and** CodeRabbit) — reply *and* mark resolved —
   before merge. Copilot auto-reviews on push.
6. A human merges; `release-plz` handles the release.

## Keep documentation in sync

On **every** change, align **all** documentation so it never drifts from the code — treat "the docs
match the code" as part of the definition of done, not a follow-up:

- **`README.md`** — the tool list, the `mcpServers` config block, env vars, and examples must match
  the actual tools/config.
- **doc-comments and examples** — keep them accurate and compiling.
- When you change the **tool surface or its inputs**, update the README's tool table and the
  `get_info` instructions string in the same change.

## Flaky tests are a hard no

Fix a flaky test **immediately, as top priority**, regardless of the current task or whether the flake
is a pre-existing / non-regression issue. Flaky tests slow everyone down. The unit suite is hermetic
by design (fake backend); **never** make a network/Docker/binary-download dependency a default test
gate — that is exactly how flakiness creeps in. Find the root cause rather than papering over it.

## Coverage

Patch coverage must be **≥ 95%**. Measure with the exact CI command (`just coverage` → `codecov.json`),
not an ad-hoc line count. Codecov counts a `?`-operator's **untaken error branch as a partial** that
lowers patch coverage; prefer combinator chains (`.and_then`/`.map`) or a shared helper over scattered
`?`/`match` in tests so those partials don't pile up.

## Spellcheck, commit subjects & PR titles

- **PR titles must be spellcheck-clean** — the `PRTitle` CI task checks every PR title against the
  wordlist and fails it at PR time. `release-plz` copies merged commit subjects (squashed PR titles)
  **verbatim** into `CHANGELOG.md` (itself spellchecked), so an unknown word would otherwise surface —
  too late — on the release PR. Keep titles clean at the source.
- When you add a **new type / term** (e.g. an MCP tool name), add it to **`.github/wordlist.txt`**.
- In Markdown, **backtick** code and type names (`` `query_read` ``) — backticked spans are ignored by
  the spellchecker.

## CHANGELOG & releases

- Keep a `## [Unreleased]` section; `release-plz` promotes it on release. **Don't hardcode the next
  version.** Put the **detailed entry directly under the version's single `### Added`/`### Fixed`
  heading** (release-plz prepends a short line from the PR title — don't add a second heading).
- Use **Conventional Commits** (`feat`, `fix`, `ci`, `docs`, `chore`, …). Mark breaking changes with
  `feat!`.
- Include a `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>` trailer on
  agent-authored commits.

## MCP-server conventions (project-specific)

- **Read-only safety by construction.** `query_read` uses the server's `GRAPH.RO_QUERY` (which rejects
  writes) — **never** parse Cypher to decide read-vs-write. Note `GRAPH.PROFILE` *executes* the query,
  so `profile` is write-capable and stays behind the (future) writes gate.
- **Pin `rmcp`** to an exact version (`=1.7.x`) — it moves fast; bump deliberately and re-test.
- **Serialize via explicit DTOs.** `FalkorValue`/`Node`/`Edge`/`Path` don't derive `Serialize`; map
  them to the JSON DTOs in [`src/backend.rs`](src/backend.rs) — don't rely on the client's types.
- **stdout belongs to the MCP protocol.** All logging goes to **stderr** (`tracing` → stderr).
- **Connection config comes only from the operator's environment** (`FALKORDB_URL`, …), never from a
  tool-call parameter. **Scrub credentials** from any error returned to the client; surface
  `FalkorDBError::mitigation_hint()` instead.
- **Testability:** tools depend on the `FalkorBackend` trait, so add a fake-backend unit test for each
  tool's success and error path.

## Code style

Keep code **tidy, simple, and efficient**. Comment only what genuinely needs clarification — not the
obvious. Match the surrounding style; prefer the smallest change that fully solves the problem.
