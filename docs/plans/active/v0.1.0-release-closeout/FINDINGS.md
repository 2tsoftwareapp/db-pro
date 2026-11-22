# Findings — Task 9 MySQL text-protocol savepoints

`MySqlConnector::execute_parameterized_transaction` joins `entry.open_session` via `checkout_if_held` and wraps the run in a per-run `SAVEPOINT` from `transaction_session::savepoint_name()`. Control statements go through `mysql_text_execute` (`sqlx::raw_sql` / COM_QUERY). Prepared `sqlx::query("SAVEPOINT …")` is not used (MySQL ERROR 1295). Statement failure issues `ROLLBACK TO SAVEPOINT` then `RELEASE`; a failed undo reports `Unknown` and discards the lease. Success `RELEASE` then `lease.succeed()`. Tab `BEGIN`/`COMMIT`/`ROLLBACK` on `execute()` with no params also use the text path so a query tab can actually hold a lease.

**Data Grid still does not join a query-tab transaction.** `UiCommand::ApplyTableChanges` has no `session_id`. `table_mutation_actions.rs` does not pass the active query document. `RuntimeCommand::ApplyTableChanges` calls `apply_mutations_detailed` without `with_query_session`. Grid apply therefore still opens its own `pool.begin()` even when a tab holds `BEGIN`. This task does not wire that path and does not claim Data Grid join.

Live proof of BEGIN → joined mutation → tab ROLLBACK is `mysql_joined_parameterized_transaction_rolls_back_with_the_tab` and `mysql_joined_parameterized_transaction_rolls_back_only_its_own_statements` in `mysql_integration` (`#[ignore]`; need `DATABASE_URL=mysql://…`).
