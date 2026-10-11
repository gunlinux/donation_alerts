## Purpose

Define how the DonationAlerts CLI reads its configuration from `.env` and the process
environment: which source wins, and how malformed files and multi-word values are handled.

## ADDED Requirements

### Requirement: A malformed `.env` fails loudly

When `.env` exists but cannot be parsed, the CLI MUST report the failure (with the offending
line) instead of silently applying only the keys parsed before the failure. A missing `.env`
SHALL remain valid.

#### Scenario: Unparseable line stops the load with an error

- **WHEN** `.env` contains a line dotenvy rejects (for example an unquoted value with spaces)
- **THEN** loading configuration returns an error mentioning `.env`, and no configuration is
  produced

#### Scenario: A missing `.env` is fine

- **WHEN** no `.env` file exists
- **THEN** loading falls back to the process environment without error

### Requirement: Multi-word values round-trip through `.env`

Values that contain whitespace MUST be written quoted, so the file re-parses to the same value.
The shipped `.env.example` MUST use a form that parses.

#### Scenario: A quoted scope list loads

- **WHEN** `.env` contains `SCOPES="oauth-user-show oauth-donation-subscribe"`
- **THEN** the loaded configuration has `scopes` equal to that full string

#### Scenario: The CLI quotes a spaced value it writes

- **WHEN** the CLI persists a value containing spaces
- **THEN** the written line is quoted, so a subsequent load yields the original value

### Requirement: Persisted credentials override the inherited environment

When loading configuration, the CLI MUST give values defined in `.env` precedence over
variables already present in the process environment for the keys it manages (`APP_ID`,
`CLIENT_SECRET`, `USER_ID`, `ACCESS_TOKEN`, `REDIRECT_URI`, `SCOPES`). A managed key that
`.env` does not define SHALL continue to fall back to the process environment. This ensures a
token written by `token <CODE>` or `refresh` is the token actually used by the next run.

#### Scenario: `.env` overrides an exported variable

- **WHEN** the process environment already contains `ACCESS_TOKEN=old` and `.env` contains
  `ACCESS_TOKEN=new`
- **THEN** the loaded configuration uses `new`

#### Scenario: Keys absent from `.env` fall back to the environment

- **WHEN** a managed key (for example `APP_ID`) is present in the process environment but not
  defined in `.env`
- **THEN** the loaded configuration uses the environment value
