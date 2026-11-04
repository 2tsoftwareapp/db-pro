//! Tests for `../transaction_control.rs` (moved out of the source file).
use super::{transaction_control, TransactionControl};

#[test]
fn simple_controls_and_begin_modes_are_recognized() {
    assert_eq!(transaction_control("BEGIN"), Some(TransactionControl::Begin));
    assert_eq!(transaction_control("  begin ; "), Some(TransactionControl::Begin));
    assert_eq!(
        transaction_control("START TRANSACTION"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("BEGIN ISOLATION LEVEL SERIALIZABLE"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(transaction_control("COMMIT WORK"), Some(TransactionControl::Commit));
    assert_eq!(transaction_control("ROLLBACK"), Some(TransactionControl::Rollback));
}

#[test]
fn savepoints_and_chained_commit_are_not_session_edges() {
    assert_eq!(transaction_control("ROLLBACK TO SAVEPOINT s"), None);
    assert_eq!(transaction_control("COMMIT AND CHAIN"), None);
    assert_eq!(transaction_control("SELECT 1"), None);
}

#[test]
fn start_transaction_modes_are_recognized() {
    assert_eq!(
        transaction_control("START TRANSACTION READ ONLY"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("START TRANSACTION READ ONLY;"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("START TRANSACTION READ WRITE"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("START TRANSACTION READ WRITE;"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("START TRANSACTION ISOLATION LEVEL SERIALIZABLE"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("START TRANSACTION ISOLATION LEVEL SERIALIZABLE;"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("start transaction isolation level repeatable read, read only"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("START TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ ONLY, DEFERRABLE;"),
        Some(TransactionControl::Begin)
    );
}

#[test]
fn begin_commit_and_rollback_variants_preserve_classification() {
    assert_eq!(transaction_control("BEGIN WORK"), Some(TransactionControl::Begin));
    assert_eq!(
        transaction_control("BEGIN TRANSACTION"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(transaction_control("BEGIN READ ONLY"), Some(TransactionControl::Begin));
    assert_eq!(transaction_control("BEGIN READ WRITE"), Some(TransactionControl::Begin));
    assert_eq!(
        transaction_control("begin read write;"),
        Some(TransactionControl::Begin)
    );
    assert_eq!(
        transaction_control("start transaction;"),
        Some(TransactionControl::Begin)
    );

    assert_eq!(transaction_control("COMMIT"), Some(TransactionControl::Commit));
    assert_eq!(transaction_control("COMMIT WORK"), Some(TransactionControl::Commit));
    assert_eq!(
        transaction_control("COMMIT TRANSACTION"),
        Some(TransactionControl::Commit)
    );
    assert_eq!(transaction_control("END"), Some(TransactionControl::Commit));
    assert_eq!(transaction_control("END WORK"), Some(TransactionControl::Commit));
    assert_eq!(transaction_control("END TRANSACTION"), Some(TransactionControl::Commit));
    assert_eq!(transaction_control("commit;"), Some(TransactionControl::Commit));
    assert_eq!(transaction_control("commit work;"), Some(TransactionControl::Commit));
    assert_eq!(
        transaction_control("commit transaction;"),
        Some(TransactionControl::Commit)
    );
    assert_eq!(transaction_control("end;"), Some(TransactionControl::Commit));
    assert_eq!(transaction_control("end work;"), Some(TransactionControl::Commit));
    assert_eq!(
        transaction_control("end transaction;"),
        Some(TransactionControl::Commit)
    );

    assert_eq!(transaction_control("ROLLBACK"), Some(TransactionControl::Rollback));
    assert_eq!(transaction_control("ROLLBACK WORK"), Some(TransactionControl::Rollback));
    assert_eq!(
        transaction_control("ROLLBACK TRANSACTION"),
        Some(TransactionControl::Rollback)
    );
    assert_eq!(transaction_control("ABORT"), Some(TransactionControl::Rollback));
    assert_eq!(transaction_control("ABORT WORK"), Some(TransactionControl::Rollback));
    assert_eq!(transaction_control("rollback;"), Some(TransactionControl::Rollback));
    assert_eq!(
        transaction_control("rollback work;"),
        Some(TransactionControl::Rollback)
    );
    assert_eq!(
        transaction_control("rollback transaction;"),
        Some(TransactionControl::Rollback)
    );
    assert_eq!(transaction_control("abort;"), Some(TransactionControl::Rollback));
    assert_eq!(transaction_control("abort work;"), Some(TransactionControl::Rollback));
}
