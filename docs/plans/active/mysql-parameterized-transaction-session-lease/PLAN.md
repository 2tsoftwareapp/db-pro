# Feature Plan: MySQL Parameterized Transaction Session Lease

## Slug
`mysql-parameterized-transaction-session-lease`

## Summary
In `MySqlConnector::execute_parameterized_transaction`, active session leases (`SessionLease`) held by an open query tab transaction were ignored, causing `execute_parameterized_transaction` to open a separate pool connection via `pool.begin()`.

This plan implements `execute_parameterized_transaction_joined` in `MySqlConnector`. It uses text-protocol execution (`conn.execute`) for `SAVEPOINT`, `ROLLBACK TO SAVEPOINT`, and `RELEASE SAVEPOINT` to avoid prepared-statement protocol Error 1295. Upon successful savepoint rollback, it reports `TransactionFailureOutcome::RolledBack`.

## Target Files
- `crates/infrastructure/src/mysql/connector.rs`
- `crates/infrastructure/src/mysql/tests/connector_tests.rs`
- `crates/infrastructure/tests/mysql_integration.rs`
- `docs/plans/active/mysql-parameterized-transaction-session-lease/*`

## Verification
- Unit tests in `crates/infrastructure/src/mysql/tests/connector_tests.rs`
- Integration tests in `crates/infrastructure/tests/mysql_integration.rs`
- Quality gates: `cargo +nightly fmt`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p db-pro-infrastructure`
