//! Tests for `../ide_workspace.rs` (moved out of the source file).
    use super::*;

    fn temp_dir() -> PathBuf {
        static CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = CTR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
        let dir = std::env::temp_dir().join(format!("dbpro-workspace-{nanos}-{count}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn multi_root_and_replace_require_trust() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("migrations")).unwrap();
        std::fs::write(dir.join("migrations/001_init.sql"), "CREATE TABLE users (id INT);").unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        assert!(workspace.apply_replace("users", "accounts").is_err());
        workspace.set_trusted(true);
        let count = workspace.apply_replace("users", "accounts").unwrap();
        assert!(count >= 1);
        let content = std::fs::read_to_string(dir.join("migrations/001_init.sql")).unwrap();
        assert!(content.contains("accounts"));
        let second = temp_dir();
        std::fs::write(second.join("extra.sql"), "SELECT 1;").unwrap();
        workspace.add_root(second.clone()).unwrap();
        assert_eq!(workspace.roots.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(second);
    }

    #[test]
    fn migrations_and_diagnostics_scan() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("migrations")).unwrap();
        std::fs::write(dir.join("migrations/002_bad.sql"), "SELECT * FROM t;\nDROP TABLE t;\n").unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        let migrations = workspace.detect_migrations();
        assert_eq!(migrations.len(), 1);
        workspace.scan_diagnostics();
        assert!(workspace.workspace_diagnostics.len() >= 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn notebook_patch_sandbox_and_sql_tests_helpers() {
        let dir = temp_dir();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.set_trusted(true);
        let nb = dir.join("notes.dbpro-nb");
        let cells = vec![
            NotebookCell {
                kind: NotebookCellKind::Markdown,
                source: "# hello".to_owned(),
            },
            NotebookCell {
                kind: NotebookCellKind::Sql,
                source: "SELECT 1;".to_owned(),
            },
        ];
        IdeWorkspaceState::save_notebook(&nb, &cells).unwrap();
        let loaded = IdeWorkspaceState::load_notebook(&nb).unwrap();
        assert_eq!(loaded.len(), 2);
        workspace.apply_workspace_patch("patch.sql", "SELECT 2;").unwrap();
        assert!(dir.join("patch.sql").exists());
        let _ = workspace.rename_path("patch.sql", "patched.sql");
        let refs = workspace.find_references("SELECT", 10);
        assert!(!refs.is_empty());
        let tests = IdeWorkspaceState::parse_sql_tests("-- test: a\nSELECT 1;\n-- test: b\nSELECT 2;");
        assert_eq!(tests.len(), 2);
        let source_db = dir.join("demo.sqlite");
        std::fs::write(&source_db, b"sqlite").unwrap();
        let sandbox = workspace.create_sandbox_sqlite_copy(&source_db).unwrap();
        assert!(sandbox.exists());
        assert!(workspace.active_environment().is_some());
        let _ = IdeWorkspaceState::qualified_relative("root-1", "a.sql");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn workspace_paths_reject_traversal_and_absolute_paths() {
        let dir = temp_dir();
        let outside = temp_dir();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        let root_id = workspace.primary_root().unwrap().id.clone();

        assert!(workspace.absolute_for_relative("../outside.txt").is_none());
        assert!(workspace.absolute_for_relative("nested/../../outside.txt").is_none());
        assert!(workspace.absolute_for_relative(outside.join("outside.txt").to_str().unwrap()).is_none());
        assert!(workspace
            .absolute_for_relative(&IdeWorkspaceState::qualified_relative(&root_id, "../outside.txt"))
            .is_none());

        assert!(workspace.create_file("../", "outside.txt", "nope").is_err());
        assert!(workspace.create_file("", "../outside.txt", "nope").is_err());
        assert!(workspace.create_file("", "nested/outside.txt", "nope").is_err());
        assert!(workspace.rename_path("patch.sql", "nested/renamed.sql").is_err());
        assert!(workspace.create_folder("", "../outside-dir").is_err());
        assert!(workspace.rename_path("../outside.txt", "renamed.txt").is_err());
        assert!(workspace.delete_path("../outside.txt").is_err());
        assert!(workspace.delete_path(".").is_err());
        assert!(workspace.delete_path("").is_err());
        assert!(workspace.rename_path(".", "renamed-root").is_err());
        assert!(workspace.rename_path("", "renamed-root").is_err());
        assert!(dir.is_dir());
        workspace.set_trusted(true);
        assert!(workspace.apply_workspace_patch("../outside.txt", "nope").is_err());
        assert!(!outside.join("outside.txt").exists());

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn delete_workspace_symlink_does_not_delete_external_target() {        use std::os::unix::fs::symlink;

        let dir = temp_dir();
        let outside = temp_dir();
        let target = outside.join("keep.txt");
        std::fs::write(&target, "keep me").unwrap();
        symlink(&target, dir.join("external-link.txt")).unwrap();
        symlink(&outside, dir.join("external-dir-link")).unwrap();

        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.delete_path("external-link.txt").unwrap();
        assert!(workspace.delete_path("external-dir-link/keep.txt").is_err());

        assert!(target.exists());
        assert!(!dir.join("external-link.txt").exists());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn rename_dir_remaps_expanded_state_513() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src/sub1")).unwrap();
        std::fs::write(dir.join("src/sub1/note.sql"), "SELECT 1;").unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.expanded.insert("src".to_owned());
        workspace.expanded.insert("src/sub1".to_owned());
        workspace.rename_path("src", "source").unwrap();
        assert!(
            workspace.expanded.contains("source"),
            "expanded should remap src -> source, got {:?}",
            workspace.expanded
        );
        assert!(
            workspace.expanded.contains("source/sub1"),
            "expanded should remap src/sub1 -> source/sub1, got {:?}",
            workspace.expanded
        );
        assert!(!workspace.expanded.contains("src"));
        assert!(!workspace.expanded.contains("src/sub1"));
        assert!(dir.join("source/sub1/note.sql").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rename_nested_dir_remaps_only_its_subtree_513() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src/sub1/deep")).unwrap();
        std::fs::create_dir_all(dir.join("src/other")).unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        for key in ["src", "src/sub1", "src/sub1/deep", "src/other"] {
            workspace.expanded.insert(key.to_owned());
        }
        workspace.rename_path("src/sub1", "renamed").unwrap();
        assert!(workspace.expanded.contains("src"));
        assert!(workspace.expanded.contains("src/other"));
        assert!(workspace.expanded.contains("src/renamed"));
        assert!(workspace.expanded.contains("src/renamed/deep"));
        assert!(!workspace.expanded.contains("src/sub1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rename_dir_does_not_touch_sibling_prefix_513() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("src2")).unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.expanded.insert("src".to_owned());
        workspace.expanded.insert("src2".to_owned());
        workspace.rename_path("src", "source").unwrap();
        assert!(workspace.expanded.contains("source"));
        assert!(workspace.expanded.contains("src2"));
        assert!(!workspace.expanded.contains("src"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rename_file_leaves_parent_expanded_513() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.sql"), "SELECT 1;").unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.expanded.insert("src".to_owned());
        workspace.rename_path("src/a.sql", "b.sql").unwrap();
        assert!(workspace.expanded.contains("src"));
        assert!(dir.join("src/b.sql").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_dir_prunes_expanded_subtree_513() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src/sub1")).unwrap();
        std::fs::create_dir_all(dir.join("keep")).unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        for key in ["src", "src/sub1", "keep"] {
            workspace.expanded.insert(key.to_owned());
        }
        workspace.delete_path("src").unwrap();
        assert!(!workspace.expanded.contains("src"));
        assert!(!workspace.expanded.contains("src/sub1"));
        assert!(workspace.expanded.contains("keep"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dependency_edges_are_cached_until_the_index_epoch_moves_578() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("migrations")).unwrap();
        std::fs::write(
            dir.join("migrations/001_init.sql"),
            "SELECT * FROM users JOIN accounts ON accounts.id = users.id;\n",
        )
        .unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();

        let epoch = workspace.index_epoch();
        let first = workspace.dependency_edges().to_vec();
        assert!(!first.is_empty(), "graph should see the FROM/JOIN refs");
        // Second call is served from the cache: same epoch, same data.
        let second = workspace.dependency_edges().to_vec();
        assert_eq!(first, second);
        assert!(workspace.dependency_edge_cache.is_some());
        assert_eq!(
            workspace.dependency_edge_cache.as_ref().map(|c| c.index_epoch),
            Some(epoch)
        );

        // Rewriting the file must not be observed while the epoch is stable.
        std::fs::write(
            dir.join("migrations/001_init.sql"),
            "SELECT * FROM brand_new_table;\n",
        )
        .unwrap();
        assert_eq!(workspace.dependency_edges().to_vec(), first);

        // A rescan bumps the epoch and drops the cache.
        workspace.refresh().unwrap();
        assert!(workspace.index_epoch() > epoch);
        let refreshed = workspace.dependency_edges().to_vec();
        assert!(refreshed
            .iter()
            .any(|edge| edge.object_name.contains("brand_new_table")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn closing_the_workspace_invalidates_dependency_edges_578() {
        let dir = temp_dir();
        std::fs::write(dir.join("q.sql"), "SELECT * FROM orders;\n").unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        let epoch = workspace.index_epoch();
        assert!(!workspace.dependency_edges().is_empty());
        workspace.close();
        assert!(workspace.index_epoch() > epoch);
        assert!(workspace.dependency_edges().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_and_delete_update_the_index_without_a_rescan_579() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("sql")).unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.set_trusted(true);
        let epoch = workspace.index_epoch();

        let created = workspace.create_file("sql", "new.sql", "SELECT 1;\n").unwrap();
        assert!(created.exists());
        assert!(workspace.index_epoch() > epoch);
        assert!(workspace
            .roots
            .iter()
            .flat_map(|root| root.index.iter())
            .any(|entry| entry.relative_path == "sql/new.sql" && entry.is_sql));

        workspace.delete_path("sql/new.sql").unwrap();
        assert!(!workspace
            .roots
            .iter()
            .flat_map(|root| root.index.iter())
            .any(|entry| entry.relative_path == "sql/new.sql"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shell_tasks_stream_and_cancel_579() {
        let dir = temp_dir();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.set_trusted(true);
        let cwd = dir.clone();
        let handle =
            ide_workspace_scan::spawn_task_task("echo hello-db-pro".to_owned(), cwd.clone()).unwrap();
        let mut lines = Vec::new();
        let mut code = None;
        // The worker signals Exit last, so this terminates.
        for _ in 0..200 {
            match handle.receiver.try_recv() {
                Ok(ide_workspace_scan::TaskEvent::Line(line)) => lines.push(line),
                Ok(ide_workspace_scan::TaskEvent::Exit(exit)) => {
                    code = Some(exit);
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
        assert_eq!(code, Some(Some(0)));
        assert!(lines.iter().any(|line| line.contains("hello-db-pro")));

        // Cancel path: a long-running command must not block the caller.
        let slow = ide_workspace_scan::spawn_task_task("sleep 30".to_owned(), cwd).unwrap();
        let started = std::time::Instant::now();
        slow.kill();
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn read_workers_match_the_blocking_search_579() {
        let dir = temp_dir();
        std::fs::write(dir.join("a.sql"), "SELECT * FROM orders;\n").unwrap();
        std::fs::write(dir.join("b.sql"), "SELECT * FROM customers;\n").unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.set_trusted(true);

        let blocking = workspace.search("orders", 10);
        let receiver = ide_workspace_scan::spawn_search_task(
            workspace.read_snapshot(),
            workspace.root_ids(),
            "orders".to_owned(),
            10,
        );
        match receiver.recv().unwrap() {
            ide_workspace_scan::WorkspaceReadOutcome::Hits(hits) => assert_eq!(blocking, hits),
            other => panic!("unexpected outcome: {other:?}"),
        }

        let receiver = ide_workspace_scan::spawn_apply_replace_task(
            workspace.read_snapshot(),
            "customers".to_owned(),
            "clients".to_owned(),
        );
        match receiver.recv().unwrap() {
            ide_workspace_scan::WorkspaceReadOutcome::Replaced(result) => {
                assert_eq!(result.unwrap(), 1);
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
        let updated = std::fs::read_to_string(dir.join("b.sql")).unwrap();
        assert!(updated.contains("clients"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// #579: replace-in-files rewrites contents only, so it must not rescan the
    /// tree — but cached dependency edges must still be dropped.
    #[test]
    fn apply_replace_keeps_the_index_and_drops_edge_cache_579() {
        let dir = temp_dir();
        std::fs::write(dir.join("a.sql"), "SELECT 1;\n").unwrap();
        std::fs::write(dir.join("b.sql"), "SELECT customers FROM t;\n").unwrap();
        let mut workspace = IdeWorkspaceState::default();
        workspace.open_root(dir.clone()).unwrap();
        workspace.set_trusted(true);
        assert!(!workspace.dependency_edges().is_empty());
        let epoch_before = workspace.index_epoch;
        let index_before = workspace.roots[0].index.clone();

        let count = workspace.apply_replace("customers", "clients").unwrap();
        assert_eq!(count, 1);

        // The file set is unchanged, so the index is untouched — and the cached
        // edges were dropped even though no rescan happened.
        assert_eq!(workspace.roots[0].index, index_before);
        assert!(workspace.index_epoch > epoch_before);
        assert!(workspace.dependency_edge_cache.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
