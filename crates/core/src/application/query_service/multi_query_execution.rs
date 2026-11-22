use super::{
    classify_statement, format_transaction_failure, transaction_control_rejection, MultiQueryError, MultiQueryResult,
    QueryService, StatementClass, StatementResultKind,
};
use crate::domain::connection::{ConnectionId, DriverType};
use crate::domain::query::QueryResult;
use crate::domain::safety::{transaction_control_verb, validate_against_policy, ConnectionSafetyPolicy};
use crate::ports::{TransactionFailure, TransactionFailureOutcome, TransactionStatementResult};

pub(super) struct MultiQueryExecution<'a> {
    pub(super) service: &'a QueryService,
    pub(super) connection_id: &'a ConnectionId,
    pub(super) handle: &'a crate::domain::connection::ConnectionHandle,
    pub(super) policy: &'a ConnectionSafetyPolicy,
    pub(super) driver: DriverType,
    pub(super) statements: &'a [String],
    pub(super) query_statements: &'a [bool],
    pub(super) has_schema_change: bool,
    pub(super) started_at: std::time::Instant,
}

impl MultiQueryExecution<'_> {
    pub(super) fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    fn finish(
        &self,
        results: Vec<QueryResult>,
        result_kinds: Vec<StatementResultKind>,
        error: Option<(usize, MultiQueryError)>,
    ) -> MultiQueryResult {
        MultiQueryResult {
            results,
            result_kinds,
            total_duration_ms: self.elapsed_ms(),
            error,
        }
    }

    pub(super) fn validate_transactional_batch(&self) -> Option<(usize, MultiQueryError)> {
        for (index, statement) in self.statements.iter().enumerate() {
            if let Err(message) = validate_against_policy(statement, self.policy) {
                return Some((index, MultiQueryError::message(message)));
            }
        }

        self.statements.iter().enumerate().find_map(|(index, statement)| {
            transaction_control_verb(statement)
                .map(|verb| (index, MultiQueryError::message(transaction_control_rejection(verb))))
        })
    }

    pub(super) async fn execute_transactional(&self) -> MultiQueryResult {
        if let Some(error) = self.validate_transactional_batch() {
            return self.finish(Vec::new(), Vec::new(), Some(error));
        }

        match self
            .service
            .connector
            .execute_transaction(self.handle, self.statements, self.query_statements)
            .await
        {
            Ok(transaction_results) => {
                if self.has_schema_change {
                    self.service.invalidate_schema_cache(self.connection_id).await;
                }
                self.convert_transaction_results(transaction_results)
            }
            Err(failure) => self.finish_transaction_failure(failure).await,
        }
    }

    async fn finish_transaction_failure(&self, failure: TransactionFailure) -> MultiQueryResult {
        if self.has_schema_change && failure.outcome == TransactionFailureOutcome::Unknown {
            self.service.invalidate_schema_cache(self.connection_id).await;
        }
        let failure_error = failure.error;
        let mut result = self.convert_transaction_results(failure.results);
        if result.error.is_none() {
            result.error = Some((
                failure.statement_index,
                MultiQueryError {
                    message: format_transaction_failure(failure.phase, failure.outcome, &failure_error),
                    ..MultiQueryError::from(failure_error)
                },
            ));
        }
        result
    }

    fn convert_transaction_results(&self, transaction_results: Vec<TransactionStatementResult>) -> MultiQueryResult {
        let mut results = Vec::with_capacity(transaction_results.len());
        let mut result_kinds = Vec::with_capacity(transaction_results.len());
        for (index, transaction_result) in transaction_results.into_iter().enumerate() {
            match super::transaction_result_to_query_result(transaction_result) {
                Ok((kind, result)) => {
                    results.push(result);
                    result_kinds.push(kind);
                }
                Err(error) => {
                    return self.finish(results, result_kinds, Some((index, MultiQueryError::from(error))));
                }
            }
        }
        self.finish(results, result_kinds, None)
    }

    // cc-scan:allow DEEP_NESTING — nesting mirrors structure depth
    pub(super) async fn execute_sequential(&self) -> MultiQueryResult {
        let mut results = Vec::with_capacity(self.statements.len());
        let mut result_kinds = Vec::with_capacity(self.statements.len());

        for (index, statement) in self.statements.iter().enumerate() {
            let statement_started_at = std::time::Instant::now();
            if let Err(message) = validate_against_policy(statement, self.policy) {
                return self.finish(results, result_kinds, Some((index, MultiQueryError::message(message))));
            }

            match classify_statement(statement, self.driver) {
                StatementClass::Read => match self.service.connector.query(self.handle, statement, &[]).await {
                    Ok(result) => {
                        if let Err(error) = result.validate() {
                            return self.finish(results, result_kinds, Some((index, MultiQueryError::message(error))));
                        }
                        results.push(result);
                        result_kinds.push(StatementResultKind::ResultSet);
                    }
                    Err(error) => {
                        return self.finish(results, result_kinds, Some((index, MultiQueryError::from(error))));
                    }
                },
                StatementClass::Write => match self.service.connector.execute(self.handle, statement, &[]).await {
                    Ok(row_count) => {
                        results.push(QueryResult {
                            columns: Vec::new(),
                            rows: Vec::new(),
                            row_count,
                            duration_ms: statement_started_at.elapsed().as_millis() as u64,
                        });
                        result_kinds.push(StatementResultKind::Command);
                    }
                    Err(error) => {
                        return self.finish(results, result_kinds, Some((index, MultiQueryError::from(error))));
                    }
                },
            }
        }

        self.finish(results, result_kinds, None)
    }
}
