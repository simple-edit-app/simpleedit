//! Git, through the `git` program: what the Git panel shows and does.
//!
//! Every command runs in the repository's folder and answers with text that
//! is parsed here (porcelain formats, never what a human is meant to read).
//! Commands are async so the window never waits for one.

pub mod conflicts;
pub mod diff;
pub mod graph;
pub mod history;
pub mod marks;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub use diff::{parse_diff, FileDiff};

/// Runs `git args…` in `repo`; the output, or what git complained about.
pub async fn run(repo: &Path, args: &[&str]) -> Result<String, String> {
    run_with(repo, args, None, &[]).await
}

/// A git command for `repo`: no password or editor prompt nobody could see.
fn command(repo: &Path, args: &[&str], env: &[(&str, &str)], stdin: bool) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", "true")
        .envs(env.iter().copied())
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// How long a command may take before it is given up.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// The same for what talks to a remote (a push can be big and the line slow).
const NETWORK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);

/// Like [`run`], with text for git's standard input and extra environment.
pub async fn run_with(
    repo: &Path,
    args: &[&str],
    input: Option<&str>,
    env: &[(&str, &str)],
) -> Result<String, String> {
    run_timed(repo, args, input, env, TIMEOUT).await
}

/// `git push …`: it may take far longer than local commands.
pub async fn run_network(repo: &Path, args: &[&str]) -> Result<String, String> {
    run_timed(repo, args, None, &[], NETWORK_TIMEOUT).await
}

async fn run_timed(
    repo: &Path,
    args: &[&str],
    input: Option<&str>,
    env: &[(&str, &str)],
    timeout: std::time::Duration,
) -> Result<String, String> {
    let mut child = command(repo, args, env, input.is_some())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    // Written while the output is read, so a big input can't block git.
    let writer = match (input, child.stdin.take()) {
        (Some(text), Some(mut stdin)) => {
            let text = text.to_string();
            Some(tokio::spawn(async move {
                let _ = stdin.write_all(text.as_bytes()).await;
            }))
        }
        _ => None,
    };
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| t!("git.err_timeout").to_string())?
        .map_err(|e| format!("git: {e}"))?;
    if let Some(writer) = writer {
        let _ = writer.await;
    }
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

/// A long operation git was left in the middle of (conflicts to settle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

/// The state of the working tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// What is in progress, if anything is.
    pub operation: Option<Operation>,
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// In the index (`git add`ed), against HEAD.
    pub staged: Vec<Change>,
    /// In the working tree, against the index (untracked files included).
    pub unstaged: Vec<Change>,
}

/// The state of the working tree; `untracked`: also list the files git does
/// not know (costly in a big folder that isn't ignored).
pub async fn status(repo: &Path, untracked: bool) -> Result<Status, String> {
    let out = run(
        repo,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            if untracked {
                "--untracked-files=all"
            } else {
                "--untracked-files=no"
            },
        ],
    )
    .await?;
    let mut status = parse_status(&out);
    status.operation = operation(repo).await;
    Ok(status)
}

/// What the repository is in the middle of.
pub async fn operation(repo: &Path) -> Option<Operation> {
    let dir = run(repo, &["rev-parse", "--absolute-git-dir"]).await.ok()?;
    let dir = PathBuf::from(dir.trim());
    // A rebase replays commits with cherry-picks: it comes first.
    if dir.join("rebase-merge").exists() || dir.join("rebase-apply").exists() {
        Some(Operation::Rebase)
    } else if dir.join("MERGE_HEAD").exists() {
        Some(Operation::Merge)
    } else if dir.join("CHERRY_PICK_HEAD").exists() {
        Some(Operation::CherryPick)
    } else if dir.join("REVERT_HEAD").exists() {
        Some(Operation::Revert)
    } else {
        None
    }
}

impl Status {
    /// The files in conflict.
    pub fn conflicts(&self) -> impl Iterator<Item = &Change> {
        self.unstaged.iter().filter(|c| c.kind == Kind::Conflict)
    }
}

/// What an operation says when it stops on conflicts (they are left in place
/// to settle).
pub const CONFLICT: &str = "conflict";

/// Whether any file is in conflict.
pub async fn has_conflicts(repo: &Path) -> bool {
    run(repo, &["diff", "--name-only", "--diff-filter=U"])
        .await
        .is_ok_and(|out| !out.trim().is_empty())
}

/// Ends a failed operation: with conflicts it stays in progress (the answer
/// is [`CONFLICT`]), else `abort` puts everything back.
pub async fn stopped(repo: &Path, error: String, abort: &[&str]) -> String {
    if has_conflicts(repo).await {
        CONFLICT.to_string()
    } else {
        let _ = run(repo, abort).await;
        error
    }
}

/// Keeps one side of a conflicted file and marks it settled. `rebasing`:
/// git's "ours" is then the branch being rebased onto, not the user's work.
pub async fn accept(repo: &Path, path: &str, yours: bool, rebasing: bool) -> Result<(), String> {
    let side = if yours != rebasing {
        "--ours"
    } else {
        "--theirs"
    };
    match run(repo, &["checkout", side, "--", path]).await {
        Ok(_) => run(repo, &["add", "--", path]).await.map(|_| ()),
        // That side deleted the file: keeping it means deleting.
        Err(_) => run(repo, &["rm", "-f", "--", path]).await.map(|_| ()),
    }
}

/// The conflicted text of a file rebuilt from the three versions git keeps
/// (base, ours, theirs), with the base shown in each conflict.
pub async fn merge_text(repo: &Path, path: &str) -> Option<String> {
    let base = run(repo, &["show", &format!(":1:{path}")])
        .await
        .unwrap_or_default();
    let ours = run(repo, &["show", &format!(":2:{path}")]).await.ok()?;
    let theirs = run(repo, &["show", &format!(":3:{path}")]).await.ok()?;
    let dir = tempfile::tempdir().ok()?;
    let write = |name: &'static str, text: String| {
        let file = dir.path().join(name);
        async move {
            tokio::fs::write(&file, text).await.ok()?;
            Some(file.to_string_lossy().into_owned())
        }
    };
    let (ours, base, theirs) = (
        write("ours", ours).await?,
        write("base", base).await?,
        write("theirs", theirs).await?,
    );
    // The exit status counts the conflicts: only the text matters.
    let out = run_allow_failure(
        repo,
        &[
            "merge-file",
            "-p",
            "--diff3",
            "-L",
            "yours",
            "-L",
            "base",
            "-L",
            "theirs",
            &ours,
            &base,
            &theirs,
        ],
    )
    .await;
    (!out.is_empty()).then_some(out)
}

/// Writes the settled text of a conflicted file and marks it settled.
pub async fn resolve_with(repo: &Path, path: &str, text: &str) -> Result<(), String> {
    tokio::fs::write(repo.join(path), text)
        .await
        .map_err(|e| format!("{path}: {e}"))?;
    run(repo, &["add", "--", path]).await.map(|_| ())
}

/// Goes on with an operation whose conflicts are settled.
pub async fn continue_operation(repo: &Path, operation: Operation) -> Result<(), String> {
    let result = match operation {
        Operation::Merge => run(repo, &["commit", "--no-edit"]).await,
        Operation::Rebase => run(repo, &["rebase", "--continue"]).await,
        Operation::CherryPick => run(repo, &["cherry-pick", "--continue"]).await,
        Operation::Revert => run(repo, &["revert", "--continue"]).await,
    };
    match result {
        Ok(_) => Ok(()),
        // The next commit of a rebase may conflict in turn.
        Err(_) if has_conflicts(repo).await => Err(CONFLICT.to_string()),
        Err(e) => Err(e),
    }
}

/// Leaves out the commit an operation is stopped on (it became empty once its
/// conflicts were settled, say). A merge has no such step.
pub async fn skip_operation(repo: &Path, operation: Operation) -> Result<(), String> {
    let args: &[&str] = match operation {
        Operation::Rebase => &["rebase", "--skip"],
        Operation::CherryPick => &["cherry-pick", "--skip"],
        Operation::Revert => &["revert", "--skip"],
        Operation::Merge => return abort_operation(repo, operation).await,
    };
    match run(repo, args).await {
        Ok(_) => Ok(()),
        // The next commit of a rebase may conflict in turn.
        Err(_) if has_conflicts(repo).await => Err(CONFLICT.to_string()),
        Err(e) => Err(e),
    }
}

/// Gives an operation up: everything is as it was before.
pub async fn abort_operation(repo: &Path, operation: Operation) -> Result<(), String> {
    let args: &[&str] = match operation {
        Operation::Merge => &["merge", "--abort"],
        Operation::Rebase => &["rebase", "--abort"],
        Operation::CherryPick => &["cherry-pick", "--abort"],
        Operation::Revert => &["revert", "--abort"],
    };
    run(repo, args).await.map(|_| ())
}

/// How `pull` brings in what the remote has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PullMode {
    /// `git pull`: a merge commit when the branches diverged.
    #[default]
    Merge,
    /// `git pull --rebase`: your commits replayed on top.
    Rebase,
    /// `git pull --ff-only`: only when nothing diverged.
    FastForwardOnly,
}

impl PullMode {
    pub const ALL: [PullMode; 3] = [PullMode::Merge, PullMode::Rebase, PullMode::FastForwardOnly];

    fn flag(self) -> &'static str {
        match self {
            PullMode::Merge => "--no-rebase",
            PullMode::Rebase => "--rebase",
            PullMode::FastForwardOnly => "--ff-only",
        }
    }
}

impl std::fmt::Display for PullMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            PullMode::Merge => t!("git.pull_merge"),
            PullMode::Rebase => t!("git.pull_rebase"),
            PullMode::FastForwardOnly => t!("git.pull_ff_only"),
        })
    }
}

/// How a merge is made when nothing says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeMode {
    /// Git's own choice: fast-forward when it can, else a merge commit.
    #[default]
    Default,
    /// `--no-ff`: always a merge commit.
    NoFastForward,
    /// `--ff-only`: refuses when a merge commit would be needed.
    FastForwardOnly,
    /// `--squash`: the changes are staged as one, to commit yourself.
    Squash,
}

impl MergeMode {
    pub const ALL: [MergeMode; 4] = [
        MergeMode::Default,
        MergeMode::NoFastForward,
        MergeMode::FastForwardOnly,
        MergeMode::Squash,
    ];

    fn flag(self) -> Option<&'static str> {
        match self {
            MergeMode::Default => None,
            MergeMode::NoFastForward => Some("--no-ff"),
            MergeMode::FastForwardOnly => Some("--ff-only"),
            MergeMode::Squash => Some("--squash"),
        }
    }
}

impl std::fmt::Display for MergeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            MergeMode::Default => t!("git.merge_default"),
            MergeMode::NoFastForward => t!("git.merge_no_ff"),
            MergeMode::FastForwardOnly => t!("git.merge_ff_only"),
            MergeMode::Squash => t!("git.merge_squash"),
        })
    }
}

/// Merges a branch into the current one.
pub async fn merge_branch(repo: &Path, branch: &str, mode: MergeMode) -> Result<(), String> {
    let mut args = vec!["merge", "--no-edit"];
    args.extend(mode.flag());
    args.push(branch);
    match run(repo, &args).await {
        Ok(_) => Ok(()),
        Err(e) => Err(stopped(repo, e, &["merge", "--abort"]).await),
    }
}

/// Brings in what a remote has: the upstream of the current branch, or
/// `from` (a remote and one of its branches). What is not committed is put
/// aside meanwhile.
pub async fn pull(repo: &Path, mode: PullMode, from: Option<(&str, &str)>) -> Result<(), String> {
    let mut args = vec!["pull", "--autostash", mode.flag()];
    if mode == PullMode::Merge {
        args.push("--no-edit");
    }
    if let Some((remote, branch)) = from {
        args.extend([remote, branch]);
    }
    match run_network(repo, &args).await {
        Ok(_) => Ok(()),
        Err(e) => {
            let abort: &[&str] = if mode == PullMode::Rebase {
                &["rebase", "--abort"]
            } else {
                &["merge", "--abort"]
            };
            Err(stopped(repo, e, abort).await)
        }
    }
}

/// Replays the current branch on top of another.
pub async fn rebase_onto(repo: &Path, branch: &str) -> Result<(), String> {
    match run(repo, &["rebase", branch]).await {
        Ok(_) => Ok(()),
        Err(e) => Err(stopped(repo, e, &["rebase", "--abort"]).await),
    }
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

/// What HEAD has of a file (`None`: it is not there, or there is no commit).
pub async fn head_text(repo: &Path, relative: &str) -> Option<String> {
    run(repo, &["show", &format!("HEAD:{relative}")]).await.ok()
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
            tokio::fs::remove_file(&target)
                .await
                .map_err(|e| format!("{path}: {e}"))
        }
        Kind::Added if staged => run(repo, &["rm", "-f", "--", path]).await.map(|_| ()),
        // The new name isn't in HEAD: it goes, and the old one comes back.
        Kind::Renamed if staged => {
            run(repo, &["rm", "-f", "--", path]).await?;
            let from = change.from.as_deref().unwrap_or(path);
            run(
                repo,
                &[
                    "restore",
                    "--source=HEAD",
                    "--staged",
                    "--worktree",
                    "--",
                    from,
                ],
            )
            .await
            .map(|_| ())
        }
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

/// Puts one block of changes of the working tree back as it was.
pub async fn rollback_block(
    repo: &Path,
    path: &str,
    diff: &FileDiff,
    index: usize,
) -> Result<(), String> {
    let blocks = diff.blocks();
    let block = blocks
        .get(index)
        .ok_or_else(|| t!("git.err_no_such_change").to_string())?;
    let target = repo.join(path);
    let text = tokio::fs::read_to_string(&target)
        .await
        .map_err(|e| format!("{path}: {e}"))?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    let end = block.at + block.added.len();
    // What the diff saw must still be there.
    let current: Vec<&str> = lines
        .get(block.at..end)
        .map(|l| l.iter().map(|s| s.trim_end_matches(['\n', '\r'])).collect())
        .ok_or_else(|| t!("git.err_file_changed").to_string())?;
    if current != block.added.iter().map(String::as_str).collect::<Vec<_>>() {
        return Err(t!("git.err_file_changed").to_string());
    }
    let old: Vec<String> = block.removed.iter().map(|l| format!("{l}{eol}")).collect();
    lines.splice(block.at..end, old);
    tokio::fs::write(&target, lines.concat())
        .await
        .map_err(|e| format!("{path}: {e}"))
}

/// Deletes a file and tells git: `git rm`, or a plain delete when untracked.
pub async fn delete(repo: &Path, change: &Change) -> Result<(), String> {
    match change.kind {
        Kind::Untracked => tokio::fs::remove_file(repo.join(&change.path))
            .await
            .map_err(|e| format!("{}: {e}", change.path)),
        _ => run(repo, &["rm", "-f", "--", change.path.as_str()])
            .await
            .map(|_| ()),
    }
}

// ─── Commit, push… ──────────────────────────────────────────────────────────

/// Commits what is staged; `amend` rewrites the last commit with it.
/// Commits one file only, whatever else is in the index (a new file is
/// added first).
pub async fn commit_only(
    repo: &Path,
    message: &str,
    amend: bool,
    path: &str,
) -> Result<(), String> {
    run(repo, &["add", "--", path]).await?;
    let mut args = vec!["commit", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    args.extend(["--", path]);
    run_with(repo, &args, Some(message), &[]).await.map(|_| ())
}

pub async fn commit(repo: &Path, message: &str, amend: bool) -> Result<(), String> {
    let mut args = vec!["commit", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    run_with(repo, &args, Some(message), &[]).await.map(|_| ())
}

/// Pushes the current branch (creating its upstream the first time).
pub async fn push(repo: &Path, branch: &str, has_upstream: bool) -> Result<(), String> {
    let mut args = vec!["push"];
    if !has_upstream {
        args.extend(["--set-upstream", "origin", branch]);
    }
    run_network(repo, &args).await.map(|_| ())
}

/// Whether a revision typed by the user names something (and cannot be read
/// by git as an option). The name it resolves to.
pub async fn verify_rev(repo: &Path, rev: &str) -> Result<String, String> {
    if rev.starts_with('-') {
        return Err(t!("git.err_bad_revision", rev = rev).to_string());
    }
    let spec = format!("{rev}^{{object}}");
    run(repo, &["rev-parse", "--verify", "--quiet", &spec])
        .await
        .map(|out| out.trim().to_string())
        .map_err(|_| t!("git.err_bad_revision", rev = rev).to_string())
}

pub async fn last_message(repo: &Path) -> Result<String, String> {
    run(repo, &["log", "-1", "--format=%B"])
        .await
        .map(|m| m.trim_end().to_string())
}

/// Applies a patch to the index only (`--cached`), or takes it out of it
/// (`reverse`): the way to stage or unstage a block or some lines.
pub async fn apply_patch(repo: &Path, patch: &str, reverse: bool) -> Result<(), String> {
    let mut args = vec!["apply", "--cached", "--recount", "--whitespace=nowarn"];
    if reverse {
        args.push("-R");
    }
    run_with(repo, &args, Some(patch), &[]).await.map(|_| ())
}

/// Gets what the remotes have (without changing the working tree), and drops
/// the branches they no longer have.
pub async fn fetch(repo: &Path) -> Result<(), String> {
    run_network(repo, &["fetch", "--all", "--prune"])
        .await
        .map(|_| ())
}

/// How far `reset` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// Only the branch moves: the changes stay staged.
    Soft,
    /// The branch and the index: the changes stay in the files.
    Mixed,
    /// Everything: the changes are lost.
    Hard,
}

/// Moves the current branch to a commit.
pub async fn reset(repo: &Path, to: &str, mode: ResetMode) -> Result<(), String> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };
    run(repo, &["reset", flag, "--end-of-options", to])
        .await
        .map(|_| ())
}

/// Takes the last commit back, its changes staying staged; its message.
pub async fn undo_last_commit(repo: &Path) -> Result<String, String> {
    let message = last_message(repo).await?;
    run(repo, &["reset", "--soft", "HEAD~1"]).await?;
    Ok(message)
}

/// Puts one file's changes aside (a stash of that file alone).
pub async fn stash_file(repo: &Path, path: &str, message: &str) -> Result<(), String> {
    let mut args = vec!["stash", "push", "--include-untracked"];
    if !message.trim().is_empty() {
        args.extend(["-m", message]);
    }
    args.extend(["--", path]);
    run(repo, &args).await.map(|_| ())
}

/// Adds a file to the `.gitignore` of the repository (made if it is not
/// there). A file git already tracks is taken out of the index too (its
/// copy on disk stays), else the line would change nothing.
pub async fn ignore_file(repo: &Path, path: &str) -> Result<bool, String> {
    let file = repo.join(".gitignore");
    let mut text = tokio::fs::read_to_string(&file).await.unwrap_or_default();
    let pattern = format!("/{path}");
    if !text
        .lines()
        .any(|l| l.trim() == pattern || l.trim() == path)
    {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&pattern);
        text.push('\n');
        tokio::fs::write(&file, text)
            .await
            .map_err(|e| format!(".gitignore: {e}"))?;
    }
    let tracked = run(repo, &["ls-files", "--error-unmatch", "--", path])
        .await
        .is_ok();
    if tracked {
        run(repo, &["rm", "--cached", "-q", "--", path]).await?;
    }
    Ok(tracked)
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
    /// One commit against another (the second against the first).
    Range(String, String),
}

/// How a diff is read: context lines around the changes (`u32::MAX`: the
/// whole file) and whether changes of blanks count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOptions {
    pub context: u32,
    pub ignore_blanks: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions {
            context: 3,
            ignore_blanks: false,
        }
    }
}

#[cfg(test)]
pub async fn diff_of(repo: &Path, source: &Source, path: &str) -> Result<FileDiff, String> {
    diff_with(repo, source, path, DiffOptions::default()).await
}

pub async fn diff_with(
    repo: &Path,
    source: &Source,
    path: &str,
    options: DiffOptions,
) -> Result<FileDiff, String> {
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
        Source::Range(from, to) => {
            owned = from.clone();
            base = to.clone();
            vec![
                "diff",
                "--no-color",
                "--no-ext-diff",
                "-U3",
                &owned,
                &base,
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
    // Paths as they are, not quoted (`+++ b/…` names the file); the options
    // of the reading in place of the default context.
    let context = format!("-U{}", options.context.min(1_000_000));
    let mut full = vec!["-c", "core.quotepath=false"];
    for arg in args {
        if arg == "-U3" {
            full.push(&context);
            if options.ignore_blanks {
                full.push("-w");
            }
        } else {
            full.push(arg);
        }
    }
    let args = full;
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
    let Ok(child) = command(repo, args, &[], false).spawn() else {
        return String::new();
    };
    match tokio::time::timeout(TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => String::from_utf8_lossy(&output.stdout).into_owned(),
        _ => String::new(),
    }
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
    use crate::git::diff::Tag;

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

        let status = block(status(repo.path(), true)).unwrap();
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
        let status = block(status(repo.path(), true)).unwrap();
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
        let s = block(status(repo.path(), true)).unwrap();
        assert_eq!(s.staged.len(), 2);
        block(unstage(repo.path(), &["n.txt".into()])).unwrap();
        let s = block(status(repo.path(), true)).unwrap();
        assert_eq!(s.staged.len(), 1);
        assert_eq!(s.unstaged[0].kind, Kind::Untracked);

        block(commit(repo.path(), "second\n\nbody", false)).unwrap();
        assert_eq!(block(last_message(repo.path())).unwrap(), "second\n\nbody");
        block(commit(repo.path(), "amended", true)).unwrap();
        assert_eq!(block(last_message(repo.path())).unwrap(), "amended");
        let s = block(status(repo.path(), true)).unwrap();
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
    fn one_block_of_changes_can_be_rolled_back_alone() {
        let repo = Repo::new();
        repo.write(
            "a.txt",
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
        );
        repo.commit_all("first");
        // Three blocks: a change, a removal, an addition.
        repo.write(
            "a.txt",
            "one\nTWO\nthree\nfour\nfive\nsix\nseven\nnine\nten\nextra\n",
        );
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "a.txt")).unwrap();
        assert_eq!(diff.blocks().len(), 3);
        // The last block first, then the removal, then the first one.
        block(rollback_block(repo.path(), "a.txt", &diff, 2)).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "one\nTWO\nthree\nfour\nfive\nsix\nseven\nnine\nten\n"
        );
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "a.txt")).unwrap();
        block(rollback_block(repo.path(), "a.txt", &diff, 1)).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "one\nTWO\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n"
        );
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "a.txt")).unwrap();
        block(rollback_block(repo.path(), "a.txt", &diff, 0)).unwrap();
        assert!(block(status(repo.path(), true))
            .unwrap()
            .unstaged
            .is_empty());
    }

    #[test]
    fn a_stale_diff_is_refused_instead_of_corrupting_the_file() {
        let repo = Repo::new();
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\nTWO\nthree\n");
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "a.txt")).unwrap();
        repo.write("a.txt", "one\nsomething else\nthree\n");
        assert!(block(rollback_block(repo.path(), "a.txt", &diff, 0)).is_err());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "one\nsomething else\nthree\n"
        );
    }

    /// `a.txt` changed on both `main` and `side`, from the same base.
    fn conflicting() -> Repo {
        let repo = Repo::new();
        repo.write("a.txt", "1\n2\n3\n");
        repo.commit_all("base");
        repo.git(&["checkout", "-q", "-b", "side"]);
        repo.write("a.txt", "1\nside\n3\n");
        repo.commit_all("side change");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("a.txt", "1\nmain\n3\n");
        repo.commit_all("main change");
        repo
    }

    fn read(repo: &Repo) -> String {
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap()
    }

    #[test]
    fn a_merge_that_conflicts_is_settled_by_keeping_a_side_then_continued() {
        let repo = conflicting();
        assert_eq!(
            block(merge_branch(repo.path(), "side", MergeMode::Default)).unwrap_err(),
            CONFLICT
        );
        let now = block(status(repo.path(), true)).unwrap();
        assert_eq!(now.operation, Some(Operation::Merge));
        assert_eq!(
            now.conflicts().map(|c| c.path.as_str()).collect::<Vec<_>>(),
            vec!["a.txt"]
        );
        // In a merge, "yours" is the branch we are on.
        block(accept(repo.path(), "a.txt", true, false)).unwrap();
        assert_eq!(read(&repo), "1\nmain\n3\n");
        block(continue_operation(repo.path(), Operation::Merge)).unwrap();
        assert_eq!(block(operation(repo.path())), None);
        assert_eq!(
            repo.git(&["log", "-1", "--format=%P"])
                .split_whitespace()
                .count(),
            2
        );
    }

    #[test]
    fn during_a_rebase_yours_is_the_commit_being_replayed() {
        let repo = conflicting();
        repo.git(&["checkout", "-q", "side"]);
        assert_eq!(
            block(rebase_onto(repo.path(), "main")).unwrap_err(),
            CONFLICT
        );
        let now = block(status(repo.path(), true)).unwrap();
        assert_eq!(now.operation, Some(Operation::Rebase));
        // "Yours" = my commit ("side"), although git calls it theirs here.
        block(accept(repo.path(), "a.txt", true, true)).unwrap();
        assert_eq!(read(&repo), "1\nside\n3\n");
        block(continue_operation(repo.path(), Operation::Rebase)).unwrap();
        assert_eq!(block(operation(repo.path())), None);
        assert_eq!(
            repo.git(&["log", "--format=%s"]).lines().next(),
            Some("side change")
        );
    }

    #[test]
    fn a_cherry_pick_that_conflicts_can_take_their_side_or_be_aborted() {
        let repo = conflicting();
        let side = repo.git(&["rev-parse", "side"]).trim().to_string();
        assert_eq!(
            block(history::cherry_pick(
                repo.path(),
                std::slice::from_ref(&side)
            ))
            .unwrap_err(),
            CONFLICT
        );
        assert_eq!(block(operation(repo.path())), Some(Operation::CherryPick));
        block(abort_operation(repo.path(), Operation::CherryPick)).unwrap();
        assert_eq!(read(&repo), "1\nmain\n3\n");
        assert_eq!(block(operation(repo.path())), None);
        // Again, and this time theirs is taken.
        let _ = block(history::cherry_pick(repo.path(), &[side]));
        block(accept(repo.path(), "a.txt", false, false)).unwrap();
        assert_eq!(read(&repo), "1\nside\n3\n");
        block(continue_operation(repo.path(), Operation::CherryPick)).unwrap();
        assert_eq!(block(operation(repo.path())), None);
    }

    #[test]
    fn the_three_versions_rebuild_a_conflict_that_shows_its_base() {
        let repo = conflicting();
        let _ = block(merge_branch(repo.path(), "side", MergeMode::Default));
        let text = block(merge_text(repo.path(), "a.txt")).expect("a merge");
        let merge = conflicts::parse(&text, false).unwrap();
        let chunk = merge.conflicts().next().unwrap();
        assert_eq!(chunk.yours, vec!["main"]);
        assert_eq!(chunk.theirs, vec!["side"]);
        assert_eq!(chunk.base.as_deref(), Some(&["2".to_string()][..]));
    }

    #[test]
    fn a_hand_made_resolution_is_written_and_staged() {
        let repo = conflicting();
        let _ = block(merge_branch(repo.path(), "side", MergeMode::Default));
        let text = std::fs::read_to_string(repo.path().join("a.txt")).unwrap();
        let mut merge = conflicts::parse(&text, false).unwrap();
        for chunk in merge.conflicts_mut() {
            chunk.set(conflicts::Side::Yours, conflicts::Pick::Taken);
            chunk.set(conflicts::Side::Theirs, conflicts::Pick::Taken);
        }
        block(resolve_with(repo.path(), "a.txt", &merge.render())).unwrap();
        assert_eq!(read(&repo), "1\nmain\nside\n3\n");
        assert_eq!(
            block(status(repo.path(), true))
                .unwrap()
                .conflicts()
                .count(),
            0
        );
        block(continue_operation(repo.path(), Operation::Merge)).unwrap();
    }

    #[test]
    fn a_merge_that_fails_without_conflicts_is_aborted_cleanly() {
        let repo = conflicting();
        assert!(block(merge_branch(
            repo.path(),
            "no-such-branch",
            MergeMode::Default
        ))
        .is_err());
        assert_eq!(block(operation(repo.path())), None);
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
        let s = block(status(repo.path(), true)).unwrap();
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

    #[test]
    fn committing_one_file_leaves_the_rest_of_the_index_alone() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.write("b.txt", "one\n");
        repo.commit_all("first");
        repo.write("a.txt", "two\n");
        repo.write("b.txt", "two\n");
        repo.write("new.txt", "fresh\n");
        repo.git(&["add", "b.txt"]);
        block(commit_only(repo.path(), "only a", false, "a.txt")).unwrap();
        let last = repo.git(&["show", "--name-only", "--format=%s", "HEAD"]);
        assert!(last.contains("only a") && last.contains("a.txt"));
        assert!(!last.contains("b.txt"));
        // b.txt is still staged, untouched; new.txt untracked.
        let status = block(status(repo.path(), true)).unwrap();
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].path, "b.txt");
        // A new file is added then committed alone.
        block(commit_only(repo.path(), "only new", false, "new.txt")).unwrap();
        let last = repo.git(&["show", "--name-only", "--format=%s", "HEAD"]);
        assert!(last.contains("new.txt") && !last.contains("b.txt"));
    }

    #[test]
    fn a_file_is_compared_with_a_branch_or_a_revision() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["tag", "v1"]);
        repo.write("a.txt", "one\ntwo\n");
        repo.commit_all("second");
        repo.write("a.txt", "one\ntwo\nthree\n");
        let diff = block(diff_of(repo.path(), &Source::Tree("v1".into()), "a.txt")).unwrap();
        let added: usize = diff
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.tag == crate::git::diff::Tag::Added)
            .count();
        assert_eq!(added, 2);
        let sha = repo.git(&["rev-parse", "HEAD~1"]);
        let by_sha = block(diff_of(
            repo.path(),
            &Source::Tree(sha.trim().to_string()),
            "a.txt",
        ))
        .unwrap();
        assert_eq!(by_sha.hunks.len(), 1);
    }

    #[test]
    fn a_rewritten_branch_is_refused_by_a_plain_push() {
        let remote = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .current_dir(remote.path())
            .status()
            .unwrap();
        let repo = Repo::new();
        repo.git(&["remote", "add", "origin", remote.path().to_str().unwrap()]);
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        // The first push sets the upstream.
        block(push(repo.path(), "main", false)).unwrap();
        repo.git(&["commit", "-q", "--amend", "-m", "first, reworded"]);
        assert!(block(push(repo.path(), "main", true)).is_err());
    }

    #[test]
    fn a_block_of_a_staged_diff_can_be_put_back_and_the_file_is_to_stage_again() {
        let repo = Repo::new();
        repo.write("a.txt", "one\ntwo\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\nTWO\n");
        repo.git(&["add", "a.txt"]);
        let diff = block(diff_of(repo.path(), &Source::Staged, "a.txt")).unwrap();
        block(rollback_block(repo.path(), "a.txt", &diff, 0)).unwrap();
        let text = std::fs::read_to_string(repo.path().join("a.txt")).unwrap();
        assert_eq!(text, "one\ntwo\n");
        // The index still holds the change, the file is back: both columns.
        let status = block(status(repo.path(), true)).unwrap();
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.unstaged.len(), 1);
    }

    #[test]
    fn rolling_back_a_staged_rename_brings_the_old_name_back() {
        let repo = Repo::new();
        repo.write("old.txt", "hello\n");
        repo.commit_all("first");
        repo.git(&["mv", "old.txt", "new.txt"]);
        let status = block(status(repo.path(), true)).unwrap();
        let change = status.staged[0].clone();
        assert_eq!(change.kind, Kind::Renamed);
        block(rollback(repo.path(), &change, true)).unwrap();
        assert!(repo.path().join("old.txt").exists());
        assert!(!repo.path().join("new.txt").exists());
        assert!(repo.git(&["status", "--porcelain"]).trim().is_empty());
    }

    #[test]
    fn a_skipped_commit_of_a_rebase_goes_on_to_the_end() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["checkout", "-q", "-b", "side"]);
        repo.write("a.txt", "side\n");
        repo.commit_all("side");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("a.txt", "main\n");
        repo.commit_all("main");
        repo.git(&["checkout", "-q", "side"]);
        assert_eq!(
            block(rebase_onto(repo.path(), "main")).unwrap_err(),
            CONFLICT
        );
        block(skip_operation(repo.path(), Operation::Rebase)).unwrap();
        assert!(block(operation(repo.path())).is_none());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "main\n"
        );
    }

    #[test]
    fn a_revision_is_checked_before_it_reaches_git_as_an_option() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["tag", "v1"]);
        assert!(block(verify_rev(repo.path(), "v1")).is_ok());
        assert!(block(verify_rev(repo.path(), "HEAD")).is_ok());
        assert!(block(verify_rev(repo.path(), "nope")).is_err());
        assert!(block(verify_rev(repo.path(), "--output=/tmp/x")).is_err());
    }

    #[test]
    fn a_file_named_with_a_b_folder_has_its_own_diff() {
        let repo = Repo::new();
        repo.write("dir b/x.txt", "one\n");
        repo.commit_all("first");
        repo.write("dir b/x.txt", "two\n");
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "dir b/x.txt")).unwrap();
        assert_eq!(diff.path, "dir b/x.txt");
        assert_eq!(diff.blocks().len(), 1);
    }

    #[test]
    fn a_removed_line_of_dashes_is_a_block_to_roll_back() {
        let repo = Repo::new();
        repo.write("q.sql", "SELECT 1;\n-- a comment\nSELECT 2;\n");
        repo.commit_all("first");
        repo.write("q.sql", "SELECT 1;\nSELECT 2;\n");
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "q.sql")).unwrap();
        assert_eq!(diff.blocks().len(), 1);
        block(rollback_block(repo.path(), "q.sql", &diff, 0)).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("q.sql")).unwrap(),
            "SELECT 1;\n-- a comment\nSELECT 2;\n"
        );
    }

    /// A repository cloned from a bare remote, and a second clone to push from.
    fn two_clones() -> (Repo, Repo, tempfile::TempDir) {
        let remote = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .current_dir(remote.path())
            .status()
            .unwrap();
        let url = remote.path().to_str().unwrap().to_string();
        let a = Repo::new();
        a.git(&["remote", "add", "origin", &url]);
        a.write("f.txt", "one\n");
        a.commit_all("first");
        a.git(&["push", "-q", "-u", "origin", "main"]);
        let b = Repo::new();
        b.git(&["remote", "add", "origin", &url]);
        b.git(&["pull", "-q", "origin", "main"]);
        b.git(&["branch", "--set-upstream-to=origin/main", "main"]);
        (a, b, remote)
    }

    #[test]
    fn a_pull_brings_in_what_the_remote_has_by_merge_rebase_or_fast_forward() {
        for mode in PullMode::ALL {
            let (a, b, _remote) = two_clones();
            b.write("b.txt", "from b\n");
            b.commit_all("on b");
            b.git(&["push", "-q"]);
            // A local commit too, so the branches diverged.
            a.write("a.txt", "from a\n");
            a.commit_all("on a");
            let result = block(pull(a.path(), mode, None));
            match mode {
                PullMode::FastForwardOnly => {
                    assert!(result.is_err(), "diverged: no fast-forward");
                    assert!(!a.path().join("b.txt").exists());
                }
                PullMode::Merge => {
                    result.unwrap();
                    assert!(a.path().join("b.txt").exists());
                    assert!(a.git(&["log", "--merges", "--format=%s"]).contains("Merge"));
                }
                PullMode::Rebase => {
                    result.unwrap();
                    assert!(a.path().join("b.txt").exists());
                    assert!(a.git(&["log", "--merges", "--format=%s"]).trim().is_empty());
                }
            }
        }
    }

    #[test]
    fn a_pull_goes_on_with_changes_in_the_working_tree_and_stops_on_conflicts() {
        let (a, b, _remote) = two_clones();
        b.write("f.txt", "from b\n");
        b.commit_all("b changes f");
        b.git(&["push", "-q"]);
        // Uncommitted work in another file: put aside and brought back.
        a.write("other.txt", "wip\n");
        a.git(&["add", "other.txt"]);
        block(pull(a.path(), PullMode::Rebase, None)).unwrap();
        assert_eq!(
            std::fs::read_to_string(a.path().join("f.txt")).unwrap(),
            "from b\n"
        );
        assert!(a.path().join("other.txt").exists());
        // The same line changed on both sides: left to settle.
        b.write("f.txt", "again from b\n");
        b.commit_all("b again");
        b.git(&["push", "-q"]);
        a.git(&["commit", "-q", "-m", "wip"]);
        a.write("f.txt", "from a\n");
        a.commit_all("a changes f");
        assert_eq!(
            block(pull(a.path(), PullMode::Merge, None)).unwrap_err(),
            CONFLICT
        );
        assert!(block(has_conflicts(a.path())));
    }

    #[test]
    fn a_pull_can_name_the_remote_branch() {
        let (a, b, _remote) = two_clones();
        b.git(&["checkout", "-q", "-b", "topic"]);
        b.write("t.txt", "topic\n");
        b.commit_all("topic");
        b.git(&["push", "-q", "origin", "topic"]);
        block(pull(a.path(), PullMode::Merge, Some(("origin", "topic")))).unwrap();
        assert!(a.path().join("t.txt").exists());
    }

    #[test]
    fn a_merge_follows_the_chosen_mode() {
        let make = || {
            let repo = Repo::new();
            repo.write("a.txt", "one\n");
            repo.commit_all("first");
            repo.git(&["checkout", "-q", "-b", "side"]);
            repo.write("b.txt", "side\n");
            repo.commit_all("on side");
            repo.git(&["checkout", "-q", "main"]);
            repo
        };
        // Default: nothing diverged, so a fast-forward.
        let repo = make();
        block(merge_branch(repo.path(), "side", MergeMode::Default)).unwrap();
        assert!(repo
            .git(&["log", "--merges", "--format=%s"])
            .trim()
            .is_empty());
        // --no-ff: a merge commit all the same.
        let repo = make();
        block(merge_branch(repo.path(), "side", MergeMode::NoFastForward)).unwrap();
        assert!(repo
            .git(&["log", "--merges", "--format=%s"])
            .contains("Merge"));
        // --squash: staged, not committed.
        let repo = make();
        block(merge_branch(repo.path(), "side", MergeMode::Squash)).unwrap();
        assert!(repo.git(&["status", "--porcelain"]).contains("A  b.txt"));
        assert_eq!(repo.git(&["log", "--format=%s"]).trim(), "first");
        // --ff-only refuses when the branches diverged.
        let repo = make();
        repo.write("c.txt", "main\n");
        repo.commit_all("on main");
        assert!(block(merge_branch(
            repo.path(),
            "side",
            MergeMode::FastForwardOnly
        ))
        .is_err());
    }

    const SEVEN: &str = "a\nb\nc\nd\ne\nf\ng\n";
    const SEVEN_CHANGED: &str = "a\nB\nc\nd\ne\nF\ng\n";

    fn staged(repo: &Repo) -> String {
        repo.git(&["diff", "--cached", "-U0"])
    }

    #[test]
    fn one_block_is_staged_then_taken_out_of_the_index_again() {
        let repo = Repo::new();
        repo.write("f.txt", SEVEN);
        repo.commit_all("first");
        repo.write("f.txt", SEVEN_CHANGED);
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "f.txt")).unwrap();
        assert_eq!(diff.blocks().len(), 2);
        // The second block goes to the index, the first stays in the file.
        block(apply_patch(
            repo.path(),
            &diff.block_patch(1, false).unwrap(),
            false,
        ))
        .unwrap();
        let cached = staged(&repo);
        assert!(cached.contains("+F") && !cached.contains("+B"), "{cached}");
        assert_eq!(
            std::fs::read_to_string(repo.path().join("f.txt")).unwrap(),
            SEVEN_CHANGED
        );
        // The staged diff has that block; out it goes (the file is not touched).
        let cached_diff = block(diff_of(repo.path(), &Source::Staged, "f.txt")).unwrap();
        block(apply_patch(
            repo.path(),
            &cached_diff.block_patch(0, true).unwrap(),
            true,
        ))
        .unwrap();
        assert!(staged(&repo).trim().is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("f.txt")).unwrap(),
            SEVEN_CHANGED
        );
    }

    #[test]
    fn some_lines_are_staged_and_unstaged() {
        let repo = Repo::new();
        repo.write("f.txt", "one\ntwo\nthree\n");
        repo.commit_all("first");
        repo.write("f.txt", "ONE\ntwo\nTHREE\nfour\n");
        let diff = block(diff_of(repo.path(), &Source::Unstaged, "f.txt")).unwrap();
        // Hunk 0: lines are -one +ONE two -three +THREE +four (indexes 0..5).
        let lines: Vec<&str> = diff.hunks[0]
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        let at = |text: &str, tag| {
            diff.hunks[0]
                .lines
                .iter()
                .position(|l| l.text == text && l.tag == tag)
                .unwrap()
        };
        let picked: std::collections::HashSet<(usize, usize)> = [
            (0, at("one", Tag::Removed)),
            (0, at("ONE", Tag::Added)),
            (0, at("four", Tag::Added)),
        ]
        .into_iter()
        .collect();
        assert!(lines.contains(&"four"));
        block(apply_patch(
            repo.path(),
            &diff.lines_patch(&picked, false).unwrap(),
            false,
        ))
        .unwrap();
        let cached = repo.git(&["show", ":f.txt"]);
        assert_eq!(cached, "ONE\ntwo\nthree\nfour\n");
        // Take two of them out again, one line stays staged.
        let cached_diff = block(diff_of(repo.path(), &Source::Staged, "f.txt")).unwrap();
        let out: std::collections::HashSet<(usize, usize)> = [(
            0,
            cached_diff.hunks[0]
                .lines
                .iter()
                .position(|l| l.text == "four" && l.tag == Tag::Added)
                .unwrap(),
        )]
        .into_iter()
        .collect();
        block(apply_patch(
            repo.path(),
            &cached_diff.lines_patch(&out, true).unwrap(),
            true,
        ))
        .unwrap();
        assert_eq!(repo.git(&["show", ":f.txt"]), "ONE\ntwo\nthree\n");
        // The file itself never changed.
        assert_eq!(
            std::fs::read_to_string(repo.path().join("f.txt")).unwrap(),
            "ONE\ntwo\nTHREE\nfour\n"
        );
    }

    #[test]
    fn a_diff_can_ignore_blanks_and_show_more_context() {
        let repo = Repo::new();
        repo.write("f.txt", "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
        repo.commit_all("first");
        repo.write("f.txt", "a\nb\nc\nd\ne\n f\ng\nh\ni\nJ\n");
        let read =
            |options| block(diff_with(repo.path(), &Source::Unstaged, "f.txt", options)).unwrap();
        // Default: the indented line counts.
        assert_eq!(read(DiffOptions::default()).changes(), 2);
        // Blanks ignored: only the real change.
        let ignoring = read(DiffOptions {
            ignore_blanks: true,
            ..Default::default()
        });
        assert_eq!(ignoring.changes(), 1);
        // More context: the whole file in one hunk.
        let whole = read(DiffOptions {
            context: u32::MAX,
            ..Default::default()
        });
        assert_eq!(whole.hunks.len(), 1);
        assert_eq!(whole.hunks[0].lines.len(), 12);
    }

    #[test]
    fn two_commits_can_be_compared() {
        let repo = Repo::new();
        repo.write("f.txt", "one\n");
        repo.commit_all("first");
        repo.write("f.txt", "one\ntwo\n");
        repo.commit_all("second");
        repo.write("f.txt", "one\ntwo\nthree\n");
        repo.commit_all("third");
        let (a, c) = (
            repo.git(&["rev-parse", "HEAD~2"]),
            repo.git(&["rev-parse", "HEAD"]),
        );
        let diff = block(diff_of(
            repo.path(),
            &Source::Range(a.trim().into(), c.trim().into()),
            "f.txt",
        ))
        .unwrap();
        assert_eq!(diff.added(), 2);
    }

    #[test]
    fn fetch_brings_the_remote_in_without_touching_the_files_and_prunes() {
        let (a, b, _remote) = two_clones();
        b.write("b.txt", "from b\n");
        b.commit_all("on b");
        b.git(&["push", "-q"]);
        block(fetch(a.path())).unwrap();
        let status = block(status(a.path(), false)).unwrap();
        assert_eq!(status.behind, 1);
        assert!(
            !a.path().join("b.txt").exists(),
            "the files are not touched"
        );
        // A branch deleted on the remote is dropped from the tracking ones.
        b.git(&["push", "-q", "origin", "HEAD:refs/heads/gone"]);
        block(fetch(a.path())).unwrap();
        assert!(a.git(&["branch", "-r"]).contains("origin/gone"));
        b.git(&["push", "-q", "origin", "--delete", "gone"]);
        block(fetch(a.path())).unwrap();
        assert!(!a.git(&["branch", "-r"]).contains("origin/gone"));
    }

    #[test]
    fn reset_moves_the_branch_and_the_last_commit_can_be_taken_back() {
        let repo = Repo::new();
        repo.write("f.txt", "one\n");
        repo.commit_all("first");
        repo.write("f.txt", "two\n");
        repo.commit_all("second");
        repo.write("f.txt", "three\n");
        repo.commit_all("third");
        // Soft: the changes stay staged. Mixed: in the files. Hard: lost.
        block(reset(repo.path(), "HEAD~1", ResetMode::Soft)).unwrap();
        assert!(repo.git(&["status", "--porcelain"]).starts_with("M  "));
        block(reset(repo.path(), "HEAD", ResetMode::Mixed)).unwrap();
        assert!(repo.git(&["status", "--porcelain"]).starts_with(" M"));
        block(reset(repo.path(), "HEAD~1", ResetMode::Hard)).unwrap();
        assert!(repo.git(&["status", "--porcelain"]).trim().is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("f.txt")).unwrap(),
            "one\n"
        );
        // A name that is an option is refused.
        assert!(block(reset(repo.path(), "--hard", ResetMode::Hard)).is_err());
        // Undoing the last commit gives its message back.
        repo.write("g.txt", "g\n");
        repo.commit_all("to be taken back");
        let message = block(undo_last_commit(repo.path())).unwrap();
        assert_eq!(message, "to be taken back");
        assert!(repo.git(&["status", "--porcelain"]).contains("A  g.txt"));
        assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "first");
    }

    #[test]
    fn one_file_can_be_stashed_alone_and_a_file_can_be_ignored() {
        let repo = Repo::new();
        repo.write("a.txt", "a\n");
        repo.write("b.txt", "b\n");
        repo.commit_all("first");
        repo.write("a.txt", "a changed\n");
        repo.write("b.txt", "b changed\n");
        block(stash_file(repo.path(), "a.txt", "just a")).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "a\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.path().join("b.txt")).unwrap(),
            "b changed\n"
        );
        assert!(repo.git(&["stash", "list"]).contains("just a"));
        // Ignoring a file git doesn't track: a line in .gitignore.
        repo.write("scratch.log", "noise\n");
        assert_eq!(block(ignore_file(repo.path(), "scratch.log")), Ok(false));
        assert_eq!(
            std::fs::read_to_string(repo.path().join(".gitignore")).unwrap(),
            "/scratch.log\n"
        );
        assert!(!repo.git(&["status", "--porcelain"]).contains("scratch.log"));
        // Again: not added twice. A tracked file leaves the index, stays on disk.
        assert_eq!(block(ignore_file(repo.path(), "scratch.log")), Ok(false));
        assert_eq!(block(ignore_file(repo.path(), "b.txt")), Ok(true));
        assert_eq!(
            std::fs::read_to_string(repo.path().join(".gitignore")).unwrap(),
            "/scratch.log\n/b.txt\n"
        );
        assert!(repo.path().join("b.txt").exists());
        assert!(repo.git(&["status", "--porcelain"]).contains("D  b.txt"));
    }
}
