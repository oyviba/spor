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
    pub owner: String,
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

    // host detection by substring — handles enterprise hosts that include the
    // canonical domain (e.g. github.mycorp.com would still match GitHub).
    let host = if normalized.contains("github") {
        Host::GitHub
    } else if normalized.contains("gitlab") {
        Host::GitLab
    } else if normalized.contains("bitbucket") {
        Host::Bitbucket
    } else {
        Host::Unknown
    };

    // Pull owner/repo from the last two path segments. This handles GitLab
    // subgroups by treating "everything before the last segment" as owner.
    let after_scheme = normalized.split_once("://").map(|(_, r)| r).unwrap_or(normalized);
    let path = after_scheme.split_once('/').map(|(_, p)| p).unwrap_or("");
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let repo = parts.pop()?.to_string();
    let owner = parts.join("/");

    let web_url = format!(
        "{}://{}/{owner}/{repo}",
        if normalized.starts_with("http://") { "http" } else { "https" },
        after_scheme.split_once('/').map(|(h, _)| h).unwrap_or("")
    );

    Some(RemoteInfo { host, web_url, owner, repo })
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
    fn compare_url_github() {
        let r = parse_url("git@github.com:foo/bar.git").unwrap();
        assert_eq!(
            compare_url(&r, "main", "feat/x"),
            "https://github.com/foo/bar/compare/main...feat/x?expand=1"
        );
    }
}
