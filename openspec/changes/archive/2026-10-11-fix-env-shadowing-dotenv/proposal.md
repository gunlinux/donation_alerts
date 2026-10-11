## Why

Two defects in `.env` loading combine to make stored tokens appear broken:

1. **Parsing stops silently.** dotenvy rejects a line whose unquoted value contains spaces
   (`Err(LineParse)`). The shipped `.env.example` sets
   `SCOPES=oauth-user-show oauth-donation-subscribe oauth-donation-index` unquoted, and
   `Config::load_from` discards the dotenvy error (`let _ = …`). dotenvy applies only the keys
   parsed **before** the failing line, so `ACCESS_TOKEN` — the last entry written by
   `token`/`refresh` — is never read from `.env`.
2. **The environment shadows `.env`.** `dotenvy::from_filename` does not overwrite variables
   already present in the process environment. When the shell exports `ACCESS_TOKEN`, an old
   value wins, so a freshly stored token is ignored.

Together: `donationalerts token <CODE>` writes `ACCESS_TOKEN` into `.env`, but the next
`listen` never loads it — it uses the stale exported value, or fails with "ACCESS_TOKEN is not
set" when nothing is exported. Automatic refresh then also appears broken.

## What Changes

- Surface a malformed `.env` as an error instead of silently applying a partial file.
- Quote multi-word values: fix `SCOPES` in `.env.example` (and the CLI's own writer), so a
  value with spaces round-trips.
- Make `.env` authoritative over pre-existing process-environment variables for the keys the
  CLI manages (`APP_ID`, `CLIENT_SECRET`, `USER_ID`, `ACCESS_TOKEN`, `REDIRECT_URI`, `SCOPES`),
  so a token written by `token`/`refresh` is always the one used by the next run.
- Add regression tests for all three behaviours.

## Capabilities

### New Capabilities
- `env-loading`: how the CLI loads `.env` and the process environment — parse-failure
  handling, value quoting, and precedence.

### Modified Capabilities
<!-- none -->

## Impact

- `src/config.rs`: `Config::load_from` (error handling, precedence), `upsert_env_var` (quoting),
  and unit tests.
- `.env.example`: quote the `SCOPES` value.
- Local `.env` (gitignored): quote `SCOPES` so the current install starts working.
- `README.md`: note the precedence and the quoting rule.
- No public CLI surface, API, or dependency changes.
