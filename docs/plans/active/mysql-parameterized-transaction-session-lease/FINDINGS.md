# Findings — MySQL Parameterized Transaction Session Lease

## Defect Summary
In `MySqlConnector::execute_parameterized_transaction`, active session leases (`SessionLease`) held by an open query tab transaction were ignored, causing `execute_parameterized_transaction` to open a separate pool connection via `pool.begin()`.

## Protocol Detail for MySQL Savepoints
`MySqlConnector::execute_parameterized_transaction_joined` executes `SAVEPOINT <name>`, `ROLLBACK TO SAVEPOINT <name>`, and `RELEASE SAVEPOINT <name>` using text-protocol query execution (`conn.execute(&*sql)` via `sqlx::Executor`), ensuring MySQL executes savepoint control statements without protocol Error 1295.

## Transaction Failure Outcome Contract
Per `TransactionFailureOutcome` contract in `crates/core/src/ports/db_connector.rs`:
`RolledBack` indicates that the operation's savepoint within a caller-owned transaction was rolled back.
When savepoint rollback succeeds, `execute_parameterized_transaction_joined` returns `TransactionFailureOutcome::RolledBack`, keeping the outer session transaction usable in `open_session`. If the savepoint rollback itself fails, the connection is discarded and the outcome reports `TransactionFailureOutcome::Unknown`.

## Regression Coverage
1. Unit tests in `crates/infrastructure/src/mysql/tests/connector_tests.rs`:
   - `execute_parameterized_transaction_attempts_lease_checkout_when_held`
   - `execute_parameterized_transaction_reports_validation_failure_on_unknown_handle`
2. Integration tests in `crates/infrastructure/tests/mysql_integration.rs`:
   - `mysql_joined_parameterized_transaction_rolls_back_with_the_tab`
   - `mysql_joined_parameterized_transaction_rolls_back_only_its_own_statements`

## Severity
P1 (database mutation correctness, transaction isolation failure, provider parity).
