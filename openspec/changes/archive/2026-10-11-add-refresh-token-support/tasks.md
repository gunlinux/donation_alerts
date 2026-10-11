## 1. Persistence foundation

- [x] 1.1 Add `.tokens.json` to `.gitignore` and verify `git status --porcelain` stays clean after a `.tokens.json` file is created locally
- [x] 1.2 Define a `TokenSet` type (`access_token`, `refresh_token: Option<String>`, `expires_at: Option<i64>`) with parsing from the token-endpoint JSON (`expires_in` → absolute `expires_at`), and add unit tests covering: refresh token present, refresh token absent, and missing `expires_in` — verify `cargo test` passes
- [x] 1.3 Add secrets-file read/write to `config.rs`: load/save `TokenSet` refresh material at `.tokens.json`, writing with mode `0o600` under `cfg(unix)`; verify a round-trip test writes then reloads and that the file mode is `0600` on Unix
- [x] 1.4 Wire the new path into `Config` (token-store path + accessors, reusing the existing `.env` root) and verify `cargo build` succeeds with no unused warnings

## 2. Token exchange and the refresh command

- [x] 2.1 Update `exchange_and_store` to persist the full `TokenSet`: `ACCESS_TOKEN` to `.env`, refresh material to `.tokens.json`, and clear stale refresh material when the response has none — verify with a unit test that `.env` never contains the refresh token value
- [x] 2.2 Implement the refresh exchange (`grant_type=refresh_token` with `client_id`/`client_secret`/`refresh_token`) that rewrites `ACCESS_TOKEN` in `.env`, persists rotated/retained refresh material + new `expires_at`, and errors with a "run `token <CODE>`" message when no refresh material exists — verify error path leaves `.env` unchanged
- [x] 2.3 Add the `refresh` subcommand to `main.rs`/`Cli` (with an alias) and verify `cargo run -- refresh` with no stored material prints the actionable error and exits non-zero

## 3. Listener integration

- [x] 3.1 Add the pre-flight refresh in `listen`: refresh before the loop when `expires_at` is missing or within the 60s skew margin, otherwise connect with the current `ACCESS_TOKEN` — verify via unit test on the "should refresh?" predicate for past/near/future expiries
- [x] 3.2 Introduce a distinct authorization-failure error for 401 responses from `user/oauth` and `centrifuge/subscribe`, and verify the existing error strings for other statuses are unchanged
- [x] 3.3 On a run's authorization failure, attempt exactly one refresh, reconnect on success, and on impossible/rejected refresh return an actionable error that exits `listen` instead of reconnecting — verify transient (non-401) errors still reconnect

## 4. Documentation and checks

- [x] 4.1 Update `README.md` and `.env.example` to describe the refresh-token file, the `refresh` command, and automatic renewal in `listen`
- [x] 4.2 Run the full check suite — `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` — and verify all pass
