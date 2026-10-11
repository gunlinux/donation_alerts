# token-refresh Specification

## Purpose

Keep the DonationAlerts CLI authorized over time by persisting the OAuth refresh material the
token endpoint returns and using it to renew an expired access token, so a long-running
listener does not require re-running the browser authorization flow.

## Requirements

### Requirement: Persist refresh material from the token exchange

When the OAuth token exchange succeeds, the CLI SHALL store the returned access token as
`ACCESS_TOKEN` in `.env` and SHALL additionally persist the returned `refresh_token` and the
access-token expiry. The `refresh_token` MUST NOT be written to `.env` or any other
world- or group-readable location; it SHALL be stored in a dedicated secrets file that is
excluded from version control and readable only by the owning user.

#### Scenario: Successful exchange stores both tokens

- **WHEN** the user runs `token <CODE>` and the token endpoint returns an access token, a
  refresh token, and an expiry
- **THEN** `ACCESS_TOKEN` is written to `.env`
- **AND** the refresh token and expiry are written to the dedicated secrets file
- **AND** the secrets file is not tracked by git and is readable only by the owning user

#### Scenario: Refresh token is never written to .env

- **WHEN** the token exchange completes
- **THEN** no line of `.env` contains the refresh token value

#### Scenario: Token response without a refresh token

- **WHEN** the token endpoint returns an access token but no refresh token
- **THEN** the access token is stored as `ACCESS_TOKEN`
- **AND** no stale refresh material is left behind from a previous exchange
- **AND** the command does not fail

### Requirement: Renew the access token from stored material

The CLI SHALL provide a command that exchanges the stored refresh token at the token endpoint
for a new access token and updates persisted state accordingly.

#### Scenario: Refresh succeeds

- **WHEN** the user runs the refresh command with valid stored refresh material
- **THEN** a new access token is written to `.env` as `ACCESS_TOKEN`
- **AND** any rotated refresh token and the new expiry are persisted to the secrets file
- **AND** the command reports success

#### Scenario: No stored refresh material

- **WHEN** the user runs the refresh command and no refresh token is stored
- **THEN** the command fails with a message directing the user to run the authorization flow
  (`token <CODE>`)
- **AND** `.env` is left unchanged

#### Scenario: Refresh is rejected

- **WHEN** the token endpoint rejects the refresh token (for example a 400 or 401)
- **THEN** the command fails and reports the endpoint's error
- **AND** the existing `ACCESS_TOKEN` and stored refresh material are left unchanged

### Requirement: Listener survives access-token expiry

The listener SHALL obtain a usable access token before opening the gateway connection and
SHALL recover from an access token that expires mid-run, without requiring the user to
re-run the authorization flow, as long as refresh material is available.

#### Scenario: Token expired before connecting

- **WHEN** the listener starts and the stored expiry is absent or within the refresh margin of
  the current time
- **THEN** it renews the access token using the stored refresh token before connecting

#### Scenario: Token accepted before connecting

- **WHEN** the listener starts and the stored expiry is comfortably in the future
- **THEN** it connects using the existing `ACCESS_TOKEN` without performing a refresh

#### Scenario: Token expires while listening

- **WHEN** a connected listener receives an authorization failure (for example HTTP 401)
- **THEN** it attempts to renew the access token from the stored refresh material and, on
  success, reconnects with the new token

#### Scenario: Expiry cannot be recovered

- **WHEN** an authorization failure occurs and no usable refresh material is available, or the
  refresh is rejected
- **THEN** the listener reports an actionable error telling the user to re-run the
  authorization flow and stops rather than reconnecting in a loop
