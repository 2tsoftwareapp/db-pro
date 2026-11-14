use super::ide_workspace_types::{
    WorkspaceDiagnostic, WorkspaceDiagnosticSeverity, WorkspaceFileNode, WorkspaceIndexEntry, WORKSPACE_MAX_DEPTH,
    WORKSPACE_MAX_ENTRIES,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

const INDEXABLE_EXTENSIONS: &[&str] = &["sql", "md", "txt", "toml", "json", "yml", "yaml", "dbpro-nb"];

/// Hard cap for workspace documents opened into the editor (#377). Reading
/// files is moved off the UI thread, but a multi-hundred-MB buffer would still
/// stall rope insertion and repaint, so oversized files are refused.
pub(super) const WORKSPACE_FILE_MAX_BYTES: u64 = 64 * 1024 * 1024;

fn is_indexable(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        INDEXABLE_EXTENSIONS
            .iter()
            .any(|allowed| ext.eq_ignore_ascii_case(allowed))
    })
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "target" | ".idea" | ".vscode" | "dist" | "build" | ".next"
    )
}

/// Case-insensitive directory order without allocating a lowercased `String`
/// per entry on every scan (#393).
fn cmp_os_str_ci(a: &OsStr, b: &OsStr) -> std::cmp::Ordering {
    a.to_string_lossy()
        .chars()
        .flat_map(char::to_lowercase)
        .cmp(b.to_string_lossy().chars().flat_map(char::to_lowercase))
        .then_with(|| a.cmp(b))
}

pub(super) fn scan_directory(
    root: &Path,
    dir: &Path,
    depth: usize,
    count: &mut usize,
) -> Result<Vec<WorkspaceFileNode>, String> {
    if depth > WORKSPACE_MAX_DEPTH || *count >= WORKSPACE_MAX_ENTRIES {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(dir).map_err(|error| format!("read {}: {error}", dir.display()))?;
    let mut nodes = Vec::new();
    let mut collected = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("read {}: {error}", dir.display()))?;
        collected.push(entry);
    }
    collected.sort_by(|a, b| {
        let a_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        b_dir
            .cmp(&a_dir)
            .then_with(|| cmp_os_str_ci(&a.file_name(), &b.file_name()))
    });

    for entry in collected {
        if *count >= WORKSPACE_MAX_ENTRIES {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && name != ".env.example" && name != ".dbpro" && name != ".sqlfluff" {
            continue;
        }
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        // `file_type` does not follow links, so skip symlinks explicitly — a
        // symlinked dir would otherwise recurse into link cycles (#393).
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if should_skip_dir(&name) {
                continue;
            }
            *count += 1;
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let children = scan_directory(root, &path, depth + 1, count)?;
            nodes.push(WorkspaceFileNode {
                name,
                relative_path: relative,
                absolute_path: path,
                is_dir: true,
                children,
            });
        } else if is_indexable(&path) {
            *count += 1;
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            nodes.push(WorkspaceFileNode {
                name,
                relative_path: relative,
                absolute_path: path,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }
    Ok(nodes)
}

pub(super) fn flatten_index(root_id: &str, nodes: &[WorkspaceFileNode]) -> Vec<WorkspaceIndexEntry> {
    let mut out = Vec::new();
    fn walk(root_id: &str, nodes: &[WorkspaceFileNode], out: &mut Vec<WorkspaceIndexEntry>) {
        for node in nodes {
            if node.is_dir {
                walk(root_id, &node.children, out);
            } else {
                out.push(WorkspaceIndexEntry {
                    root_id: root_id.to_owned(),
                    is_sql: node
                        .relative_path
                        .rsplit('.')
                        .next()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("sql")),
                    relative_path: node.relative_path.clone(),
                });
            }
        }
    }
    walk(root_id, nodes, &mut out);
    out
}

/// Diagnostics over an already-built index; shared by the sync path and the
/// background scan worker so file reads stay off the draw path (#393).
pub(super) fn index_diagnostics(
    root_id: &str,
    root_path: &Path,
    index: &[WorkspaceIndexEntry],
) -> Vec<WorkspaceDiagnostic> {
    let mut diagnostics = Vec::new();
    for entry in index.iter().filter(|entry| entry.is_sql) {
        let path = root_path.join(&entry.relative_path);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("SELECT *") {
                diagnostics.push(WorkspaceDiagnostic {
                    root_id: root_id.to_owned(),
                    relative_path: entry.relative_path.clone(),
                    line: line_idx + 1,
                    message: "avoid SELECT * in workspace SQL".to_owned(),
                    severity: WorkspaceDiagnosticSeverity::Warning,
                });
            }
            if trimmed.contains("DROP TABLE") && !trimmed.contains("IF EXISTS") {
                diagnostics.push(WorkspaceDiagnostic {
                    root_id: root_id.to_owned(),
                    relative_path: entry.relative_path.clone(),
                    line: line_idx + 1,
                    message: "DROP TABLE without IF EXISTS".to_owned(),
                    severity: WorkspaceDiagnosticSeverity::Error,
                });
            }
        }
    }
    diagnostics
}

// ---------------------------------------------------------------------------
// Off-UI-thread workspace I/O (#377, #393)
//
// `scan_directory` and `read_to_string` block; views must never run them on the
// draw path. The operations below are spawned on a dedicated std::thread and
// report back through a channel the state polls per frame — same shape as
// `git_workspace::spawn_git_task` (#391).
// ---------------------------------------------------------------------------

/// One root scanned on a worker thread, ready to be applied to UI state.
#[derive(Debug)]
pub(super) struct ScannedRoot {
    pub(super) id: String,
    pub(super) path: PathBuf,
    pub(super) result: Result<(Vec<WorkspaceFileNode>, Vec<WorkspaceIndexEntry>), String>,
    pub(super) diagnostics: Vec<WorkspaceDiagnostic>,
}

fn scan_root(id: String, path: PathBuf) -> ScannedRoot {
    let result = (|| {
        if !path.is_dir() {
            return Err(format!("not a folder: {}", path.display()));
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        let mut count = 0usize;
        let tree = scan_directory(&canonical, &canonical, 0, &mut count)?;
        let index = flatten_index(&id, &tree);
        Ok::<_, String>((canonical, tree, index))
    })();
    match result {
        Ok((canonical, tree, index)) => {
            let diagnostics = index_diagnostics(&id, &canonical, &index);
            ScannedRoot {
                id,
                path: canonical,
                result: Ok((tree, index)),
                diagnostics,
            }
        }
        Err(error) => ScannedRoot {
            id,
            path,
            result: Err(error),
            diagnostics: Vec::new(),
        },
    }
}

/// Scan `(id, path)` roots on a worker thread; the outcome arrives on the
/// returned receiver so the egui frame never blocks on `read_dir` (#393).
pub(super) fn spawn_scan_task(roots: Vec<(String, PathBuf)>) -> std::sync::mpsc::Receiver<Vec<ScannedRoot>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let scanned = roots
            .into_iter()
            .map(|(id, path)| scan_root(id, path))
            .collect();
        let _ = sender.send(scanned);
    });
    receiver
}

/// What a finished file read should do once the content arrives.
#[derive(Debug)]
pub(super) enum FileReadOutcome {
    /// Open the file as a new document (or switch to it if already open).
    Open {
        path: PathBuf,
        result: Result<(String, Option<u64>), String>,
    },
}

/// Read `path` on a worker thread with a size cap; the outcome arrives on the
/// returned receiver so the egui frame never blocks on disk I/O (#377).
pub(super) fn spawn_file_read_task(path: PathBuf) -> std::sync::mpsc::Receiver<FileReadOutcome> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let metadata = std::fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            if metadata.len() > WORKSPACE_FILE_MAX_BYTES {
                return Err(format!(
                    "{} is {} MB — too large to open in the editor ({} MB limit)",
                    path.display(),
                    metadata.len() / (1024 * 1024),
                    WORKSPACE_FILE_MAX_BYTES / (1024 * 1024)
                ));
            }
            let content = std::fs::read_to_string(&path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::InvalidData {
                    format!("{} is not valid UTF-8 — convert its encoding first", path.display())
                } else {
                    format!("{}: {error}", path.display())
                }
            })?;
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs());
            Ok((content, mtime))
        })();
        let _ = sender.send(FileReadOutcome::Open { path, result });
    });
    receiver
}

// ---------------------------------------------------------------------------
// Off-UI-thread shell task runner (#579)
// ---------------------------------------------------------------------------

/// One line of live output, or the terminal event of a shell task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TaskEvent {
    /// A decoded stdout line (stderr lines are prefixed with `! `).
    Line(String),
    /// The process exited (or the runner failed); always the last event.
    Exit(Option<i32>),
}

/// Cap on the live output ring the UI keeps per task (last N lines).
pub(super) const TASK_OUTPUT_MAX_LINES: usize = 400;

/// A shell command running on a worker thread (#579).
///
/// `run_task` used to call `Command::output()` from the reducer, so `sleep 30`
/// or a hung `npm test` froze the whole app with no way to cancel. The worker
/// streams lines through the channel and the UI polls it per frame, exactly
/// like the scan/file-read workers (#377, #393).
#[derive(Debug)]
pub(super) struct WorkspaceTaskHandle {
    pub(super) command: String,
    pub(super) receiver: std::sync::mpsc::Receiver<TaskEvent>,
    /// Kept so the Cancel button can `child.kill()`.
    pub(super) child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
    pub(super) started: std::time::Instant,
}

impl WorkspaceTaskHandle {
    /// Ask the OS to terminate the process. Best effort: the worker still owns
    /// the pipes and reports `Exit` afterwards.
    pub(super) fn kill(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }

    pub(super) fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

/// Spawn `command_line` through the platform shell in `cwd`.
pub(super) fn spawn_task_task(command_line: String, cwd: PathBuf) -> std::io::Result<WorkspaceTaskHandle> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", &command_line]);
        command
    };
    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut command = Command::new("sh");
        command.args(["-lc", &command_line]);
        command
    };
    let child = command
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    let (sender, receiver) = std::sync::mpsc::channel();
    spawn_task_reader(child.clone(), sender.clone());
    // The reaper must be the only thread that calls `wait()`: a second wait
    // would race the reader that owns stdout/stderr draining.
    std::thread::spawn(move || {
        let status = child.lock().ok().and_then(|mut child| child.wait().ok());
        let _ = sender.send(TaskEvent::Exit(status.and_then(|status| status.code())));
    });
    Ok(WorkspaceTaskHandle {
        command: command_line,
        receiver,
        child,
        started: std::time::Instant::now(),
    })
}

fn spawn_task_reader(
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
    sender: std::sync::mpsc::Sender<TaskEvent>,
) {
    let mut taken = child.lock().ok().and_then(|mut child| {
        Some((
            child.stdout.take(),
            child.stderr.take(),
        ))
    });
    let (stdout, stderr) = match taken.take() {
        Some((stdout, stderr)) => (stdout, stderr),
        None => return,
    };
    if let Some(stdout) = stdout {
        let sender = sender.clone();
        std::thread::spawn(move || stream_task_lines(stdout, "", sender));
    }
    if let Some(stderr) = stderr {
        std::thread::spawn(move || stream_task_lines(stderr, "! ", sender));
    }
}

fn stream_task_lines<R: std::io::Read + Send + 'static>(
    reader: R,
    prefix: &'static str,
    sender: std::sync::mpsc::Sender<TaskEvent>,
) {
    use std::io::BufRead;
    for line in std::io::BufReader::new(reader).lines() {
        let Ok(line) = line else {
            break;
        };
        if sender.send(TaskEvent::Line(format!("{prefix}{line}"))).is_err() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Off-UI-thread workspace reads (#579)
// ---------------------------------------------------------------------------

/// Root/index snapshot the search worker reads from.
pub(super) type WorkspaceReadSnapshot = Vec<(PathBuf, Vec<String>)>;

/// Result of a workspace-wide read: hits, or replacement previews/counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkspaceReadOutcome {
    /// `search`-style hits, capped by the caller's limit.
    Hits(Vec<super::ide_workspace_types::SearchHit>),
    /// `preview_replace`-style counts.
    Previews(Vec<super::ide_workspace_types::ReplacePreview>),
    /// Files rewritten by `apply_replace`.
    Replaced(Result<usize, String>),
}

/// `(root_id, root_path, index)` triples collected on the UI thread (`roots`
/// and `index` are plain data — cloning them is far cheaper than the file I/O
/// they unlock).
pub(super) fn read_snapshot(
    roots: &[super::ide_workspace_types::WorkspaceRoot],
) -> WorkspaceReadSnapshot {
    roots
        .iter()
        .map(|root| {
            (
                root.path.clone(),
                root.index
                    .iter()
                    .map(|entry| entry.relative_path.clone())
                    .collect(),
            )
        })
        .collect()
}

/// Run a workspace-wide search on a worker thread (#579).
pub(super) fn spawn_search_task(
    snapshot: WorkspaceReadSnapshot,
    root_ids: Vec<String>,
    query: String,
    limit: usize,
) -> std::sync::mpsc::Receiver<WorkspaceReadOutcome> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let hits = search_snapshot(&snapshot, &root_ids, &query, limit);
        let _ = sender.send(WorkspaceReadOutcome::Hits(hits));
    });
    receiver
}

/// Preview replacements for every indexed file on a worker thread (#579).
pub(super) fn spawn_preview_replace_task(
    snapshot: WorkspaceReadSnapshot,
    root_ids: Vec<String>,
    query: String,
) -> std::sync::mpsc::Receiver<WorkspaceReadOutcome> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let previews = preview_replace_snapshot(&snapshot, &root_ids, &query);
        let _ = sender.send(WorkspaceReadOutcome::Previews(previews));
    });
    receiver
}

/// Apply replacements on a worker thread (#579).
pub(super) fn spawn_apply_replace_task(
    snapshot: WorkspaceReadSnapshot,
    query: String,
    replacement: String,
) -> std::sync::mpsc::Receiver<WorkspaceReadOutcome> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = apply_replace_snapshot(&snapshot, &query, &replacement);
        let _ = sender.send(WorkspaceReadOutcome::Replaced(result));
    });
    receiver
}

fn walk_snapshot<F: FnMut(&str, &str, &str)>(snapshot: &WorkspaceReadSnapshot, root_ids: &[String], mut visit: F) {
    for (index, (root_path, entries)) in snapshot.iter().enumerate() {
        let root_id = root_ids.get(index).map_or("", String::as_str);
        for relative in entries {
            let path = root_path.join(relative);
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            visit(root_id, relative, &content);
        }
    }
}

pub(super) fn search_snapshot(
    snapshot: &WorkspaceReadSnapshot,
    root_ids: &[String],
    query: &str,
    limit: usize,
) -> Vec<super::ide_workspace_types::SearchHit> {
    use super::ide_workspace_types::SearchHit;
    let mut hits = Vec::new();
    if query.is_empty() {
        return hits;
    }
    let needle = query.to_lowercase();
    walk_snapshot(snapshot, root_ids, |root_id, relative, content| {
        for (line_idx, line) in content.lines().enumerate() {
            if hits.len() >= limit {
                return;
            }
            if line.to_lowercase().contains(&needle) {
                hits.push(SearchHit {
                    root_id: root_id.to_owned(),
                    relative_path: relative.to_owned(),
                    line: line_idx + 1,
                    preview: line.chars().take(160).collect(),
                });
            }
        }
    });
    hits
}

pub(super) fn preview_replace_snapshot(
    snapshot: &WorkspaceReadSnapshot,
    root_ids: &[String],
    query: &str,
) -> Vec<super::ide_workspace_types::ReplacePreview> {
    use super::ide_workspace_types::ReplacePreview;
    let mut previews = Vec::new();
    if query.is_empty() {
        return previews;
    }
    walk_snapshot(snapshot, root_ids, |root_id, relative, content| {
        let count = content.matches(query).count();
        if count > 0 {
            previews.push(ReplacePreview {
                root_id: root_id.to_owned(),
                relative_path: relative.to_owned(),
                replacements: count,
            });
        }
    });
    previews
}

pub(super) fn apply_replace_snapshot(
    snapshot: &WorkspaceReadSnapshot,
    query: &str,
    replacement: &str,
) -> Result<usize, String> {
    if query.is_empty() {
        return Err("empty search".to_owned());
    }
    let mut total = 0usize;
    let mut failure = None;
    walk_snapshot_fallible(snapshot, query, replacement, &mut total, &mut failure);
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(total)
}

pub(super) fn walk_snapshot_fallible(
    snapshot: &WorkspaceReadSnapshot,
    query: &str,
    replacement: &str,
    total: &mut usize,
    failure: &mut Option<String>,
) {
    for (root_path, entries) in snapshot {
        for relative in entries {
            if let Err(error) = replace_one(&root_path.join(relative), query, replacement, total) {
                *failure = Some(error);
                return;
            }
        }
    }
}

fn replace_one(
    path: &Path,
    query: &str,
    replacement: &str,
    total: &mut usize,
) -> Result<(), String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(()); // unreadable/gone: not a failure of the replace itself
    };
    if !content.contains(query) {
        return Ok(());
    }
    std::fs::write(path, content.replace(query, replacement)).map_err(|error| error.to_string())?;
    *total += content.matches(query).count();
    Ok(())
}
