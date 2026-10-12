## Purpose

Publish every donation the listener consumes from the DonationAlerts gateway to RabbitMQ, so
downstream services in the workspace bus can consume donations from a durable queue instead of
scraping the listener's stdout.

## ADDED Requirements

### Requirement: Consumed donations are published to RabbitMQ

While listening, the CLI SHALL publish every donation it receives to RabbitMQ, in addition to
writing it to standard output. Each message SHALL be published to the default exchange with the
fixed routing key / queue name `donations`, and SHALL be published as a persistent message
(delivery mode 2) so it survives a broker restart.

#### Scenario: A received donation is published

- **WHEN** the listener receives a donation from the gateway
- **THEN** it publishes that donation as one message to the `donations` queue on the default
  exchange, marked persistent

#### Scenario: Standard output is unchanged

- **WHEN** publishing is enabled and a donation is received
- **THEN** the donation is still written to standard output as one JSON line, exactly as before

#### Scenario: The durable queue exists before publishing

- **WHEN** the listener starts and connects to the broker
- **THEN** it declares a durable queue named `donations` before publishing any message

### Requirement: RABBIT_MQ_DSN is required to listen

Because listening now always publishes, the CLI SHALL read the broker connection string from
`RABBIT_MQ_DSN`, and the listener SHALL fail fast — it SHALL panic with a message naming
`RABBIT_MQ_DSN` — when that variable is unset or empty, before it connects to the gateway.

#### Scenario: Missing DSN panics

- **WHEN** `listen` runs and `RABBIT_MQ_DSN` is not set in `.env` or the environment
- **THEN** the process panics with a message naming `RABBIT_MQ_DSN` and does not connect to the
  gateway

#### Scenario: Empty DSN panics

- **WHEN** `listen` runs and `RABBIT_MQ_DSN` is set to an empty value
- **THEN** the process panics with a message naming `RABBIT_MQ_DSN`

#### Scenario: Present DSN is used

- **WHEN** `listen` runs with `RABBIT_MQ_DSN` present
- **THEN** the listener connects to the broker at that address and proceeds

### Requirement: The published payload is the raw donation JSON

The published message body SHALL be the donation JSON object exactly as the gateway delivered
it, with no field re-mapping, renaming, or filtering. Fields that are empty or null in the
source (`name`, `message`, `payin_system`, `shown_at`, and any others) SHALL be passed through
unchanged, and fields the CLI does not know about SHALL be preserved.

#### Scenario: Empty or null optional fields are preserved

- **WHEN** a donation arrives whose `name` or `message` is empty or null
- **THEN** the published message is still produced and those fields carry the same empty or null
  value they had in the source

#### Scenario: Unknown fields are preserved

- **WHEN** a donation carries fields the CLI does not interpret
- **THEN** those fields appear unchanged in the published message

### Requirement: Broker failures do not silently drop donations

When the broker connection is unavailable or a publish fails, the CLI SHALL report the condition
on standard error and SHALL recover by reconnecting and retrying, rather than treating the
donation stream as healthy while dropping messages in silence.

#### Scenario: Broker connection lost

- **WHEN** the connection to the broker drops while listening
- **THEN** the CLI reports the failure on standard error, reconnects, and resumes publishing
  subsequent donations

#### Scenario: A publish attempt fails

- **WHEN** publishing a donation fails
- **THEN** the CLI reports the failure on standard error and retries the publish once a
  connection is available
