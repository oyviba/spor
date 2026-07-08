//! Remote host detection and PR/MR creation.
//!
//! Strategy: parse `git remote get-url origin` to figure out whose service
//! we're talking to, then either shell out to that host's CLI tool (`gh`,
//! `glab`) or construct a compare URL the user can open.

use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum Host {
    GitHub,
    GitLab,
    Bitbucket,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub host: Host,
    /// Web URL for the repo, e.g. https://github.com/owner/repo
    pub web_url: String,
    // owner/repo are parsed out but currently only consumed by tests; kept
    // because PR-title/description prefill will want them.
    #[cfg_attr(not(test), allow(dead_code))]
    pub owner: String,
    #[cfg_attr(not(test), allow(dead_code))]
    pub repo: String,
}

/// Inspect `origin` and figure out where it lives.
/// Handles both SSH (`git@github.com:owner/repo.git`) and HTTPS forms.
pub fn detect() -> Option<RemoteInfo> {
    let url = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;

    parse_url(&url)
}

/// Parse a git remote URL into its parts. Returns None if it doesn't look like
/// any host we know how to PR against.
fn parse_url(url: &str) -> Option<RemoteInfo> {
    // Normalize: SSH form `git@host:owner/repo(.git)` -> `https://host/owner/repo`
    let normalized = if let Some(rest) = url.strip_prefix("git@") {
        // rest = "github.com:owner/repo.git"
        let (host, path) = rest.split_once(':')?;
        format!("https://{host}/{path}")
    } else if url.starts_with("ssh://") {
        // ssh://git@host/owner/repo.git
        url.replace("ssh://git@", "https://")
    } else {
        url.to_string()
    };

    // Strip trailing .git
    let normalized = normalized.trim_end_matches(".git").trim_end_matches('/');

    let after_scheme = normalized
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(normalized);
    let host_part = after_scheme.split('/').next().unwrap_or("");

    // Host detection by substring on the host only — a repo *named*
    // "gitlab-mirror" on some other host must not match, but enterprise hosts
    // that include the canonical domain (github.mycorp.com) still should.
    let host = if host_part.contains("github") {
        Host::GitHub
    } else if host_part.contains("gitlab") {
        Host::GitLab
    } else if host_part.contains("bitbucket") {
        Host::Bitbucket
    } else {
        Host::Unknown
    };

    // Pull owner/repo from the last two path segments. This handles GitLab
    // subgroups by treating "everything before the last segment" as owner.
    let path = after_scheme.split_once('/').map(|(_, p)| p).unwrap_or("");
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let repo = parts.pop()?.to_string();
    let owner = parts.join("/");

    let web_url = format!(
        "{}://{}/{owner}/{repo}",
        if normalized.starts_with("http://") {
            "http"
        } else {
            "https"
        },
        after_scheme.split_once('/').map(|(h, _)| h).unwrap_or("")
    );

    Some(RemoteInfo {
        host,
        web_url,
        owner,
        repo,
    })
}

/// Compose a "compare" URL for opening a PR/MR in the browser.
/// `base` is usually `main` or `master`, `head` is the current branch.
pub fn compare_url(info: &RemoteInfo, base: &str, head: &str) -> String {
    match info.host {
        Host::GitHub => format!("{}/compare/{base}...{head}?expand=1", info.web_url),
        Host::GitLab => format!(
            "{}/-/merge_requests/new?merge_request%5Bsource_branch%5D={head}&merge_request%5Btarget_branch%5D={base}",
            info.web_url
        ),
        Host::Bitbucket => format!(
            "{}/pull-requests/new?source={head}&dest={base}",
            info.web_url
        ),
        Host::Unknown => info.web_url.clone(),
    }
}

/// Which CLI tool (if any) is installed for this host?
pub fn cli_tool(host: &Host) -> Option<&'static str> {
    let tool = match host {
        Host::GitHub => "gh",
        Host::GitLab => "glab",
        _ => return None,
    };
    Command::new("which")
        .arg(tool)
        .output()
        .ok()
        .filter(|o| o.status.success() && !o.stdout.is_empty())
        .map(|_| tool)
}

/// CI status for a PR's head commit, rolled up across all checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChecksState {
    None, // no checks configured
    Pending,
    Passing,
    Failing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReviewState {
    None, // no decision yet (or review not required)
    Approved,
    ChangesRequested,
}

/// An open PR, as much of it as the branch-tip badges need.
#[derive(Debug, Clone)]
pub struct PrInfo {
    pub number: u64,
    pub head_branch: String,
    pub draft: bool,
    pub review: ReviewState,
    pub checks: ChecksState,
}

/// gh's built-in jq flattens the PR list to one tab-separated line each:
///   number \t head branch \t draft|open \t review decision \t check1,check2,…
/// so we never have to parse JSON ourselves. Empty check conclusions mean
/// "still running" in gh's export, hence the PENDING mapping.
const PR_LIST_JQ: &str = r#".[] | [(.number|tostring), .headRefName, (if .isDraft then "draft" else "open" end), (.reviewDecision // ""), ([.statusCheckRollup[]? | ((.conclusion // .state // "") | if . == "" then "PENDING" else . end)] | join(","))] | @tsv"#;

/// List open PRs for the repo we're in. Slow (network) — call off the UI
/// thread. Returns None when the host isn't GitHub or `gh` isn't installed;
/// badges simply don't appear.
pub fn fetch_prs() -> Option<Vec<PrInfo>> {
    let info = detect()?;
    if info.host != Host::GitHub {
        return None;
    }
    cli_tool(&info.host)?;
    let out = Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "200",
            "--json",
            "number,headRefName,isDraft,reviewDecision,statusCheckRollup",
            "--jq",
            PR_LIST_JQ,
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(parse_pr_lines(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_pr_lines(out: &str) -> Vec<PrInfo> {
    out.lines().filter_map(parse_pr_line).collect()
}

fn parse_pr_line(line: &str) -> Option<PrInfo> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < 5 {
        return None;
    }
    Some(PrInfo {
        number: fields[0].parse().ok()?,
        head_branch: fields[1].to_string(),
        draft: fields[2] == "draft",
        review: match fields[3] {
            "APPROVED" => ReviewState::Approved,
            "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
            _ => ReviewState::None,
        },
        checks: rollup_checks(fields[4]),
    })
}

/// Fold per-check conclusions into one state. GitHub mixes two vocabularies
/// (CheckRun conclusions and commit-status states); any failure-ish word makes
/// the whole rollup Failing, otherwise any pending-ish word makes it Pending.
fn rollup_checks(list: &str) -> ChecksState {
    if list.is_empty() {
        return ChecksState::None;
    }
    let mut pending = false;
    for c in list.split(',') {
        match c {
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => return ChecksState::Failing,
            "PENDING" | "EXPECTED" | "QUEUED" | "IN_PROGRESS" | "WAITING" | "REQUESTED" => {
                pending = true
            }
            _ => {} // SUCCESS, NEUTRAL, SKIPPED
        }
    }
    if pending {
        ChecksState::Pending
    } else {
        ChecksState::Passing
    }
}

/// Build the argv for the PR-create command. Caller invokes this either
/// directly (auto mode with --fill) or after suspending the TUI (interactive).
pub fn pr_create_args(host: &Host, base: &str) -> Vec<String> {
    match host {
        Host::GitHub => vec!["pr".into(), "create".into(), "--base".into(), base.into()],
        Host::GitLab => vec![
            "mr".into(),
            "create".into(),
            "--target-branch".into(),
            base.into(),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_ssh() {
        let r = parse_url("git@github.com:oyvindandreassen/spor.git").unwrap();
        assert_eq!(r.host, Host::GitHub);
        assert_eq!(r.owner, "oyvindandreassen");
        assert_eq!(r.repo, "spor");
        assert_eq!(r.web_url, "https://github.com/oyvindandreassen/spor");
    }

    #[test]
    fn parse_github_https() {
        let r = parse_url("https://github.com/oyvindandreassen/spor.git").unwrap();
        assert_eq!(r.host, Host::GitHub);
        assert_eq!(r.repo, "spor");
    }

    #[test]
    fn parse_gitlab_subgroup() {
        let r = parse_url("git@gitlab.com:group/subgroup/proj.git").unwrap();
        assert_eq!(r.host, Host::GitLab);
        assert_eq!(r.owner, "group/subgroup");
        assert_eq!(r.repo, "proj");
    }

    #[test]
    fn host_matched_on_host_part_only() {
        // "gitlab" in the repo name must not override the actual host.
        let r = parse_url("https://example.com/team/gitlab-mirror.git").unwrap();
        assert_eq!(r.host, Host::Unknown);
        assert_eq!(r.repo, "gitlab-mirror");
    }

    #[test]
    fn enterprise_host_still_detected() {
        let r = parse_url("git@github.mycorp.com:team/proj.git").unwrap();
        assert_eq!(r.host, Host::GitHub);
    }

    #[test]
    fn parse_pr_line_full() {
        let pr = parse_pr_line("42\tfeat/login\topen\tAPPROVED\tSUCCESS,SKIPPED").unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.head_branch, "feat/login");
        assert!(!pr.draft);
        assert_eq!(pr.review, ReviewState::Approved);
        assert_eq!(pr.checks, ChecksState::Passing);
    }

    #[test]
    fn parse_pr_line_draft_no_review_no_checks() {
        let pr = parse_pr_line("7\twip\tdraft\t\t").unwrap();
        assert!(pr.draft);
        assert_eq!(pr.review, ReviewState::None);
        assert_eq!(pr.checks, ChecksState::None);
    }

    #[test]
    fn parse_pr_lines_skips_garbage() {
        let prs = parse_pr_lines("1\ta\topen\t\tSUCCESS\nnot a record\n\n2\tb\topen\t\t\n");
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[1].head_branch, "b");
    }

    #[test]
    fn rollup_failure_beats_pending() {
        assert_eq!(
            rollup_checks("SUCCESS,PENDING,FAILURE"),
            ChecksState::Failing
        );
        assert_eq!(rollup_checks("SUCCESS,IN_PROGRESS"), ChecksState::Pending);
        assert_eq!(rollup_checks("SUCCESS,NEUTRAL"), ChecksState::Passing);
        assert_eq!(rollup_checks(""), ChecksState::None);
    }

    #[test]
    fn review_changes_requested() {
        let pr = parse_pr_line("3\tfix/y\topen\tCHANGES_REQUESTED\tFAILURE").unwrap();
        assert_eq!(pr.review, ReviewState::ChangesRequested);
        assert_eq!(pr.checks, ChecksState::Failing);
    }

    #[test]
    fn compare_url_github() {
        let r = parse_url("git@github.com:foo/bar.git").unwrap();
        assert_eq!(
            compare_url(&r, "main", "feat/x"),
            "https://github.com/foo/bar/compare/main...feat/x?expand=1"
        );
    }
}
