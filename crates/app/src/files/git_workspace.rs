//! Optional Git adapter for the Files / IDE workspace (#255).
//!
//! Shells out to `git` when available. DB Pro stays fully usable without Git.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileStatus {
    pub path: String,
    /// Two-letter porcelain status (e.g. ` M`, `M `, `??`, `A `).
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorkspaceStatus {
    pub root: PathBuf,
    pub branch: Option<String>,
    pub available: bool,
    pub message: String,
    pub entries: Vec<GitFileStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiffResult {
    pub path: String,
    pub against: String,
    pub text: String,
}

fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                "git is not installed — workspace still works without Git".to_owned()
            } else {
                format!("failed to spawn git: {err}")
            }
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        Err(if detail.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            detail
        })
    }
}

// ---------------------------------------------------------------------------
// Off-UI-thread execution (#391)
//
// `run_git` above is a blocking `Command::output()`. Views must never call it
// directly from the draw path: the operations below are spawned on a dedicated
// std::thread and report back through a channel the state polls per frame.
// ---------------------------------------------------------------------------

/// A git action the workspace can run off the UI thread.
#[derive(Debug, Clone)]
pub enum GitOperation {
    /// `git rev-parse` + `git status --porcelain` refresh.
    RefreshStatus,
    Stage { relative: String },
    Unstage { relative: String },
    Commit { message: String },
    Diff { relative: String },
}

/// Result of a finished [`GitOperation`]; routed back into `WorkspaceFilesState`.
#[derive(Debug)]
pub enum GitTaskOutcome {
    Status(Result<GitWorkspaceStatus, String>),
    /// Mutating actions re-probe status so the UI reflects the result (#391).
    Mutation {
        result: Result<String, String>,
        status: GitWorkspaceStatus,
    },
    Diff(Result<GitDiffResult, String>),
}

/// Run `operation` on a worker thread; the outcome arrives on the returned
/// receiver so the egui frame never blocks on a subprocess (#391).
pub fn spawn_git_task(root: PathBuf, operation: GitOperation) -> std::sync::mpsc::Receiver<GitTaskOutcome> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let outcome = match operation {
            GitOperation::RefreshStatus => GitTaskOutcome::Status(Ok(probe_git_status(&root))),
            GitOperation::Stage { relative } => GitTaskOutcome::Mutation {
                result: stage_path(&root, &relative).map(|_| format!("Staged {relative}")),
                status: probe_git_status(&root),
            },
            GitOperation::Unstage { relative } => GitTaskOutcome::Mutation {
                result: unstage_path(&root, &relative).map(|_| format!("Unstaged {relative}")),
                status: probe_git_status(&root),
            },
            GitOperation::Commit { message } => GitTaskOutcome::Mutation {
                result: commit_paths(&root, &message),
                status: probe_git_status(&root),
            },
            GitOperation::Diff { relative } => GitTaskOutcome::Diff(diff_against_head(&root, &relative)),
        };
        let _ = sender.send(outcome);
    });
    receiver
}

/// Probe whether `root` is inside a Git work tree and collect porcelain status.
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
pub fn probe_git_status(root: &Path) -> GitWorkspaceStatus {
    match run_git(root, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(out) if out.trim() == "true" => {}
        Ok(_) => {
            return GitWorkspaceStatus {
                root: root.to_path_buf(),
                branch: None,
                available: false,
                message: "Not a Git work tree".into(),
                entries: Vec::new(),
            };
        }
        Err(message) => {
            return GitWorkspaceStatus {
                root: root.to_path_buf(),
                branch: None,
                available: false,
                message,
                entries: Vec::new(),
            };
        }
    }

    let branch = run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let porcelain = run_git(root, &["-c", "core.quotePath=false", "status", "--porcelain", "-uall"]).unwrap_or_default();
    let mut entries = Vec::new();
    for line in porcelain.lines() {
        if line.len() < 4 {
            continue;
        }
        let code = line[..2].to_owned();
        let raw_path = line[3..].trim();
        // Unquote paths if git still quoted them (#469).
        let path = if raw_path.starts_with('"') && raw_path.ends_with('"') && raw_path.len() >= 2 {
            raw_path[1..raw_path.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
        } else {
            raw_path.to_owned()
        };
        if path.is_empty() {
            continue;
        }
        entries.push(GitFileStatus { path, code });
    }
    entries.truncate(200);

    GitWorkspaceStatus {
        root: root.to_path_buf(),
        branch,
        available: true,
        message: format!("{} changed path(s)", entries.len()),
        entries,
    }
}

/// Diff a relative path against HEAD (or show working-tree content for untracked).
pub fn diff_against_head(root: &Path, relative: &str) -> Result<GitDiffResult, String> {
    let text = match run_git(root, &["diff", "HEAD", "--", relative]) {
        Ok(t) if !t.trim().is_empty() => t,
        Ok(_) => match run_git(root, &["diff", "--", relative]) {
            Ok(t) if !t.trim().is_empty() => t,
            _ => {
                let path = root.join(relative);
                // File read errors must be displayed differently from "no diff": combining them
                // would cause I/O errors to be misunderstood as matching HEAD.
                match std::fs::read_to_string(&path) {
                    Ok(body) if !body.is_empty() => format!("--- /dev/null\n+++ b/{relative}\n{body}"),
                    Ok(_) => "(no diff — file matches HEAD or is empty)".into(),
                    Err(error) => format!("(could not read file: {error})"),
                }
            }
        },
        Err(err) => return Err(err),
    };
    Ok(GitDiffResult {
        path: relative.to_owned(),
        against: "HEAD".into(),
        text,
    })
}

pub fn stage_path(root: &Path, relative: &str) -> Result<(), String> {
    run_git(root, &["add", "--", relative]).map(|_| ())
}

pub fn unstage_path(root: &Path, relative: &str) -> Result<(), String> {
    run_git(root, &["restore", "--staged", "--", relative]).map(|_| ())
}

/// Explicit user-triggered commit. Never called automatically.
pub fn commit_paths(root: &Path, message: &str) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("commit message is required".into());
    }
    let lower = message.to_ascii_lowercase();
    for needle in ["password=", "pwd=", "secret=", "private_key"] {
        if lower.contains(needle) {
            return Err("commit message looks like it contains secret material".into());
        }
    }
    run_git(root, &["commit", "-m", message])
}

/// Detect whether the on-disk file changed since `known_mtime` (secs since epoch).
pub fn disk_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// Compare editor buffer to disk without overwriting. Returns true when disk differs.
pub fn disk_diverged_from_buffer(path: &Path, buffer: &str) -> bool {
    match std::fs::read_to_string(path) {
        Ok(disk) => disk != buffer,
        Err(_) => false,
    }
}

#[cfg(test)]
#[path = "tests/git_workspace_tests.rs"]
mod tests;

