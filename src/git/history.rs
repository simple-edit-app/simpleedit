//! The log, the branches, the stashes, blame, and what rewrites history
//! (revert, cherry-pick, drop, squash, reword).

use std::collections::HashSet;
use std::path::Path;

use super::graph::Commit;
use super::{run, run_network, run_with, Change, Kind};

// ─── The log ────────────────────────────────────────────────────────────────

const FIELD: char = '\x1f';
const RECORD: char = '\x1e';

/// The commits reachable from `rev` (a branch, a tag, `HEAD`), newest first,
/// children before parents; with a `path`, only those that touched that file
/// (through renames).
pub async fn log(
    repo: &Path,
    rev: &str,
    limit: usize,
    path: Option<&str>,
) -> Result<Vec<Commit>, String> {
    log_matching(repo, rev, limit, path, None).await
}

/// What the commit search looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// The words of the message.
    #[default]
    Message,
    Author,
    /// Text that a commit added or removed (`git log -S`).
    Content,
    /// Commits that touched a path.
    File,
    /// Commits since a date (`2026-01-31`, `2 weeks ago`).
    Since,
}

impl SearchMode {
    pub const ALL: [SearchMode; 5] = [
        SearchMode::Message,
        SearchMode::Author,
        SearchMode::Content,
        SearchMode::File,
        SearchMode::Since,
    ];
}

/// [`log`] restricted by a search `(what to look at, the text)`.
pub async fn log_matching(
    repo: &Path,
    rev: &str,
    limit: usize,
    path: Option<&str>,
    search: Option<(SearchMode, &str)>,
) -> Result<Vec<Commit>, String> {
    let max = format!("--max-count={limit}");
    let mut args = vec![
        "log",
        "--topo-order",
        &max,
        "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s%x1f%D%x1e",
    ];
    if path.is_some() {
        args.push("--follow");
    }
    let found;
    let mut path = path;
    if let Some((mode, text)) = search.filter(|(_, t)| !t.trim().is_empty()) {
        let text = text.trim();
        match mode {
            SearchMode::Message => {
                found = format!("--grep={text}");
                args.extend([found.as_str(), "-i", "--fixed-strings"]);
            }
            SearchMode::Author => {
                found = format!("--author={text}");
                args.extend([found.as_str(), "-i", "--fixed-strings"]);
            }
            SearchMode::Content => {
                found = format!("-S{text}");
                args.push(&found);
            }
            SearchMode::Since => {
                found = format!("--since={text}");
                args.push(&found);
            }
            // A path to follow, as the file history does.
            SearchMode::File => {
                path = Some(text);
                args.push("--follow");
            }
        }
    }
    args.extend(["--end-of-options", rev, "--"]);
    args.extend(path);
    let out = run(repo, &args).await?;
    let mut commits = parse_log(&out);
    if path.is_some() {
        // `%P` names the real parents, which the filtered list mostly lacks:
        // each commit is drawn on top of the next one listed.
        let shas: Vec<String> = commits.iter().map(|c| c.sha.clone()).collect();
        for (i, commit) in commits.iter_mut().enumerate() {
            commit.parents = shas.get(i + 1).cloned().into_iter().collect();
        }
    }
    Ok(commits)
}

pub fn parse_log(out: &str) -> Vec<Commit> {
    out.split(RECORD)
        .filter_map(|record| {
            let record = record.trim_matches(['\n', '\r']);
            let mut f = record.split(FIELD);
            let sha = f.next().filter(|s| !s.is_empty())?.to_string();
            let parents = f.next()?.split_whitespace().map(str::to_string).collect();
            let author = f.next()?.to_string();
            let email = f.next()?.to_string();
            let time = f.next()?.parse().ok()?;
            let subject = f.next()?.to_string();
            let refs = f
                .next()
                .unwrap_or("")
                .split(", ")
                .filter(|r| !r.is_empty())
                .map(|r| r.to_string())
                .collect();
            Some(Commit {
                sha,
                parents,
                author,
                email,
                time,
                subject,
                refs,
            })
        })
        .collect()
}

/// The files a commit changed (against its first parent).
pub async fn commit_files(repo: &Path, commit: &Commit) -> Result<Vec<Change>, String> {
    let out = match commit.parents.first() {
        Some(parent) => {
            run(
                repo,
                &["diff", "--name-status", "-z", "-M", parent, &commit.sha],
            )
            .await?
        }
        None => {
            run(
                repo,
                &[
                    "diff-tree",
                    "--root",
                    "-r",
                    "--no-commit-id",
                    "--name-status",
                    "-z",
                    "-M",
                    &commit.sha,
                ],
            )
            .await?
        }
    };
    Ok(parse_name_status(&out))
}

/// `git diff --name-status -z`: `M\0path\0`, `R100\0old\0new\0`.
pub fn parse_name_status(out: &str) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    while let Some(status) = fields.next() {
        let kind = match status.as_bytes().first() {
            Some(b'A') | Some(b'C') => Kind::Added,
            Some(b'D') => Kind::Deleted,
            Some(b'R') => Kind::Renamed,
            Some(b'U') => Kind::Conflict,
            _ => Kind::Modified,
        };
        if matches!(kind, Kind::Renamed) || status.starts_with('C') {
            let (Some(from), Some(path)) = (fields.next(), fields.next()) else {
                break;
            };
            changes.push(Change {
                path: path.to_string(),
                from: Some(from.to_string()),
                kind,
            });
        } else {
            let Some(path) = fields.next() else { break };
            changes.push(Change {
                path: path.to_string(),
                from: None,
                kind,
            });
        }
    }
    changes
}

/// The whole message of a commit.
pub async fn message(repo: &Path, sha: &str) -> Result<String, String> {
    run(repo, &["show", "-s", "--format=%B", sha])
        .await
        .map(|m| m.trim_end().to_string())
}

/// The branches that contain a commit (local and remote, by name).
pub async fn branches_containing(repo: &Path, sha: &str) -> Result<Vec<String>, String> {
    let out = run(
        repo,
        &[
            "branch",
            "-a",
            "--contains",
            sha,
            "--format=%(refname:short)",
        ],
    )
    .await?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.ends_with("/HEAD") && !l.starts_with("(HEAD"))
        .map(str::to_string)
        .collect())
}

/// The commits of HEAD, to tell which ones a branch's log shares with it.
pub async fn head_commits(repo: &Path, limit: usize) -> Result<HashSet<String>, String> {
    let max = format!("--max-count={limit}");
    let out = run(repo, &["rev-list", &max, "HEAD"]).await?;
    Ok(out.lines().map(str::to_string).collect())
}

// ─── Branches ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// `main`, or `origin/main` for a remote one.
    pub name: String,
    pub sha: String,
    pub upstream: Option<String>,
    pub current: bool,
    pub remote: bool,
    /// Commits it is ahead of / behind its upstream.
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refs {
    pub branches: Vec<Branch>,
    pub tags: Vec<String>,
}

pub async fn refs(repo: &Path) -> Result<Refs, String> {
    let out = run(
        repo,
        &[
            "for-each-ref",
            "--sort=refname",
            "--format=%(refname)%1f%(objectname)%1f%(upstream:short)%1f%(HEAD)%1f%(upstream:track,nobracket)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )
    .await?;
    Ok(parse_refs(&out))
}

pub fn parse_refs(out: &str) -> Refs {
    let mut refs = Refs::default();
    for line in out.lines() {
        let mut f = line.split(FIELD);
        let (Some(full), Some(sha)) = (f.next(), f.next()) else {
            continue;
        };
        let upstream = f.next().filter(|u| !u.is_empty()).map(str::to_string);
        let current = f.next() == Some("*");
        let track = f.next().unwrap_or("");
        let (mut ahead, mut behind) = (0, 0);
        for part in track.split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                behind = n.parse().unwrap_or(0);
            }
        }
        if let Some(name) = full.strip_prefix("refs/heads/") {
            refs.branches.push(Branch {
                name: name.to_string(),
                sha: sha.to_string(),
                upstream,
                current,
                remote: false,
                ahead,
                behind,
            });
        } else if let Some(name) = full.strip_prefix("refs/remotes/") {
            if !name.ends_with("/HEAD") {
                refs.branches.push(Branch {
                    name: name.to_string(),
                    sha: sha.to_string(),
                    upstream: None,
                    current: false,
                    remote: true,
                    ahead: 0,
                    behind: 0,
                });
            }
        } else if let Some(name) = full.strip_prefix("refs/tags/") {
            refs.tags.push(name.to_string());
        }
    }
    refs
}

/// Switches to a branch; a remote one gets a local branch that tracks it.
pub async fn checkout(repo: &Path, branch: &Branch) -> Result<(), String> {
    if branch.remote {
        let local = branch
            .name
            .split_once('/')
            .map_or(branch.name.as_str(), |(_, b)| b);
        // The local branch may already exist.
        if run(repo, &["checkout", local]).await.is_ok() {
            return Ok(());
        }
        run(repo, &["checkout", "--track", &branch.name])
            .await
            .map(|_| ())
    } else {
        run(repo, &["checkout", &branch.name]).await.map(|_| ())
    }
}

/// Creates a branch (from `start`, or where HEAD is) and switches to it;
/// what is not committed comes along.
pub async fn create_branch(repo: &Path, name: &str, start: Option<&str>) -> Result<(), String> {
    let mut args = vec!["checkout", "-b", name];
    args.extend(start);
    run(repo, &args).await.map(|_| ())
}

/// Deletes a local branch, merged or not.
pub async fn delete_branch(repo: &Path, name: &str) -> Result<(), String> {
    run(repo, &["branch", "-D", name]).await.map(|_| ())
}

/// Deletes a branch on its remote (`origin/topic`).
pub async fn delete_remote_branch(repo: &Path, name: &str) -> Result<(), String> {
    let (remote, branch) = name
        .split_once('/')
        .ok_or_else(|| t!("git.err_not_remote", name = name).to_string())?;
    run_network(repo, &["push", remote, "--delete", branch])
        .await
        .map(|_| ())
}

/// Makes a tag at a commit (or where HEAD is); with a message it is an
/// annotated tag.
pub async fn create_tag(
    repo: &Path,
    name: &str,
    at: Option<&str>,
    message: Option<&str>,
) -> Result<(), String> {
    let mut args = vec!["tag"];
    if let Some(message) = message.filter(|m| !m.trim().is_empty()) {
        args.extend(["-a", "-m", message]);
    }
    args.extend(["--end-of-options", name]);
    args.extend(at);
    run(repo, &args).await.map(|_| ())
}

/// Sends a tag to `origin`.
pub async fn push_tag(repo: &Path, name: &str) -> Result<(), String> {
    run_network(repo, &["push", "origin", &format!("refs/tags/{name}")])
        .await
        .map(|_| ())
}

/// Removes a tag from `origin`.
pub async fn delete_remote_tag(repo: &Path, name: &str) -> Result<(), String> {
    run_network(
        repo,
        &["push", "origin", "--delete", &format!("refs/tags/{name}")],
    )
    .await
    .map(|_| ())
}

pub async fn delete_tag(repo: &Path, name: &str) -> Result<(), String> {
    run(repo, &["tag", "-d", name]).await.map(|_| ())
}

/// Pushes a local branch to its upstream (`origin/topic`: the branch `topic`
/// of `origin`, whatever the local one is called); with none, to a branch of
/// the same name on `origin`, which becomes its upstream. `force`:
/// `--force-with-lease`.
pub async fn push_branch(
    repo: &Path,
    name: &str,
    upstream: Option<&str>,
    force: bool,
) -> Result<(), String> {
    let mut args = vec!["push"];
    if force {
        args.push("--force-with-lease");
    }
    let target;
    match upstream.and_then(|u| u.split_once('/')) {
        Some((remote, branch)) => {
            target = format!("{name}:refs/heads/{branch}");
            args.extend([remote, target.as_str()]);
        }
        None => args.extend(["--set-upstream", "origin", name]),
    }
    run_network(repo, &args).await.map(|_| ())
}

/// Switches to a tag or a commit (the head is then detached).
pub async fn checkout_ref(repo: &Path, name: &str) -> Result<(), String> {
    let name = name.to_string();
    run(repo, &["checkout", "--end-of-options", &name])
        .await
        .map(|_| ())
}

/// The files that differ between two commits.
pub async fn range_files(repo: &Path, from: &str, to: &str) -> Result<Vec<Change>, String> {
    let out = run(repo, &["diff", "--name-status", "-z", "-M", from, to]).await?;
    Ok(parse_name_status(&out))
}

impl std::fmt::Display for PlanAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            PlanAction::Pick => t!("git.plan_pick"),
            PlanAction::Reword => t!("git.plan_reword"),
            PlanAction::Edit => t!("git.plan_edit"),
            PlanAction::Squash => t!("git.plan_squash"),
            PlanAction::Fixup => t!("git.plan_fixup"),
            PlanAction::Drop => t!("git.plan_drop"),
        })
    }
}

impl std::fmt::Display for SearchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            SearchMode::Message => t!("git.search_message"),
            SearchMode::Author => t!("git.search_author"),
            SearchMode::Content => t!("git.search_content"),
            SearchMode::File => t!("git.search_file"),
            SearchMode::Since => t!("git.search_since"),
        })
    }
}

/// The files that differ between a ref and the working tree.
pub async fn tree_diff_files(repo: &Path, reference: &str) -> Result<Vec<Change>, String> {
    let out = run(repo, &["diff", "--name-status", "-z", "-M", reference]).await?;
    Ok(parse_name_status(&out))
}

// ─── Stashes ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub index: usize,
    /// What the user wrote, or git's own `WIP on …`.
    pub message: String,
    pub branch: String,
    pub time: i64,
}

pub async fn stashes(repo: &Path) -> Result<Vec<Stash>, String> {
    let out = run(repo, &["stash", "list", "--format=%gd%x1f%gs%x1f%ct%x1e"]).await?;
    Ok(parse_stashes(&out))
}

pub fn parse_stashes(out: &str) -> Vec<Stash> {
    out.split(RECORD)
        .filter_map(|record| {
            let record = record.trim_matches(['\n', '\r']);
            let mut f = record.split(FIELD);
            let name = f.next()?;
            let index = name
                .strip_prefix("stash@{")?
                .strip_suffix('}')?
                .parse()
                .ok()?;
            let subject = f.next()?;
            let time = f.next()?.parse().ok()?;
            // "On main: my message" / "WIP on main: abc1234 subject"
            let (branch, message) = match subject.split_once(": ") {
                Some((head, rest)) => {
                    let branch = head
                        .strip_prefix("WIP on ")
                        .or_else(|| head.strip_prefix("On "))
                        .unwrap_or(head);
                    (branch.to_string(), rest.to_string())
                }
                None => (String::new(), subject.to_string()),
            };
            Some(Stash {
                index,
                message,
                branch,
                time,
            })
        })
        .collect()
}

pub async fn stash_push(repo: &Path, message: &str) -> Result<(), String> {
    let mut args = vec!["stash", "push", "--include-untracked"];
    if !message.trim().is_empty() {
        args.push("-m");
        args.push(message);
    }
    run(repo, &args).await.map(|_| ())
}

pub async fn stash_apply(repo: &Path, index: usize) -> Result<(), String> {
    run(repo, &["stash", "apply", &format!("stash@{{{index}}}")])
        .await
        .map(|_| ())
}

pub async fn stash_pop(repo: &Path, index: usize) -> Result<(), String> {
    run(repo, &["stash", "pop", &format!("stash@{{{index}}}")])
        .await
        .map(|_| ())
}

pub async fn stash_drop(repo: &Path, index: usize) -> Result<(), String> {
    run(repo, &["stash", "drop", &format!("stash@{{{index}}}")])
        .await
        .map(|_| ())
}

/// The files a stash holds, untracked ones included (those come from the
/// stash's third parent).
pub async fn stash_files(repo: &Path, index: usize) -> Result<Vec<Change>, String> {
    let name = format!("stash@{{{index}}}");
    let base = format!("{name}^");
    let out = run(repo, &["diff", "--name-status", "-z", "-M", &base, &name]).await?;
    let mut changes = parse_name_status(&out);
    let untracked = format!("{name}^3");
    if let Ok(list) = run(repo, &["ls-tree", "-r", "--name-only", "-z", &untracked]).await {
        for path in list.split('\0').filter(|p| !p.is_empty()) {
            changes.push(Change {
                path: path.to_string(),
                from: None,
                kind: Kind::Untracked,
            });
        }
    }
    Ok(changes)
}

// ─── Blame ──────────────────────────────────────────────────────────────────

/// Who last changed a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub sha: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub summary: String,
    /// The whole message of the commit (empty until read).
    pub message: std::sync::Arc<str>,
}

impl BlameLine {
    pub fn committed(&self) -> bool {
        !self.sha.chars().all(|c| c == '0')
    }
}

pub async fn blame(repo: &Path, path: &str) -> Result<Vec<BlameLine>, String> {
    blame_at(repo, None, path).await
}

/// The text of a file as a commit had it.
pub async fn file_at(repo: &Path, rev: &str, path: &str) -> Result<String, String> {
    run(repo, &["show", &format!("{rev}:{path}")]).await
}

/// Blame of a file as a commit had it (`None`: the file as it is now).
pub async fn blame_at(
    repo: &Path,
    rev: Option<&str>,
    path: &str,
) -> Result<Vec<BlameLine>, String> {
    let mut args = vec!["blame", "--porcelain"];
    args.extend(rev);
    args.extend(["--", path]);
    let out = run(repo, &args).await?;
    let mut lines = parse_blame(&out);
    // The messages of the commits met, for what a pointer on a line shows.
    let mut shas: Vec<&str> = lines
        .iter()
        .filter(|l| l.committed())
        .map(|l| l.sha.as_str())
        .collect();
    shas.sort_unstable();
    shas.dedup();
    let messages = commit_messages(repo, &shas).await.unwrap_or_default();
    for line in &mut lines {
        if let Some(message) = messages.get(&line.sha) {
            line.message = message.clone();
        }
    }
    Ok(lines)
}

/// The whole message of each commit (by hash).
pub async fn commit_messages(
    repo: &Path,
    shas: &[&str],
) -> Result<std::collections::HashMap<String, std::sync::Arc<str>>, String> {
    if shas.is_empty() {
        return Ok(Default::default());
    }
    let mut args = vec!["log", "--no-walk=unsorted", "--format=%H%x1f%B%x1e"];
    args.extend(shas.iter().copied());
    args.push("--");
    let out = run(repo, &args).await?;
    Ok(out
        .split(RECORD)
        .filter_map(|record| {
            let (sha, message) = record.trim_matches(['\n', '\r']).split_once(FIELD)?;
            Some((sha.to_string(), std::sync::Arc::from(message.trim_end())))
        })
        .collect())
}

/// One entry per line of the file, in order.
pub fn parse_blame(out: &str) -> Vec<BlameLine> {
    use std::collections::HashMap;
    let mut known: HashMap<String, BlameLine> = HashMap::new();
    let mut lines: Vec<BlameLine> = Vec::new();
    let mut current: Option<String> = None;
    for raw in out.lines() {
        if let Some(content) = raw.strip_prefix('\t') {
            let _ = content;
            if let Some(info) = current.as_ref().and_then(|sha| known.get(sha)) {
                lines.push(info.clone());
            }
            current = None;
            continue;
        }
        let first = raw.split(' ').next().unwrap_or("");
        let is_header =
            matches!(first.len(), 40 | 64) && first.chars().all(|c| c.is_ascii_hexdigit());
        if is_header && current.is_none() {
            current = Some(first.to_string());
            known.entry(first.to_string()).or_insert_with(|| BlameLine {
                sha: first.to_string(),
                author: String::new(),
                email: String::new(),
                time: 0,
                summary: String::new(),
                message: std::sync::Arc::from(""),
            });
        } else if let Some(sha) = current.as_ref() {
            if let Some(entry) = known.get_mut(sha) {
                if let Some(a) = raw.strip_prefix("author ") {
                    entry.author = a.to_string();
                } else if let Some(m) = raw.strip_prefix("author-mail ") {
                    entry.email = m.trim_matches(['<', '>']).to_string();
                } else if let Some(t) = raw.strip_prefix("author-time ") {
                    entry.time = t.parse().unwrap_or(0);
                } else if let Some(s) = raw.strip_prefix("summary ") {
                    entry.summary = s.to_string();
                }
            }
        }
    }
    lines
}

// ─── Rewriting history ──────────────────────────────────────────────────────

/// Why a rewrite can't be done, if it can't.
pub async fn is_linear(repo: &Path, base: &str) -> Result<bool, String> {
    let range = format!("{base}..HEAD");
    let out = run(repo, &["rev-list", "--merges", "--count", &range]).await?;
    Ok(out.trim() == "0")
}

/// Rebases the commits after `base` (`None`: from the root) following a todo
/// list, one instruction per line, oldest first. What is not committed is put
/// aside and brought back. `message`: a file the todo's `exec` lines may read
/// (`$SE_MESSAGE`).
async fn rebase_with(
    repo: &Path,
    base: Option<&str>,
    todo: &str,
    message: Option<&Path>,
) -> Result<(), String> {
    let files: Vec<(String, std::path::PathBuf)> = message
        .map(|m| vec![("SE_MESSAGE".to_string(), m.to_path_buf())])
        .unwrap_or_default();
    rebase_todo(repo, base, todo, &files).await
}

/// [`rebase_with`] with any number of files for the `exec` lines to read, each
/// named by the environment variable that holds its path.
async fn rebase_todo(
    repo: &Path,
    base: Option<&str>,
    todo: &str,
    files: &[(String, std::path::PathBuf)],
) -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let todo_path = dir.path().join("todo");
    tokio::fs::write(&todo_path, todo)
        .await
        .map_err(|e| e.to_string())?;
    // The sequence editor copies the todo over the one git made; paths go
    // through the environment, so nothing needs quoting.
    let editor = "sh -c 'cp \"$SE_TODO\" \"$1\"' sh";
    let todo_env = todo_path.to_string_lossy().into_owned();
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(name, path)| (name.clone(), path.to_string_lossy().into_owned()))
        .collect();
    let mut args = vec!["rebase", "-i", "--autostash", "--no-autosquash"];
    match base {
        Some(base) => args.push(base),
        None => args.push("--root"),
    }
    let mut env: Vec<(&str, &str)> = vec![
        ("GIT_SEQUENCE_EDITOR", editor),
        ("GIT_EDITOR", "true"),
        ("SE_TODO", &todo_env),
    ];
    env.extend(owned.iter().map(|(n, p)| (n.as_str(), p.as_str())));
    match run_with(repo, &args, None, &env).await {
        Ok(_) => Ok(()),
        // Conflicts stay to be settled; any other failure leaves things as
        // they were.
        Err(e) => Err(super::stopped(repo, e, &["rebase", "--abort"]).await),
    }
}

/// What is done with a commit in an interactive rebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanAction {
    Pick,
    /// Keeps it with another message.
    Reword,
    /// Stops there, to change it (then `Continue`).
    Edit,
    /// Folds it into the commit before, keeping both messages.
    Squash,
    /// Folds it into the commit before, dropping its message.
    Fixup,
    Drop,
}

impl PlanAction {
    pub const ALL: [PlanAction; 6] = [
        PlanAction::Pick,
        PlanAction::Reword,
        PlanAction::Edit,
        PlanAction::Squash,
        PlanAction::Fixup,
        PlanAction::Drop,
    ];

    fn folds(self) -> bool {
        matches!(self, PlanAction::Squash | PlanAction::Fixup)
    }
}

/// One line of an interactive rebase: a commit, what is done with it, and
/// (for a reword) its new message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    pub sha: String,
    pub action: PlanAction,
    pub subject: String,
    /// The whole message: the commit's own, or what it is changed to.
    pub message: String,
}

/// The commits an interactive rebase from `base` would replay, oldest first,
/// all to be picked.
pub async fn plan_from(repo: &Path, base: Option<&str>) -> Result<Vec<PlanItem>, String> {
    ensure_linear(repo, base).await?;
    let shas = commits_after(repo, base).await?;
    let refs: Vec<&str> = shas.iter().map(String::as_str).collect();
    let messages = commit_messages(repo, &refs).await?;
    Ok(shas
        .iter()
        .map(|sha| {
            let message = messages.get(sha).map(|m| m.to_string()).unwrap_or_default();
            PlanItem {
                sha: sha.clone(),
                action: PlanAction::Pick,
                subject: message.lines().next().unwrap_or("").to_string(),
                message,
            }
        })
        .collect())
}

/// Why a plan can't be run, if it can't.
pub fn check_plan(plan: &[PlanItem]) -> Result<(), String> {
    let first = plan.iter().find(|i| i.action != PlanAction::Drop);
    match first {
        Some(item) if item.action.folds() => Err(t!("git.err_plan_first").to_string()),
        _ => Ok(()),
    }
}

/// Runs an interactive rebase: the commits after `base`, in the order of the
/// plan, each picked, reworded, stopped at, folded or dropped.
pub async fn interactive_rebase(
    repo: &Path,
    base: Option<&str>,
    plan: &[PlanItem],
) -> Result<(), String> {
    check_plan(plan)?;
    ensure_linear(repo, base).await?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut todo = String::new();
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    // The new message of the commit a group of folded ones ends on.
    let mut reword: Option<usize> = None;
    let close_group = |todo: &mut String, reword: &mut Option<usize>| {
        if let Some(n) = reword.take() {
            todo.push_str(&format!(
                "exec git commit --amend -q -F \"$SE_MESSAGE_{n}\"\n"
            ));
        }
    };
    for item in plan.iter().filter(|i| i.action != PlanAction::Drop) {
        if !item.action.folds() {
            close_group(&mut todo, &mut reword);
        }
        let word = match item.action {
            PlanAction::Edit => "edit",
            PlanAction::Squash => "squash",
            PlanAction::Fixup => "fixup",
            _ => "pick",
        };
        todo.push_str(&format!("{word} {}\n", item.sha));
        if item.action == PlanAction::Reword {
            let n = files.len();
            let path = dir.path().join(format!("message{n}"));
            tokio::fs::write(&path, &item.message)
                .await
                .map_err(|e| e.to_string())?;
            files.push((format!("SE_MESSAGE_{n}"), path));
            reword = Some(n);
        }
    }
    close_group(&mut todo, &mut reword);
    // Everything dropped leaves nothing to pick, which git refuses.
    if todo.is_empty() {
        todo.push_str("noop\n");
    }
    rebase_todo(repo, base, &todo, &files).await
}

/// The commits of `base..HEAD`, oldest first.
async fn commits_after(repo: &Path, base: Option<&str>) -> Result<Vec<String>, String> {
    let range = base.map_or("HEAD".to_string(), |b| format!("{b}..HEAD"));
    let out = run(repo, &["rev-list", "--reverse", "--topo-order", &range]).await?;
    Ok(out.lines().map(str::to_string).collect())
}

/// The parent of a commit (`None` for a root commit).
async fn parent_of(repo: &Path, sha: &str) -> Result<Option<String>, String> {
    let out = run(repo, &["rev-list", "--parents", "-n", "1", sha]).await?;
    Ok(out.split_whitespace().nth(1).map(str::to_string))
}

/// Removes commits from the current branch.
pub async fn drop_commits(repo: &Path, shas: &[String]) -> Result<(), String> {
    let oldest = oldest_of(repo, shas).await?;
    let base = parent_of(repo, &oldest).await?;
    ensure_linear(repo, base.as_deref()).await?;
    let all = commits_after(repo, base.as_deref()).await?;
    let todo: String = all
        .iter()
        .filter(|c| !shas.contains(c))
        .map(|c| format!("pick {c}\n"))
        .collect();
    // Dropping every commit leaves nothing to pick, which git refuses.
    let todo = if todo.is_empty() {
        "noop\n".to_string()
    } else {
        todo
    };
    rebase_with(repo, base.as_deref(), &todo, None).await
}

/// Joins consecutive commits into one with `message`.
pub async fn squash_commits(repo: &Path, shas: &[String], message: &str) -> Result<(), String> {
    let oldest = oldest_of(repo, shas).await?;
    let base = parent_of(repo, &oldest).await?;
    ensure_linear(repo, base.as_deref()).await?;
    let all = commits_after(repo, base.as_deref()).await?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let message_path = dir.path().join("message");
    tokio::fs::write(&message_path, message)
        .await
        .map_err(|e| e.to_string())?;
    let mut todo = String::new();
    let mut first_selected = true;
    let mut last_selected = None;
    for (i, c) in all.iter().enumerate() {
        if shas.contains(c) {
            todo.push_str(if first_selected { "pick " } else { "fixup " });
            todo.push_str(c);
            todo.push('\n');
            first_selected = false;
            last_selected = Some(i);
        } else {
            todo.push_str(&format!("pick {c}\n"));
        }
        // The message is set as soon as the last selected one is folded in.
        if last_selected == Some(i)
            && shas.iter().filter(|s| all[..=i].contains(s)).count() == shas.len()
        {
            todo.push_str("exec git commit --amend -q -F \"$SE_MESSAGE\"\n");
        }
    }
    rebase_with(repo, base.as_deref(), &todo, Some(&message_path)).await
}

/// Changes the message of a commit.
pub async fn reword(repo: &Path, sha: &str, message: &str) -> Result<(), String> {
    let head = run(repo, &["rev-parse", "HEAD"]).await?;
    if head.trim() == sha {
        return run_with(
            repo,
            &["commit", "--amend", "--only", "-q", "-F", "-"],
            Some(message),
            &[],
        )
        .await
        .map(|_| ());
    }
    let base = parent_of(repo, sha).await?;
    ensure_linear(repo, base.as_deref()).await?;
    let all = commits_after(repo, base.as_deref()).await?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let message_path = dir.path().join("message");
    tokio::fs::write(&message_path, message)
        .await
        .map_err(|e| e.to_string())?;
    let mut todo = String::new();
    for c in &all {
        todo.push_str(&format!("pick {c}\n"));
        if c == sha {
            todo.push_str("exec git commit --amend -q -F \"$SE_MESSAGE\"\n");
        }
    }
    rebase_with(repo, base.as_deref(), &todo, Some(&message_path)).await
}

async fn ensure_linear(repo: &Path, base: Option<&str>) -> Result<(), String> {
    let linear = match base {
        Some(base) => is_linear(repo, base).await?,
        None => {
            run(repo, &["rev-list", "--merges", "--count", "HEAD"])
                .await?
                .trim()
                == "0"
        }
    };
    if linear {
        Ok(())
    } else {
        Err(t!("git.err_merge_commits").to_string())
    }
}

/// Whether `ancestor` is `of` or one of its ancestors.
async fn is_ancestor(repo: &Path, ancestor: &str, of: &str) -> bool {
    run(repo, &["merge-base", "--is-ancestor", ancestor, of])
        .await
        .is_ok()
}

/// The oldest of some commits of the current branch (they lie on one line of
/// history, so one of them is an ancestor of all the others).
async fn oldest_of(repo: &Path, shas: &[String]) -> Result<String, String> {
    let mut oldest: Option<&String> = None;
    for sha in shas {
        if !is_ancestor(repo, sha, "HEAD").await {
            return Err(t!("git.err_not_in_branch").to_string());
        }
        oldest = match oldest {
            Some(best) if !is_ancestor(repo, sha, best).await => Some(best),
            _ => Some(sha),
        };
    }
    oldest
        .cloned()
        .ok_or_else(|| t!("git.err_not_in_branch").to_string())
}

/// Adds commits that undo `shas` (newest first).
pub async fn revert(repo: &Path, shas: &[String]) -> Result<(), String> {
    let mut args = vec!["revert", "--no-edit"];
    args.extend(shas.iter().map(String::as_str));
    match run(repo, &args).await {
        Ok(_) => Ok(()),
        Err(e) => Err(super::stopped(repo, e, &["revert", "--abort"]).await),
    }
}

/// Copies commits (oldest first) onto the current branch.
pub async fn cherry_pick(repo: &Path, shas: &[String]) -> Result<(), String> {
    let mut args = vec!["cherry-pick"];
    args.extend(shas.iter().map(String::as_str));
    match run(repo, &args).await {
        Ok(_) => Ok(()),
        Err(e) => Err(super::stopped(repo, e, &["cherry-pick", "--abort"]).await),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    fn history(repo: &Repo) -> Vec<String> {
        repo.git(&["log", "--format=%s"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn shas(repo: &Repo) -> Vec<String> {
        repo.git(&["log", "--format=%H"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn four_commits() -> Repo {
        let repo = Repo::new();
        for (i, name) in ["one", "two", "three", "four"].iter().enumerate() {
            repo.write(&format!("{name}.txt"), &format!("{i}\n"));
            repo.commit_all(name);
        }
        repo
    }

    #[test]
    fn the_log_lists_commits_with_parents_and_refs() {
        let repo = four_commits();
        repo.git(&["tag", "v1"]);
        let commits = block(log(repo.path(), "HEAD", 10, None)).unwrap();
        assert_eq!(commits.len(), 4);
        assert_eq!(commits[0].subject, "four");
        assert_eq!(commits[0].parents, vec![commits[1].sha.clone()]);
        assert!(commits[3].parents.is_empty());
        assert!(commits[0].refs.iter().any(|r| r.contains("main")));
        assert!(commits[0].refs.iter().any(|r| r == "tag: v1"));
        assert_eq!(commits[0].author, "Test");
        let limited = block(log(repo.path(), "HEAD", 2, None)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn the_log_of_a_file_lists_only_the_commits_that_touched_it() {
        let repo = four_commits();
        repo.write("one.txt", "changed\n");
        repo.commit_all("five");
        let commits = block(log(repo.path(), "HEAD", 10, Some("one.txt"))).unwrap();
        let subjects: Vec<&str> = commits.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, vec!["five", "one"]);
    }

    #[test]
    fn a_commit_lists_its_files() {
        let repo = four_commits();
        let commits = block(log(repo.path(), "HEAD", 10, None)).unwrap();
        let files = block(commit_files(repo.path(), &commits[0])).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "four.txt");
        assert_eq!(files[0].kind, Kind::Added);
        // The root commit too.
        let files = block(commit_files(repo.path(), &commits[3])).unwrap();
        assert_eq!(files[0].path, "one.txt");
        assert_eq!(
            block(message(repo.path(), &commits[0].sha)).unwrap(),
            "four"
        );
    }

    #[test]
    fn refs_list_branches_with_the_current_one_and_tags() {
        let repo = four_commits();
        repo.git(&["branch", "feature"]);
        repo.git(&["tag", "v1"]);
        let refs = block(refs(repo.path())).unwrap();
        let names: Vec<_> = refs
            .branches
            .iter()
            .map(|b| (b.name.as_str(), b.current))
            .collect();
        assert_eq!(names, vec![("feature", false), ("main", true)]);
        assert_eq!(refs.tags, vec!["v1".to_string()]);
        let feature = refs
            .branches
            .iter()
            .find(|b| b.name == "feature")
            .unwrap()
            .clone();
        block(checkout(repo.path(), &feature)).unwrap();
        assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "feature");
    }

    #[test]
    fn a_tree_diff_lists_what_differs_from_a_branch() {
        let repo = four_commits();
        repo.git(&["branch", "old", "HEAD~2"]);
        let files = block(tree_diff_files(repo.path(), "old")).unwrap();
        let paths: Vec<_> = files.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, vec!["four.txt", "three.txt"]);
    }

    #[test]
    fn stashes_can_be_created_listed_applied_popped_and_dropped() {
        let repo = four_commits();
        repo.write("one.txt", "changed\n");
        repo.write("untracked.txt", "u\n");
        block(stash_push(repo.path(), "my work")).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("one.txt")).unwrap(),
            "0\n"
        );
        assert!(!repo.path().join("untracked.txt").exists());

        let list = block(stashes(repo.path())).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].message, "my work");
        assert_eq!(list[0].branch, "main");
        let files = block(stash_files(repo.path(), 0)).unwrap();
        let paths: Vec<_> = files.iter().map(|c| (c.path.as_str(), c.kind)).collect();
        assert_eq!(
            paths,
            vec![
                ("one.txt", Kind::Modified),
                ("untracked.txt", Kind::Untracked)
            ]
        );

        block(stash_apply(repo.path(), 0)).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("one.txt")).unwrap(),
            "changed\n"
        );
        assert_eq!(block(stashes(repo.path())).unwrap().len(), 1);
        repo.git(&["checkout", "-q", "--", "."]);
        repo.git(&["clean", "-fdq"]);
        block(stash_pop(repo.path(), 0)).unwrap();
        assert!(block(stashes(repo.path())).unwrap().is_empty());

        block(stash_push(repo.path(), "")).unwrap();
        block(stash_drop(repo.path(), 0)).unwrap();
        assert!(block(stashes(repo.path())).unwrap().is_empty());
    }

    #[test]
    fn blame_says_who_changed_each_line() {
        let repo = Repo::new();
        repo.write("a.txt", "one\ntwo\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\nTWO\nthree\n");
        repo.commit_all("second");
        repo.write("a.txt", "one\nTWO\nthree\nfour\n");
        let lines = block(blame(repo.path(), "a.txt")).unwrap();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].summary, "first");
        assert_eq!(lines[1].summary, "second");
        assert_eq!(lines[2].summary, "second");
        assert_eq!(lines[0].author, "Test");
        assert!(lines[0].committed());
        assert!(!lines[3].committed());
    }

    #[test]
    fn dropping_a_commit_in_the_middle_removes_only_it() {
        let repo = four_commits();
        let all = shas(&repo);
        block(drop_commits(repo.path(), &[all[1].clone()])).unwrap(); // "three"
        assert_eq!(history(&repo), vec!["four", "two", "one"]);
        assert!(!repo.path().join("three.txt").exists());
    }

    #[test]
    fn squashing_joins_commits_under_the_chosen_message() {
        let repo = four_commits();
        let all = shas(&repo);
        block(squash_commits(
            repo.path(),
            &[all[2].clone(), all[1].clone()], // two + three
            "two and three",
        ))
        .unwrap();
        assert_eq!(history(&repo), vec!["four", "two and three", "one"]);
        assert!(repo.path().join("two.txt").exists());
        assert!(repo.path().join("three.txt").exists());
    }

    #[test]
    fn rewording_changes_one_message_and_keeps_the_rest() {
        let repo = four_commits();
        let all = shas(&repo);
        block(reword(repo.path(), &all[2], "second, better")).unwrap(); // "two"
        assert_eq!(
            history(&repo),
            vec!["four", "three", "second, better", "one"]
        );
        block(reword(repo.path(), &shas(&repo)[0], "last, better")).unwrap();
        assert_eq!(history(&repo)[0], "last, better");
    }

    #[test]
    fn reverting_and_cherry_picking() {
        let repo = four_commits();
        let all = shas(&repo);
        block(revert(repo.path(), &[all[0].clone()])).unwrap();
        assert!(!repo.path().join("four.txt").exists());
        assert!(history(&repo)[0].starts_with("Revert"));

        repo.git(&["checkout", "-q", "-b", "side", &all[3]]);
        repo.write("side.txt", "s\n");
        repo.commit_all("side work");
        let side = shas(&repo)[0].clone();
        repo.git(&["checkout", "-q", "main"]);
        block(cherry_pick(repo.path(), &[side])).unwrap();
        assert_eq!(history(&repo)[0], "side work");
        assert!(repo.path().join("side.txt").exists());
    }

    #[test]
    fn a_revert_that_conflicts_stays_in_progress_until_it_is_aborted() {
        use super::super::{abort_operation, status, Operation, CONFLICT};
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.write("a.txt", "two\n");
        repo.commit_all("second");
        repo.write("a.txt", "three\n");
        repo.commit_all("third");
        let all = shas(&repo);
        // Reverting "second" conflicts with "third".
        let error = block(revert(repo.path(), &[all[1].clone()])).unwrap_err();
        assert_eq!(error, CONFLICT);
        let now = block(status(repo.path(), true)).unwrap();
        assert_eq!(now.operation, Some(Operation::Revert));
        assert_eq!(now.conflicts().count(), 1);
        block(abort_operation(repo.path(), Operation::Revert)).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "three\n"
        );
        assert_eq!(history(&repo).len(), 3);
        assert!(repo.git(&["status", "--porcelain"]).trim().is_empty());
    }

    #[test]
    fn rewriting_across_a_merge_is_refused() {
        let repo = Repo::new();
        repo.write("a.txt", "1\n");
        repo.commit_all("base");
        repo.git(&["checkout", "-q", "-b", "side"]);
        repo.write("b.txt", "2\n");
        repo.commit_all("side");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("c.txt", "3\n");
        repo.commit_all("main work");
        repo.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
        repo.write("d.txt", "4\n");
        repo.commit_all("after");
        let base = shas(&repo).last().unwrap().clone();
        let err = block(drop_commits(repo.path(), &[base])).unwrap_err();
        assert!(err.contains("merge"));
    }

    #[test]
    fn which_branches_contain_a_commit() {
        let repo = four_commits();
        repo.git(&["branch", "old", "HEAD~2"]);
        let all = shas(&repo);
        let mut head_only = block(branches_containing(repo.path(), &all[0])).unwrap();
        head_only.sort();
        assert_eq!(head_only, vec!["main".to_string()]);
        let mut shared = block(branches_containing(repo.path(), &all[3])).unwrap();
        shared.sort();
        assert_eq!(shared, vec!["main".to_string(), "old".to_string()]);
        let set = block(head_commits(repo.path(), 100)).unwrap();
        assert_eq!(set.len(), 4);
    }

    #[test]
    fn name_status_output_handles_renames() {
        let changes = parse_name_status("M\0a.txt\0R100\0old.txt\0new.txt\0D\0gone.txt\0");
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[1].kind, Kind::Renamed);
        assert_eq!(changes[1].from.as_deref(), Some("old.txt"));
        assert_eq!(changes[2].kind, Kind::Deleted);
    }

    fn with_remote(repo: &Repo) -> tempfile::TempDir {
        let remote = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .current_dir(remote.path())
            .status()
            .unwrap();
        repo.git(&["remote", "add", "origin", remote.path().to_str().unwrap()]);
        remote
    }

    #[test]
    fn a_new_branch_starts_at_head_or_at_a_ref_and_takes_the_changes_along() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["tag", "v1"]);
        repo.write("a.txt", "one\ntwo\n");
        repo.commit_all("second");
        repo.write("a.txt", "one\ntwo\nthree\n");
        block(create_branch(repo.path(), "feature", None)).unwrap();
        assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "feature");
        assert_eq!(repo.git(&["status", "--porcelain"]).lines().count(), 1);
        repo.git(&["stash", "-q"]);
        block(create_branch(repo.path(), "old", Some("v1"))).unwrap();
        assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "first");
        // A name git refuses, or one that exists, is an error.
        assert!(block(create_branch(repo.path(), "bad name", None)).is_err());
        assert!(block(create_branch(repo.path(), "feature", None)).is_err());
    }

    #[test]
    fn merging_and_rebasing_a_branch_and_deleting_it() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["checkout", "-q", "-b", "side"]);
        repo.write("b.txt", "side\n");
        repo.commit_all("on side");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("c.txt", "main\n");
        repo.commit_all("on main");
        block(super::super::merge_branch(
            repo.path(),
            "side",
            super::super::MergeMode::Default,
        ))
        .unwrap();
        assert!(repo.path().join("b.txt").exists());
        block(delete_branch(repo.path(), "side")).unwrap();
        assert!(!repo.git(&["branch"]).contains("side"));
        // Rebase: another branch replayed on main.
        repo.git(&["checkout", "-q", "-b", "topic", "HEAD~1"]);
        repo.write("d.txt", "topic\n");
        repo.commit_all("on topic");
        block(super::super::rebase_onto(repo.path(), "main")).unwrap();
        assert!(repo.path().join("c.txt").exists());
        assert!(repo.path().join("d.txt").exists());
    }

    #[test]
    fn a_conflicting_merge_is_left_to_settle() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["checkout", "-q", "-b", "side"]);
        repo.write("a.txt", "side\n");
        repo.commit_all("side");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("a.txt", "main\n");
        repo.commit_all("main");
        assert_eq!(
            block(super::super::merge_branch(
                repo.path(),
                "side",
                super::super::MergeMode::Default
            ))
            .unwrap_err(),
            crate::git::CONFLICT
        );
        assert!(block(crate::git::has_conflicts(repo.path())));
    }

    #[test]
    fn a_branch_is_pushed_forced_and_deleted_on_the_remote() {
        let repo = Repo::new();
        let remote = with_remote(&repo);
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        // No upstream yet: it becomes one.
        block(push_branch(repo.path(), "main", None, false)).unwrap();
        repo.git(&["checkout", "-q", "-b", "topic"]);
        block(push_branch(repo.path(), "topic", None, false)).unwrap();
        repo.git(&["commit", "-q", "--amend", "-m", "reworded"]);
        assert!(block(push_branch(
            repo.path(),
            "topic",
            Some("origin/topic"),
            false
        ))
        .is_err());
        block(push_branch(
            repo.path(),
            "topic",
            Some("origin/topic"),
            true,
        ))
        .unwrap();
        let there = std::process::Command::new("git")
            .args(["log", "-1", "--format=%s", "topic"])
            .current_dir(remote.path())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&there.stdout).trim(), "reworded");
        block(delete_remote_branch(repo.path(), "origin/topic")).unwrap();
        let left = std::process::Command::new("git")
            .args(["branch"])
            .current_dir(remote.path())
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&left.stdout).contains("topic"));
    }

    #[test]
    fn a_tag_is_deleted() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["tag", "v1"]);
        block(delete_tag(repo.path(), "v1")).unwrap();
        assert!(repo.git(&["tag"]).trim().is_empty());
    }

    #[test]
    fn the_last_commit_can_be_dropped() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\ntwo\n");
        repo.commit_all("second");
        let all = shas(&repo);
        block(drop_commits(repo.path(), &[all[0].clone()])).unwrap();
        assert_eq!(history(&repo), vec!["first"]);
    }

    #[test]
    fn rewording_the_last_commit_leaves_the_index_out_of_it() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.write("b.txt", "staged\n");
        repo.git(&["add", "b.txt"]);
        let head = shas(&repo)[0].clone();
        block(reword(repo.path(), &head, "better")).unwrap();
        assert_eq!(history(&repo), vec!["better"]);
        // Still staged, not in the commit.
        assert!(
            repo.git(&["show", "--name-only", "--format=", "HEAD"])
                .trim()
                == "a.txt"
        );
        assert!(repo.git(&["status", "--porcelain"]).contains("A  b.txt"));
    }

    #[test]
    fn history_rewrites_work_with_uncommitted_changes_around() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\ntwo\n");
        repo.commit_all("second");
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.commit_all("third");
        repo.write("a.txt", "one\ntwo\nthree\nwork in progress\n");
        let all = shas(&repo);
        block(reword(repo.path(), &all[1], "second, reworded")).unwrap();
        assert_eq!(history(&repo), vec!["third", "second, reworded", "first"]);
        assert!(std::fs::read_to_string(repo.path().join("a.txt"))
            .unwrap()
            .contains("work in progress"));
    }

    #[test]
    fn a_squash_message_with_a_quote_in_the_temp_path_machinery_is_still_exact() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.write("a.txt", "one\ntwo\n");
        repo.commit_all("second");
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.commit_all("third");
        let all = shas(&repo);
        block(squash_commits(
            repo.path(),
            &[all[0].clone(), all[1].clone()],
            "it's \"two in one\"",
        ))
        .unwrap();
        assert_eq!(history(&repo), vec!["it's \"two in one\"", "first"]);
    }

    #[test]
    fn a_files_history_is_drawn_as_one_line() {
        let repo = Repo::new();
        for i in 1..=4 {
            repo.write("a.txt", &format!("{i}\n"));
            repo.write("other.txt", &format!("{i}\n"));
            repo.commit_all(&format!("c{i}"));
            // A commit that doesn't touch a.txt, between the others.
            repo.write("other.txt", &format!("{i}{i}\n"));
            repo.commit_all(&format!("o{i}"));
        }
        let commits = block(log(repo.path(), "HEAD", 100, Some("a.txt"))).unwrap();
        assert_eq!(commits.len(), 4);
        let rows = super::super::graph::layout(&commits);
        assert_eq!(super::super::graph::width(&rows), 1);
        assert_eq!(commits[0].parents, vec![commits[1].sha.clone()]);
        assert!(commits[3].parents.is_empty());
    }

    #[test]
    fn a_branch_is_pushed_to_the_branch_its_upstream_names() {
        let repo = Repo::new();
        let remote = with_remote(&repo);
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        block(push_branch(repo.path(), "main", None, false)).unwrap();
        repo.git(&["checkout", "-q", "-b", "feat"]);
        repo.git(&["push", "-q", "origin", "feat:refs/heads/feature-x"]);
        repo.git(&["fetch", "-q", "origin"]);
        repo.git(&["branch", "--set-upstream-to=origin/feature-x"]);
        repo.write("b.txt", "more\n");
        repo.commit_all("more");
        block(push_branch(
            repo.path(),
            "feat",
            Some("origin/feature-x"),
            false,
        ))
        .unwrap();
        let branches = std::process::Command::new("git")
            .args(["branch"])
            .current_dir(remote.path())
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&branches.stdout).into_owned();
        assert!(branches.contains("feature-x"));
        // Not a second branch under the local name.
        assert!(!branches.split_whitespace().any(|b| b == "feat"));
    }

    #[test]
    fn a_tag_can_be_checked_out() {
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.commit_all("first");
        repo.git(&["tag", "v1"]);
        repo.write("a.txt", "two\n");
        repo.commit_all("second");
        block(checkout_ref(repo.path(), "v1")).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "one\n"
        );
        assert!(repo.git(&["branch", "--show-current"]).trim().is_empty());
    }

    #[test]
    fn blame_lines_of_a_sha256_repository_are_read() {
        let sha = "a".repeat(64);
        let out =
            format!("{sha} 1 1 1\nauthor Me\nauthor-time 100\nsummary hi\nfilename f\n\tline\n");
        let lines = parse_blame(&out);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].author, "Me");
    }

    fn make_commit(repo: &Repo, files: &[(&str, &str)], message: &str) {
        for (name, text) in files {
            repo.write(name, text);
        }
        repo.commit_all(message);
    }

    /// Three commits, each adding a file.
    fn three(repo: &Repo) {
        make_commit(repo, &[("a.txt", "a\n")], "first");
        make_commit(repo, &[("b.txt", "b\n")], "second");
        make_commit(repo, &[("c.txt", "c\n")], "third");
        make_commit(repo, &[("d.txt", "d\n")], "fourth");
    }

    fn item(plan: &[PlanItem], n: usize, action: PlanAction) -> PlanItem {
        PlanItem {
            action,
            ..plan[n].clone()
        }
    }

    #[test]
    fn a_plan_lists_the_commits_after_the_base_oldest_first_with_their_messages() {
        let repo = Repo::new();
        three(&repo);
        let first = shas(&repo)[3].clone();
        let plan = block(plan_from(repo.path(), Some(&first))).unwrap();
        let subjects: Vec<&str> = plan.iter().map(|i| i.subject.as_str()).collect();
        assert_eq!(subjects, vec!["second", "third", "fourth"]);
        assert!(plan.iter().all(|i| i.action == PlanAction::Pick));
        // From the root: all of them.
        assert_eq!(block(plan_from(repo.path(), None)).unwrap().len(), 4);
    }

    #[test]
    fn an_interactive_rebase_reorders_drops_and_rewords() {
        let repo = Repo::new();
        three(&repo);
        let first = shas(&repo)[3].clone();
        let plan = block(plan_from(repo.path(), Some(&first))).unwrap();
        // second, third, fourth  →  fourth, (third dropped), second reworded.
        let mut reworded = item(&plan, 0, PlanAction::Reword);
        reworded.message = "it's the \"second\", renamed\n\nWith a body.".into();
        let new_plan = vec![
            item(&plan, 2, PlanAction::Pick),
            item(&plan, 1, PlanAction::Drop),
            reworded,
        ];
        block(interactive_rebase(repo.path(), Some(&first), &new_plan)).unwrap();
        assert_eq!(
            history(&repo),
            vec!["it's the \"second\", renamed", "fourth", "first"]
        );
        assert!(!repo.path().join("c.txt").exists());
        assert!(repo
            .git(&["log", "-1", "--format=%b"])
            .contains("With a body."));
    }

    #[test]
    fn folded_commits_join_the_one_before_with_or_without_their_message() {
        let repo = Repo::new();
        three(&repo);
        let first = shas(&repo)[3].clone();
        let plan = block(plan_from(repo.path(), Some(&first))).unwrap();
        // fixup the third into the second, squash the fourth into them.
        let new_plan = vec![
            item(&plan, 0, PlanAction::Pick),
            item(&plan, 1, PlanAction::Fixup),
            item(&plan, 2, PlanAction::Squash),
        ];
        block(interactive_rebase(repo.path(), Some(&first), &new_plan)).unwrap();
        let log = history(&repo);
        assert_eq!(log.len(), 2, "{log:?}");
        assert_eq!(log[0], "second");
        // All three files are there, in one commit.
        let files = repo.git(&["show", "--name-only", "--format=", "HEAD"]);
        assert!(files.contains("b.txt") && files.contains("c.txt") && files.contains("d.txt"));
        // The squash kept the message of the fourth in the body, the fixup dropped the third's.
        let body = repo.git(&["log", "-1", "--format=%B"]);
        assert!(body.contains("fourth") && !body.contains("third"));
    }

    #[test]
    fn a_folded_group_takes_the_new_message_of_the_commit_it_ends_on() {
        let repo = Repo::new();
        three(&repo);
        let first = shas(&repo)[3].clone();
        let plan = block(plan_from(repo.path(), Some(&first))).unwrap();
        let mut head = item(&plan, 0, PlanAction::Reword);
        head.message = "both of them".into();
        let new_plan = vec![
            head,
            item(&plan, 1, PlanAction::Fixup),
            item(&plan, 2, PlanAction::Pick),
        ];
        block(interactive_rebase(repo.path(), Some(&first), &new_plan)).unwrap();
        assert_eq!(history(&repo), vec!["fourth", "both of them", "first"]);
    }

    #[test]
    fn a_rebase_can_stop_at_a_commit_to_change_it_and_then_go_on() {
        let repo = Repo::new();
        three(&repo);
        let first = shas(&repo)[3].clone();
        let plan = block(plan_from(repo.path(), Some(&first))).unwrap();
        let new_plan = vec![
            item(&plan, 0, PlanAction::Edit),
            item(&plan, 1, PlanAction::Pick),
            item(&plan, 2, PlanAction::Pick),
        ];
        block(interactive_rebase(repo.path(), Some(&first), &new_plan)).unwrap();
        // Stopped after "second", the rest waiting.
        assert_eq!(
            block(crate::git::operation(repo.path())),
            Some(crate::git::Operation::Rebase)
        );
        assert_eq!(history(&repo), vec!["second", "first"]);
        repo.write("extra.txt", "x\n");
        repo.git(&["add", "extra.txt"]);
        repo.git(&["commit", "-q", "--amend", "--no-edit"]);
        block(crate::git::continue_operation(
            repo.path(),
            crate::git::Operation::Rebase,
        ))
        .unwrap();
        assert_eq!(history(&repo), vec!["fourth", "third", "second", "first"]);
        assert!(repo.path().join("extra.txt").exists());
    }

    #[test]
    fn a_plan_that_folds_the_first_commit_or_drops_everything_is_dealt_with() {
        let repo = Repo::new();
        three(&repo);
        let first = shas(&repo)[3].clone();
        let plan = block(plan_from(repo.path(), Some(&first))).unwrap();
        let bad = vec![
            item(&plan, 0, PlanAction::Squash),
            item(&plan, 1, PlanAction::Pick),
        ];
        assert!(check_plan(&bad).is_err());
        assert!(block(interactive_rebase(repo.path(), Some(&first), &bad)).is_err());
        assert_eq!(history(&repo).len(), 4, "nothing was touched");
        // A dropped one first is not "the first": folding after it is refused too.
        let also_bad = vec![
            item(&plan, 0, PlanAction::Drop),
            item(&plan, 1, PlanAction::Fixup),
        ];
        assert!(check_plan(&also_bad).is_err());
        // Everything dropped: the branch goes back to the base.
        let all_dropped: Vec<PlanItem> = plan
            .iter()
            .map(|i| PlanItem {
                action: PlanAction::Drop,
                ..i.clone()
            })
            .collect();
        block(interactive_rebase(repo.path(), Some(&first), &all_dropped)).unwrap();
        assert_eq!(history(&repo), vec!["first"]);
    }

    #[test]
    fn a_rebase_with_a_conflict_is_left_to_settle() {
        let repo = Repo::new();
        make_commit(&repo, &[("f.txt", "base\n")], "base");
        make_commit(&repo, &[("f.txt", "one\n")], "one");
        make_commit(&repo, &[("f.txt", "two\n")], "two");
        let base = shas(&repo)[2].clone();
        let plan = block(plan_from(repo.path(), Some(&base))).unwrap();
        // Replayed in the other order, they touch the same line.
        let swapped = vec![
            item(&plan, 1, PlanAction::Pick),
            item(&plan, 0, PlanAction::Pick),
        ];
        assert_eq!(
            block(interactive_rebase(repo.path(), Some(&base), &swapped)).unwrap_err(),
            crate::git::CONFLICT
        );
    }

    #[test]
    fn the_log_can_be_searched_by_message_author_content_file_and_date() {
        let repo = Repo::new();
        make_commit(&repo, &[("a.txt", "alpha\n")], "add the alpha file");
        repo.git(&[
            "commit",
            "-q",
            "--allow-empty",
            "--author",
            "Zed <z@example.com>",
            "-m",
            "an empty one by Zed",
        ]);
        make_commit(
            &repo,
            &[("b.txt", "beta needle beta\n")],
            "add the beta file",
        );
        let find = |mode, text: &str| -> Vec<String> {
            block(log_matching(
                repo.path(),
                "HEAD",
                100,
                None,
                Some((mode, text)),
            ))
            .unwrap()
            .into_iter()
            .map(|c| c.subject)
            .collect()
        };
        assert_eq!(
            find(SearchMode::Message, "ALPHA"),
            vec!["add the alpha file"]
        );
        assert_eq!(find(SearchMode::Author, "zed"), vec!["an empty one by Zed"]);
        assert_eq!(
            find(SearchMode::Content, "needle"),
            vec!["add the beta file"]
        );
        assert_eq!(find(SearchMode::File, "a.txt"), vec!["add the alpha file"]);
        assert_eq!(find(SearchMode::Since, "1 day ago").len(), 3);
        assert_eq!(find(SearchMode::Since, "2099-01-01").len(), 0);
        // Nothing typed: everything.
        assert_eq!(find(SearchMode::Message, "  ").len(), 3);
        // Text that git could read as an option stays text.
        assert!(find(SearchMode::Message, "--all").is_empty());
    }

    #[test]
    fn tags_are_made_pushed_and_removed_on_the_remote() {
        let repo = Repo::new();
        let remote = with_remote(&repo);
        make_commit(&repo, &[("a.txt", "a\n")], "first");
        make_commit(&repo, &[("a.txt", "a2\n")], "second");
        let old = shas(&repo)[1].clone();
        block(create_tag(repo.path(), "light", None, None)).unwrap();
        block(create_tag(
            repo.path(),
            "v0",
            Some(&old),
            Some("the first one"),
        ))
        .unwrap();
        assert_eq!(repo.git(&["cat-file", "-t", "light"]).trim(), "commit");
        assert_eq!(
            repo.git(&["cat-file", "-t", "v0"]).trim(),
            "tag",
            "annotated"
        );
        assert_eq!(repo.git(&["rev-list", "-n", "1", "v0"]).trim(), old);
        // A name that is an option is refused, not obeyed.
        assert!(block(create_tag(repo.path(), "--delete", None, None)).is_err());
        block(push_tag(repo.path(), "v0")).unwrap();
        let tags_there = |name: &str| {
            let out = std::process::Command::new("git")
                .args(["tag", "-l", name])
                .current_dir(remote.path())
                .output()
                .unwrap();
            !String::from_utf8_lossy(&out.stdout).trim().is_empty()
        };
        assert!(tags_there("v0") && !tags_there("light"));
        block(delete_remote_tag(repo.path(), "v0")).unwrap();
        assert!(!tags_there("v0"));
    }

    #[test]
    fn the_blame_of_an_older_revision_is_read() {
        let repo = Repo::new();
        make_commit(&repo, &[("f.txt", "one\ntwo\n")], "first");
        make_commit(&repo, &[("f.txt", "one\nTWO\nthree\n")], "second");
        let head = block(blame(repo.path(), "f.txt")).unwrap();
        assert_eq!(head.len(), 3);
        assert_eq!(head[1].summary, "second");
        let before = block(blame_at(repo.path(), Some("HEAD~1"), "f.txt")).unwrap();
        assert_eq!(before.len(), 2);
        assert!(before.iter().all(|l| l.summary == "first"));
        assert_eq!(
            block(file_at(repo.path(), "HEAD~1", "f.txt")).unwrap(),
            "one\ntwo\n"
        );
    }
}
