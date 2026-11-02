//! Git, through the `git` program: what the Git panel shows and does.
//!
//! Every command runs in the repository's folder and answers with text that
//! is parsed here (porcelain formats, never what a human is meant to read).
//! Commands are async so the window never waits for one.

pub mod diff;
pub mod graph;
pub mod history;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub use diff::{parse_diff, FileDiff};

/// Runs `git args…` in `repo`; the output, or what git complained about.
pub async fn run(repo: &Path, args: &[&str]) -> Result<String, String> {
    run_with(repo, args, None, &[]).await
}

/// Like [`run`], with text for git's standard input and extra environment.
pub async fn run_with(
    repo: &Path,
    args: &[&str],
    input: Option<&str>,
    env: &[(&str, &str)],
) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(repo)
        // Never wait for a password or an editor nobody can see.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", "true")
        .envs(env.iter().copied())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| format!("git: {e}"))?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(text.as_bytes()).await;
        drop(stdin);
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| "git: timeout".to_string())?
    .map_err(|e| format!("git: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr
        })
    }
}

/// The top folder of the repository holding `path` (a file or a folder).
pub async fn repo_root(path: &Path) -> Option<PathBuf> {
    let dir = if path.is_dir() { path } else { path.parent()? };
    let out = run(dir, &["rev-parse", "--show-toplevel"]).await.ok()?;
    let top = out.trim();
    (!top.is_empty()).then(|| PathBuf::from(top))
}

// ─── Status ─────────────────────────────────────────────────────────────────

/// What changed in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl Kind {
    /// The letter shown beside the file.
    pub fn letter(self) -> &'static str {
        match self {
            Kind::Modified => "M",
            Kind::Added => "A",
            Kind::Deleted => "D",
            Kind::Renamed => "R",
            Kind::Untracked => "U",
            Kind::Conflict => "!",
        }
    }

    fn of(code: u8) -> Option<Kind> {
        match code {
            b'M' | b'T' => Some(Kind::Modified),
            b'A' | b'C' => Some(Kind::Added),
            b'D' => Some(Kind::Deleted),
            b'R' => Some(Kind::Renamed),
            _ => None,
        }
    }
}

/// A changed file, relative to the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    /// Where a renamed file came from.
    pub from: Option<String>,
    pub kind: Kind,
}

/// The state of the working tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// In the index (`git add`ed), against HEAD.
    pub staged: Vec<Change>,
    /// In the working tree, against the index (untracked files included).
    pub unstaged: Vec<Change>,
}

pub async fn status(repo: &Path) -> Result<Status, String> {
    let out = run(
        repo,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )
    .await?;
    Ok(parse_status(&out))
}

pub fn parse_status(out: &str) -> Status {
    let mut status = Status::default();
    let mut fields = out.split('\0');
    while let Some(entry) = fields.next() {
        if let Some(rest) = entry.strip_prefix("# branch.head ") {
            status.branch = rest.to_string();
        } else if let Some(rest) = entry.strip_prefix("# branch.upstream ") {
            status.upstream = Some(rest.to_string());
        } else if let Some(rest) = entry.strip_prefix("# branch.ab ") {
            for part in rest.split(' ') {
                if let Some(n) = part.strip_prefix('+') {
                    status.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    status.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(path) = entry.strip_prefix("? ") {
            status.unstaged.push(Change {
                path: path.to_string(),
                from: None,
                kind: Kind::Untracked,
            });
        } else if let Some(rest) = entry.strip_prefix("1 ") {
            // 1 XY sub mH mI mW hH hI path
            let parts: Vec<&str> = rest.splitn(8, ' ').collect();
            if parts.len() == 8 {
                push_xy(&mut status, parts[0].as_bytes(), parts[7], None);
            }
        } else if let Some(rest) = entry.strip_prefix("2 ") {
            // 2 XY sub mH mI mW hH hI Xscore path, then the original path.
            let parts: Vec<&str> = rest.splitn(9, ' ').collect();
            let from = fields.next().map(str::to_string);
            if parts.len() == 9 {
                push_xy(&mut status, parts[0].as_bytes(), parts[8], from);
            }
        } else if let Some(rest) = entry.strip_prefix("u ") {
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            let parts: Vec<&str> = rest.splitn(10, ' ').collect();
            if parts.len() == 10 {
                status.unstaged.push(Change {
                    path: parts[9].to_string(),
                    from: None,
                    kind: Kind::Conflict,
                });
            }
        }
    }
    status
}

fn push_xy(status: &mut Status, xy: &[u8], path: &str, from: Option<String>) {
    if xy.len() < 2 {
        return;
    }
    if let Some(kind) = Kind::of(xy[0]) {
        status.staged.push(Change {
            path: path.to_string(),
            from: from.clone(),
            kind,
        });
    }
    if let Some(kind) = Kind::of(xy[1]) {
        status.unstaged.push(Change {
            path: path.to_string(),
            from: None,
            kind,
        });
    }
}

// ─── Index and working tree ─────────────────────────────────────────────────

pub async fn stage(repo: &Path, paths: &[String]) -> Result<(), String> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(repo, &args).await.map(|_| ())
}

pub async fn unstage(repo: &Path, paths: &[String]) -> Result<(), String> {
    let mut args = vec!["restore", "--staged", "--"];
    args.extend(paths.iter().map(String::as_str));
    // A repository without a commit has no HEAD to restore from.
    match run(repo, &args).await {
        Ok(_) => Ok(()),
        Err(_) => {
            let mut args = vec!["rm", "--cached", "-r", "--"];
            args.extend(paths.iter().map(String::as_str));
            run(repo, &args).await.map(|_| ())
        }
    }
}

/// Throws the changes of a file away: back to what HEAD has (a new file is
/// removed).
pub async fn rollback(repo: &Path, change: &Change, staged: bool) -> Result<(), String> {
    let path = change.path.as_str();
    match change.kind {
        Kind::Untracked => {
            let target = repo.join(path);
            std::fs::remove_file(&target).map_err(|e| format!("{path}: {e}"))
        }
        Kind::Added if staged => run(repo, &["rm", "-f", "--", path]).await.map(|_| ()),
        _ if staged => run(
            repo,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                path,
            ],
        )
        .await
        .map(|_| ()),
        _ => run(repo, &["restore", "--", path]).await.map(|_| ()),
    }
}

/// Deletes a file and tells git: `git rm`, or a plain delete when untracked.
pub async fn delete(repo: &Path, change: &Change) -> Result<(), String> {
    match change.kind {
        Kind::Untracked => std::fs::remove_file(repo.join(&change.path))
            .map_err(|e| format!("{}: {e}", change.path)),
        _ => run(repo, &["rm", "-f", "--", change.path.as_str()])
            .await
            .map(|_| ()),
    }
}

// ─── Commit, push… ──────────────────────────────────────────────────────────

/// Commits what is staged; `amend` rewrites the last commit with it.
pub async fn commit(repo: &Path, message: &str, amend: bool) -> Result<(), String> {
    let mut args = vec!["commit", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    run_with(repo, &args, Some(message), &[]).await.map(|_| ())
}

/// Pushes the current branch (creating its upstream the first time).
pub async fn push(repo: &Path, branch: &str, has_upstream: bool) -> Result<(), String> {
    if has_upstream {
        run(repo, &["push"]).await.map(|_| ())
    } else {
        run(repo, &["push", "--set-upstream", "origin", branch])
            .await
            .map(|_| ())
    }
}

pub async fn last_message(repo: &Path) -> Result<String, String> {
    run(repo, &["log", "-1", "--format=%B"])
        .await
        .map(|m| m.trim_end().to_string())
}

// ─── Diffs ──────────────────────────────────────────────────────────────────

/// What a diff compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Index against HEAD.
    Staged,
    /// Working tree against the index.
    Unstaged,
    /// A new file, against nothing.
    Untracked,
    /// What a commit changed.
    Commit(String),
    /// What a stash holds.
    Stash(usize),
    /// A ref against the working tree.
    Tree(String),
}

pub async fn diff_of(repo: &Path, source: &Source, path: &str) -> Result<FileDiff, String> {
    let owned;
    let base;
    let args: Vec<&str> = match source {
        Source::Staged => vec![
            "diff",
            "--cached",
            "--no-color",
            "--no-ext-diff",
            "-U3",
            "--",
            path,
        ],
        Source::Unstaged => vec!["diff", "--no-color", "--no-ext-diff", "-U3", "--", path],
        Source::Untracked => vec![
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "-U3",
            "--",
            "/dev/null",
            path,
        ],
        Source::Commit(sha) => {
            owned = sha.clone();
            vec![
                "show",
                "--format=",
                "--patch",
                "--no-color",
                "--no-ext-diff",
                "-U3",
                &owned,
                "--",
                path,
            ]
        }
        Source::Stash(n) => {
            // `git stash show` takes no pathspec: compare the stash with its base.
            owned = format!("stash@{{{n}}}");
            base = format!("{owned}^");
            vec![
                "diff",
                "--no-color",
                "--no-ext-diff",
                "-U3",
                &base,
                &owned,
                "--",
                path,
            ]
        }
        Source::Tree(reference) => {
            owned = reference.clone();
            vec![
                "diff",
                "--no-color",
                "--no-ext-diff",
                "-U3",
                &owned,
                "--",
                path,
            ]
        }
    };
    // `--no-index` exits with 1 when the files differ.
    let out = match run(repo, &args).await {
        Ok(out) => out,
        Err(e) if matches!(source, Source::Untracked) => {
            let out = run_allow_failure(repo, &args).await;
            if out.is_empty() {
                return Err(e);
            }
            out
        }
        Err(e) => return Err(e),
    };
    let mut files = parse_diff(&out);
    let mut file = files.pop().unwrap_or_default();
    if file.path.is_empty() {
        file.path = path.to_string();
    }
    Ok(file)
}

/// The output of a command even when it exits with a failure status.
async fn run_allow_failure(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .output()
        .await;
    output
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A repository in a temporary folder, with a fixed author.
    pub struct Repo {
        pub dir: tempfile::TempDir,
    }

    impl Repo {
        pub fn new() -> Repo {
            let dir = tempfile::tempdir().expect("a folder");
            let repo = Repo { dir };
            repo.git(&["init", "-q", "-b", "main"]);
            repo.git(&["config", "user.name", "Test"]);
            repo.git(&["config", "user.email", "test@example.com"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }

        pub fn path(&self) -> &Path {
            self.dir.path()
        }

        /// Runs git, blocking (setup of a test).
        pub fn git(&self, args: &[&str]) -> String {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(self.path())
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        }

        pub fn write(&self, name: &str, text: &str) {
            let path = self.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("a folder");
            }
            std::fs::write(path, text).expect("a file");
        }

        pub fn commit_all(&self, message: &str) {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", message]);
        }
    }

    /// Runs a future to its end (tests are synchronous).
    pub fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(future)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn status_separates_the_index_from_the_working_tree() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.write("b.txt", "one\n");
        repo.write("c.txt", "one\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\ntwo\n");
        repo.git(&["add", "a.txt"]);
        // Modified again after being staged: in both lists.
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.write("b.txt", "changed\n");
        repo.write("new.txt", "new\n");
        repo.git(&["rm", "-q", "c.txt"]);

        let status = block(status(repo.path())).unwrap();
        assert_eq!(status.branch, "main");
        let staged: Vec<_> = status
            .staged
            .iter()
            .map(|c| (c.path.as_str(), c.kind))
            .collect();
        assert_eq!(
            staged,
            vec![("a.txt", Kind::Modified), ("c.txt", Kind::Deleted)]
        );
        let unstaged: Vec<_> = status
            .unstaged
            .iter()
            .map(|c| (c.path.as_str(), c.kind))
            .collect();
        assert_eq!(
            unstaged,
            vec![
                ("a.txt", Kind::Modified),
                ("b.txt", Kind::Modified),
                ("new.txt", Kind::Untracked)
            ]
        );
    }

    #[test]
    fn a_rename_keeps_where_it_came_from() {
        let repo = Repo::new();
        repo.write(
            "old.txt",
            "some content that is long enough to be a rename\n",
        );
        repo.commit_all("first");
        repo.git(&["mv", "old.txt", "new.txt"]);
        let status = block(status(repo.path())).unwrap();
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].kind, Kind::Renamed);
        assert_eq!(status.staged[0].path, "new.txt");
        assert_eq!(status.staged[0].from.as_deref(), Some("old.txt"));
    }

    #[test]
    fn staging_unstaging_committing_and_rolling_back() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.write("a.txt", "two\n");
        repo.write("n.txt", "new\n");

        block(stage(repo.path(), &["a.txt".into(), "n.txt".into()])).unwrap();
        let s = block(status(repo.path())).unwrap();
        assert_eq!(s.staged.len(), 2);
        block(unstage(repo.path(), &["n.txt".into()])).unwrap();
        let s = block(status(repo.path())).unwrap();
        assert_eq!(s.staged.len(), 1);
        assert_eq!(s.unstaged[0].kind, Kind::Untracked);

        block(commit(repo.path(), "second\n\nbody", false)).unwrap();
        assert_eq!(block(last_message(repo.path())).unwrap(), "second\n\nbody");
        block(commit(repo.path(), "amended", true)).unwrap();
        assert_eq!(block(last_message(repo.path())).unwrap(), "amended");
        let s = block(status(repo.path())).unwrap();
        assert!(s.staged.is_empty());

        // Rolling back a modified file restores it; an untracked one is removed.
        repo.write("a.txt", "scribble\n");
        let change = Change {
            path: "a.txt".into(),
            from: None,
            kind: Kind::Modified,
        };
        block(rollback(repo.path(), &change, false)).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "two\n"
        );
        let change = Change {
            path: "n.txt".into(),
            from: None,
            kind: Kind::Untracked,
        };
        block(rollback(repo.path(), &change, false)).unwrap();
        assert!(!repo.path().join("n.txt").exists());
    }

    #[test]
    fn delete_removes_the_file_and_stages_the_removal() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        let change = Change {
            path: "a.txt".into(),
            from: None,
            kind: Kind::Modified,
        };
        block(delete(repo.path(), &change)).unwrap();
        assert!(!repo.path().join("a.txt").exists());
        let s = block(status(repo.path())).unwrap();
        assert_eq!(s.staged[0].kind, Kind::Deleted);
    }

    #[test]
    fn diffs_cover_staged_unstaged_and_untracked_files() {
        let repo = Repo::new();
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\nTWO\nthree\n");
        repo.git(&["add", "a.txt"]);
        repo.write("a.txt", "one\nTWO\nthree\nfour\n");
        repo.write("n.txt", "brand\nnew\n");

        let staged = block(diff_of(repo.path(), &Source::Staged, "a.txt")).unwrap();
        assert_eq!(staged.added(), 1);
        assert_eq!(staged.removed(), 1);
        let unstaged = block(diff_of(repo.path(), &Source::Unstaged, "a.txt")).unwrap();
        assert_eq!((unstaged.added(), unstaged.removed()), (1, 0));
        let untracked = block(diff_of(repo.path(), &Source::Untracked, "n.txt")).unwrap();
        assert_eq!((untracked.added(), untracked.removed()), (2, 0));
    }

    #[test]
    fn the_repository_root_is_found_from_a_nested_file() {
        let repo = Repo::new();
        repo.write("deep/er/a.txt", "x\n");
        let root = block(repo_root(&repo.path().join("deep/er/a.txt"))).unwrap();
        assert_eq!(
            root.canonicalize().unwrap(),
            repo.path().canonicalize().unwrap()
        );
        let outside = tempfile::tempdir().unwrap();
        assert!(block(repo_root(outside.path())).is_none());
    }
}
