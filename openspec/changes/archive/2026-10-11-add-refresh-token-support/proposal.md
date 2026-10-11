## Why

The `token` command stores only `access_token` and discards the `refresh_token` and
`expires_in` that the DonationAlerts token endpoint returns. When the access token expires,
`listen` fails with a 401 and the only fix is to redo the whole browser authorization flow.
The grant we request already includes `oauth-donation-subscribe`/`oauth-donation-index`, so the
refresh token we throw away today is exactly what would let the long-running listener renew
access silently.

## What Changes

- `token <CODE>` now also persists the `refresh_token` and the token expiry returned by the
  token endpoint. **The refresh token is NOT written to `.env`** — it goes into a separate
  gitignored secrets file (`.tokens.json`, mode `0600`). `ACCESS_TOKEN` continues to live in
  `.env` as today.
- New `refresh` subcommand: exchanges the stored `refresh_token` at the token endpoint
  (`grant_type=refresh_token`), rewrites `ACCESS_TOKEN` in `.env`, and rotates the stored
  refresh token + expiry.
- `listen` refreshes the access token when the stored expiry is missing or in the past (with a
  small skew margin) before opening the connection, and re-refreshes once on a 401 during the
  run, so a long-running listener survives token expiry without manual intervention.
- `.env.example` / `README.md` document the new file and command; `.tokens.json` is added to
  `.gitignore`.
- No breaking changes to existing commands; a missing token store degrades to today's behavior
  (a 401 tells the user to re-run `token`).

## Capabilities

### New Capabilities
- `token-refresh`: how the CLI persists OAuth refresh material, renews an access token from it,
  and keeps `listen` running across access-token expiry.

### Modified Capabilities
<!-- none: openspec/specs/ is empty; all behavior here is newly specified -->

## Impact

- **Code**: `src/auth.rs` (capture + persist refresh material, add refresh exchange),
  `src/config.rs` (load/store token metadata, new secrets-file path), `src/stream.rs`
  (pre-flight and on-401 refresh), `src/main.rs` (new `refresh` subcommand).
- **Files on disk**: `.tokens.json` (new, gitignored, `0600`), `.gitignore`, `.env.example`,
  `README.md`; `.env` continues to hold `ACCESS_TOKEN`.
- **Dependencies**: none added — `serde`/`serde_json` and `reqwest` already cover it.
- **Security**: the refresh token is a long-lived credential; the constraint is that it must
  not land in `.env` (which is copied around in docs/examples) and must be readable only by the
  owning user.
