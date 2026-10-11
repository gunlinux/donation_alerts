## 1. Loading precedence

- [x] 1.1 Update `Config::load()` in `src/config.rs` so values defined in `.env` override any
  pre-existing process-environment variables for the managed keys (`APP_ID`, `CLIENT_SECRET`,
  `USER_ID`, `ACCESS_TOKEN`, `REDIRECT_URI`, `SCOPES`), while keys absent from `.env` still
  fall back to the environment — verify `cargo build` succeeds with no warnings
- [x] 1.2 Add a unit test covering the fallback: a managed key present only in the environment
  (not in `.env`) is still loaded — verify `cargo test config::` passes

## 2. Regression coverage

- [x] 2.1 Add a unit test that exports a conflicting `ACCESS_TOKEN` and loads a `.env` defining
  a different `ACCESS_TOKEN`, asserting the `.env` value wins; serialize it against other
  env-mutating tests and restore the environment afterwards — verify `cargo test` passes and
  the test fails if the precedence fix is reverted
- [x] 2.2 Update `README.md` to note that `.env` takes precedence over exported variables for
  the managed keys — verify the note reads correctly and no other docs contradict it

## 3. Parse-failure handling

- [x] 3.1 Stop discarding the dotenvy error in `Config::load_from`: when `.env` exists but
  fails to parse, return an error that names the file; keep a missing `.env` valid — verify a
  unit test asserts an unquoted multi-word line yields an error and an absent file yields `Ok`
- [x] 3.2 Quote multi-word values in `upsert_env_var` so a written value containing spaces
  re-parses to the same value — verify a unit test round-trips a spaced value through the
  writer and the reader
- [x] 3.3 Quote `SCOPES` in `.env.example` and in the local `.env` so the scope list parses —
  verify `donationalerts login` still prints the three scopes and a load no longer stops early

## 4. End-to-end verification

- [x] 4.1 With the exported `ACCESS_TOKEN`/`USER_ID` removed from the environment, run
  `donationalerts listen` against the real `.env` and verify it now reads `ACCESS_TOKEN` from
  `.env` (reaches the gateway) instead of erroring "ACCESS_TOKEN is not set"

## 5. Checks

- [x] 5.1 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test` and verify all pass
