//! Asset source: the kit's default icons plus the few extra Lucide icons a
//! git client needs, vendored in `assets/icons` and embedded at build time.
//! (`icon` also names some of the defaults, like `circle-check`.)

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(
            (
                concat!("icons/", $name, ".svg"),
                include_bytes!(concat!("../../assets/icons/", $name, ".svg")) as &[u8],
            ),
        )*]
    };
}

static EXTRA_ICONS: &[(&str, &[u8])] = icons![
    "archive",
    "arrow-up-down",
    "circle-dashed",
    "circle-dot",
    "circle-minus",
    "circle-play",
    "circle-slash",
    "clipboard-copy",
    "clock",
    "cloud",
    "ellipsis",
    "file-diff",
    "folder-git-2",
    "git-branch",
    "git-commit-horizontal",
    "git-compare",
    "git-merge",
    "git-pull-request",
    "git-pull-request-closed",
    "git-pull-request-draft",
    "laptop",
    "list-checks",
    "message-square",
    "rotate-ccw",
    "settings-2",
    "tag",
];

/// Icon paths for `Icon::empty().path(…)`.
pub mod icon {
    pub const ARCHIVE: &str = "icons/archive.svg";
    pub const CLOUD: &str = "icons/cloud.svg";
    pub const LAPTOP: &str = "icons/laptop.svg";
    pub const SETTINGS: &str = "icons/settings-2.svg";
    pub const GIT_MERGE: &str = "icons/git-merge.svg";
    pub const CLOCK: &str = "icons/clock.svg";
    pub const COPY: &str = "icons/clipboard-copy.svg";
    pub const ELLIPSIS: &str = "icons/ellipsis.svg";
    pub const FILE_DIFF: &str = "icons/file-diff.svg";
    pub const FOLDER_GIT: &str = "icons/folder-git-2.svg";
    pub const GIT_BRANCH: &str = "icons/git-branch.svg";
    pub const GIT_COMMIT: &str = "icons/git-commit-horizontal.svg";
    pub const GIT_COMPARE: &str = "icons/git-compare.svg";
    pub const GIT_PULL_REQUEST: &str = "icons/git-pull-request.svg";
    pub const LIST_CHECKS: &str = "icons/list-checks.svg";
    pub const SYNC: &str = "icons/arrow-up-down.svg";
    pub const TAG: &str = "icons/tag.svg";
    pub const ACTIONS: &str = "icons/circle-play.svg";
    pub const CHECK_PASSED: &str = "icons/circle-check.svg";
    pub const CHECK_FAILED: &str = "icons/circle-x.svg";
    pub const CHECK_RUNNING: &str = "icons/circle-dot.svg";
    pub const CHECK_QUEUED: &str = "icons/circle-dashed.svg";
    pub const CHECK_SKIPPED: &str = "icons/circle-minus.svg";
    pub const CHECK_CANCELLED: &str = "icons/circle-slash.svg";
    pub const EXTERNAL_LINK: &str = "icons/external-link.svg";
    pub const REFRESH: &str = "icons/refresh-cw.svg";
    pub const GITHUB: &str = "icons/github.svg";
    pub const COMMENT: &str = "icons/message-square.svg";
    pub const RERUN: &str = "icons/rotate-ccw.svg";
    pub const PR_CLOSED: &str = "icons/git-pull-request-closed.svg";
    pub const PR_DRAFT: &str = "icons/git-pull-request-draft.svg";
}

pub struct Assets;

const LOGO: &[u8] = include_bytes!("../../assets/icon.png");

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == "images/spor.png" {
            return Ok(Some(Cow::Borrowed(LOGO)));
        }
        if let Some((_, bytes)) = EXTRA_ICONS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut list = gpui_kit::assets::Assets.list(path)?;
        list.extend(
            EXTRA_ICONS
                .iter()
                .filter(|(p, _)| p.starts_with(path))
                .map(|(p, _)| SharedString::from(*p)),
        );
        list.sort();
        list.dedup();
        Ok(list)
    }
}
