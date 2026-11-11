# AGENTS.md

Guidance for AI coding agents (and humans) working in this repository. Read this before making changes.

## What this is

`gcflx` is the official command-line client for [GitConflux](https://gitconflux.dev), a task-queue and Change Proposal workflow. It is an **enterprise-grade, security-sensitive** Rust project that must work on macOS, Linux and Windows (x86_64 and aarch64). The HTTP/Git contract is described in `api.md` (local reference, gitignored); the server is authoritative.

## Ground rules

1. **Rust only.** No other languages in the product. Shell is allowed only for installer/CI scripts under `scripts/` and `.github/`.
2. **Security first.** Never take a shortcut that weakens a guarantee in [SECURITY.md](SECURITY.md). If unsure, choose the safer behavior and say so.
3. **No cheap fixes.** Find the root cause, add a test that fails without the fix, and keep behavior consistent across platforms.
4. **Surgical changes.** Do not refactor or reformat unrelated code.
5. **Do not claim unimplemented server features exist.** Promotion/landing (`integrate`) is not available; Tier 2 preview is advisory only. There is no lease-renewal endpoint.

## Repository layout

| Path | Purpose |
| --- | --- |
| `crates/gitconflux-core` | Errors and exit codes, `Token` (redacted), `ApiUrl` validation, credential storage, input validation, hardened `git` runner, proposal metadata and bundling. No networking. |
| `crates/gitconflux-api` | Typed HTTP client and models. All network behavior lives here. |
| `crates/gitconflux-cli` | The `gcflx` binary: clap definitions (`cli.rs`), dispatch (`commands.rs`), read/write/submit handlers, output helpers, `askpass.rs`. |
| `scripts/` | `install.sh` (macOS/Linux installer) and `test-install.sh`. |
| `.github/workflows/` | CI (fmt, clippy, tests on 6 OS/arch targets, cargo-deny, installer tests) and release. |

Dependency direction: `cli → api → core`. Keep business rules in `core`, transport in `api`, presentation in `cli`.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets      # must be warning-free
cargo test --workspace
cargo deny check                            # advisories, licenses, bans, sources
shellcheck scripts/*.sh && sh scripts/test-install.sh
```

Run all of the above before committing. CI treats warnings as errors.

## Code conventions

- `unsafe` is forbidden. `unwrap`, `expect` and `panic!` are denied outside tests (tests add `#![allow(clippy::expect_used)]`). Return typed errors instead.
- Clippy `pedantic` is on; fix warnings rather than allowing them. If an `allow` is truly needed, scope it narrowly and add a one-line reason.
- Do not print with `println!`/`eprintln!`; use the `out()` helper or `writeln!` to the stream and handle the error.
- Errors map to stable exit codes (`ExitCode` in `core/src/error.rs`). New error kinds must pick a code deliberately; codes are a public contract.
- Comment only code that needs clarification.

## Security invariants (never break these)

- **Secrets** (personal access tokens, Git credentials) live only in `Token`/redacting types. Never put them in argv, URLs, logs, error messages, `Debug` output, or test snapshots. New secret-bearing types need a redacted `Debug` and a test proving it.
- **Transport:** HTTPS only (plain HTTP only for loopback, enforced by `ApiUrl`). Redirects are never followed. Response size is capped.
- **Mutations are never auto-retried.** Only idempotent GETs retry. The API has no idempotency keys; a timed-out mutation may have succeeded.
- **URLs:** build paths with `Client::url` (percent-encodes segments, rejects `.`/`..`) and queries with `query_pairs_mut`. Never format user input into a URL string.
- **Untrusted text** from the server must pass through `sanitize` (or the table helpers) before reaching a terminal.
- **Validate before mutating:** enforce documented limits in `core/src/validate.rs` before any request is sent.
- **Git:** only via `core::git::Git` (argv array, scrubbed env, no prompts, timeout, bounded output). Never invoke a shell. Revisions starting with `-` are rejected; use `--end-of-options`/`--` where git supports it. Network git commands authenticate through the host-pinned `GIT_ASKPASS` helper and run with hooks disabled.
- **Files:** credential files are written atomically with owner-only permissions; reads refuse group/world-accessible files on Unix.
- **Bundles:** always created under a temporary ref that is removed on every path (RAII); verify header, size (<= 50 MiB) and SHA-256.

## Testing expectations

- Every behavior change needs a test. Prefer:
  - unit tests in the module for pure logic,
  - `wiremock` tests in `gitconflux-api/tests` for HTTP behavior,
  - `assert_cmd` tests in `gitconflux-cli/tests` for end-to-end CLI behavior (exit codes, output, that tokens never appear),
  - real temporary git repositories for git/bundle logic.
- Tests must not touch the developer's real keychain, config or network. Set `HOME`/`XDG_CONFIG_HOME` to a temp dir and use `--api-url` with a mock server.
- Security-relevant changes must include a negative test (the unsafe input is rejected).

## Adding a command

1. Define args in `cli.rs` (use `value_parser` ranges/enums so clap rejects bad input).
2. Add any new API call to `gitconflux-api` with a model in `models.rs` and a wiremock test.
3. Validate input in `core/src/validate.rs` if it is a mutation.
4. Implement the handler in `read.rs`, `write.rs` or `submit.rs`; support `--json`; require auth via `authed_client` for mutations.
5. Add a CLI integration test and update `README.md` if user-visible.

## Commits and workflow

- Commit messages start with a bracketed type: `[feat]`, `[fix]`, `[chore]`, `[ci]`, `[docs]`, `[test]`, `[refactor]`, `[perf]`.
- Make small, logically separate commits with a clear body explaining *why*.
- Push to `main` after each completed unit of work. Never commit secrets, tokens, `.env` files, or `todo.md` / `api.md` (both gitignored).
- Include the trailer: `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>` when an AI assistant authored the commit.

## Using `gcflx` as an agent

Typical task loop (see `README.md` for details):

```sh
export GITCONFLUX_TOKEN=...            # or: gcflx auth login
export GITCONFLUX_AGENT_ID=...         # from: gcflx agents register <name>
gcflx tasks claim <project>            # "No tasks available." is a normal result (exit 0)
gcflx clone <project> && cd <project>
# implement, commit
gcflx proposals submit <project> --task <TASK_ID> --title ... --summary ... \
  --change-type feature --risk-level low --base <BASE_SHA>
gcflx proposals show <project> <PROPOSAL_ID> --watch
```

- Use `--json` for machine-readable output. Exit code `11` means the proposal was recorded but Tier 1 checks failed; the task stays claimed until its lease expires.
- Re-check state (`proposals list`, `tasks list`) before retrying any mutation after a timeout.
- Respect the claim lease (`leaseExpiresAt`); it cannot be renewed.
- Never print or log tokens.
