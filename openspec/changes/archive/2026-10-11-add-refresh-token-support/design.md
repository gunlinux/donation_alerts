## Context

See `proposal.md` for motivation. Relevant current state:

- `src/config.rs` owns all persistence. `Config::load` reads `.env` via `dotenvy` and
  `Config::store_access_token` rewrites the single `ACCESS_TOKEN` line with
  `upsert_env_var`, preserving other lines.
- `src/auth.rs::exchange_and_store` already parses the token response as
  `serde_json::Value` and prints it, but only extracts `access_token`.
- `src/stream.rs::listen` loops forever: `run_once` fetches a connection token, opens the
  Centrifugo socket, subscribes, then `drain`s. Every error just logs and retries after
  `RECONNECT_DELAY`.
- The project has no specs yet; `.gitignore` currently contains only `/target` and `.env`.

## Goals / Non-Goals

**Goals:**
- Persist OAuth refresh material durably across process restarts, without putting the refresh
  token in `.env`.
- Make renewal a first-class CLI action (`refresh`) *and* an automatic internal step of
  `listen`.
- Keep the change small: reuse the existing `Value`-based parsing, `reqwest` client, and
  `upsert_env_var` machinery; no new crates.

**Non-Goals:**
- OS keychain / secret-manager integration.
- Multi-account or multi-tenant token storage.
- Automating the browser step of the authorization flow.
- Changing how `login` builds the authorization URL or which scopes are requested.

## Decisions

**1. Store refresh material in a dedicated gitignored file, not `.env` (required by proposal).**
Add `.tokens.json` at the repo root, added to `.gitignore`, holding the refresh token and
expiry. `ACCESS_TOKEN` stays in `.env` — the current access token is the value the listener
reads every run, and keeping it in `.env` preserves the existing user workflow.
*Alternatives considered:* (a) put the refresh token in `.env` — rejected by the user's
constraint and because `.env` is copied into docs/examples; (b) OS keychain — a new dependency
and platform-specific code for a one-person CLI; (c) a plain `.refresh_token` file — a JSON
file is self-describing and extensible without a second format.

**2. Store expiry as an absolute Unix timestamp (`expires_at`), computed at write time from
`expires_in`.** The refresh decision then depends only on the current clock, not on file
mtimes. Absence of the field is treated as "unknown" and forces a refresh attempt before
connecting.

**3. Secrets file `0600`, created via `std::os::unix::fs::OpenOptionsExt` with
`mode(0o600)`.** Unix-only is acceptable: the target is Linux/macOS. Writing always re-asserts
the mode so an existing world-readable file is tightened.

**4. `refresh` exchange reuses the existing form-post shape** with
`grant_type=refresh_token`, `client_id`, `client_secret`, `refresh_token`. On success it
rewrites `ACCESS_TOKEN` in `.env` and re-persists `refresh_token` (new value if the endpoint
rotates it, otherwise the previous one) plus the new `expires_at`.

**5. `listen` refreshes lazily and distinguishes auth failure from transport failure.**
- Pre-flight: in `listen`, before entering the loop, refresh if `expires_at` is missing or
  within a skew margin (60s) of now.
- On a run: an authorization failure (HTTP 401 from `user/oauth` or `centrifuge/subscribe`,
  surfaced as a distinct error variant) triggers exactly one refresh attempt followed by a
  reconnect; a successful refresh clears the "already refreshed" flag.
- If refresh is impossible (no stored refresh token, or the endpoint rejects it), return an
  actionable error and exit instead of reconnecting — this avoids the current tight 401 loop.
  Transient errors keep today's reconnect-forever behavior.
*Alternatives considered:* a background refresh task — unnecessary complexity for a
single-connection client; refreshing on every `run_once` — wastes a network round trip when the
token is still valid.

**6. Restructure the token response into a small struct** (`TokenSet { access_token,
refresh_token: Option<String>, expires_at: Option<i64> }`) parsed once, used by both `token`
and `refresh`, instead of re-reading `Value` pointers in two places.

## Risks / Trade-offs

- [Refresh-token rotation semantics unknown: a provider may or may not return a new
  `refresh_token` on refresh] → Always persist the returned `refresh_token` when present, else
  retain the stored one; this is correct for both rotation and non-rotation.
- [`.tokens.json` and `.env` can drift (e.g. user hand-edits `ACCESS_TOKEN`)] → The access
  token is authoritative from `.env`; the secrets file only holds refresh material and expiry,
  so a hand-edited access token still works until it expires.
- [A refresh at startup adds a round trip before the first connection] → Only performed when
  the stored expiry is missing/near; the common steady state connects without it.
- [Non-Unix hosts would not get `0600`] → Out of scope; the project targets Linux/macOS and the
  code will set the mode under `cfg(unix)` with a plain write fallback elsewhere.
- [Changing `listen` to exit on unrecoverable 401 changes its "always reconnects" contract] →
  Intentional and specified: spinning on a 401 is never useful, and the error message tells the
  user exactly how to recover.

## Migration Plan

1. Add `.tokens.json` to `.gitignore` before any code writes it.
2. Ship code; existing installs with only `ACCESS_TOKEN` in `.env` keep working — they simply
   have no refresh material until the next `token <CODE>` (or `refresh` once material exists).
3. Rollback: remove the file from `.gitignore` and revert the code; no data migration needed
   since `.env` handling is unchanged.
