## MODIFIED Requirements

### Requirement: Persisted credentials override the inherited environment

When loading configuration, the CLI MUST give values defined in `.env` precedence over
variables already present in the process environment for the keys it manages (`APP_ID`,
`CLIENT_SECRET`, `USER_ID`, `ACCESS_TOKEN`, `REDIRECT_URI`, `SCOPES`, `RABBIT_MQ_DSN`). A
managed key that `.env` does not define SHALL continue to fall back to the process environment.
This ensures a token written by `token <CODE>` or `refresh` is the token actually used by the
next run.

#### Scenario: `.env` overrides an exported variable

- **WHEN** the process environment already contains `ACCESS_TOKEN=old` and `.env` contains
  `ACCESS_TOKEN=new`
- **THEN** the loaded configuration uses `new`

#### Scenario: Keys absent from `.env` fall back to the environment

- **WHEN** a managed key (for example `APP_ID`) is present in the process environment but not
  defined in `.env`
- **THEN** the loaded configuration uses the environment value

#### Scenario: The broker DSN follows the same precedence

- **WHEN** `RABBIT_MQ_DSN` is present in both `.env` and the process environment
- **THEN** the loaded configuration uses the `.env` value
