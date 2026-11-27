# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub's "Report a vulnerability" feature on this repository. Do not open public issues for security problems.

## Design guarantees

- **Secrets:** personal access tokens and Git credentials are wrapped in redacting types, never placed in command-line arguments or URLs, and never logged or echoed. Git receives credentials through a `GIT_ASKPASS` helper that only answers prompts for the expected host, and system credential helpers are disabled so short-lived tokens are not persisted.
- **Storage:** OS keychain by default; the optional file store is created atomically with owner-only permissions and refused if group/world accessible (Unix).
- **Transport:** HTTPS required (plain HTTP only for loopback). Redirects are not followed so credentials cannot be forwarded cross-origin. Responses are size-limited and requests time out.
- **Input handling:** all path segments and query values are percent-encoded; dot segments are rejected; documented API limits are validated before any mutation; revisions that look like options are rejected; server-supplied text is stripped of control characters before display.
- **Mutations:** never retried automatically (the API has no idempotency keys); only idempotent GETs retry on transient errors.
- **Subprocesses:** `git` is run with an argument array (no shell), a scrubbed environment, no prompts, a timeout and bounded output.
- **Code safety:** `unsafe` is forbidden workspace-wide; `unwrap`/`expect`/`panic` are denied outside tests.
- **Supply chain:** CI uses commit-pinned actions and runs `cargo deny`. Release builds use `--locked`, and the installer verifies GitHub-signed build provenance for the exact release workflow and tag before extraction. A checksum alone cannot authorize an install.
