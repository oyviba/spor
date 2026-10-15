//! Workspaces, like Slack's: named groups of repositories ("Work", "Side
//! projects") with their own repository list and last-open repository.
//! The rail on the left switches between them.
//!
//! Stored as plain text in the config directory, one tab-separated record
//! per line (`<TAB>` below):
//!
//! ```text
//! active<TAB>1
//! workspace<TAB>Work<TAB>2
//! last<TAB>/Users/me/work/api
//! repo<TAB>/Users/me/work/api
//! repo<TAB>/Users/me/work/web
//! workspace<TAB>Side projects<TAB>5
//! repo<TAB>/Users/me/code/spor
//! ```

use super::support;
use std::path::{Path, PathBuf};

const SETTING: &str = "workspaces";

/// How many colors `color` cycles through; the rail maps each to a hue.
pub const COLORS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub name: String,
    pub color: usize,
    pub repos: Vec<PathBuf>,
    /// The repository open when this workspace was last left.
    pub last: Option<PathBuf>,
}

impl Workspace {
    pub fn new(name: &str, color: usize) -> Self {
        Self {
            name: name.to_string(),
            color: color % COLORS,
            repos: Vec::new(),
            last: None,
        }
    }

    /// "Side projects" → "SP", "work" → "W".
    pub fn initials(&self) -> String {
        let words: Vec<&str> = self.name.split_whitespace().collect();
        let letters: String = match words.as_slice() {
            [] => "?".into(),
            [one] => one.chars().take(1).collect(),
            [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
        };
        letters.to_uppercase()
    }

    /// The repository to show when switching to this workspace.
    pub fn landing(&self) -> Option<&PathBuf> {
        self.last
            .as_ref()
            .filter(|p| self.repos.contains(p))
            .or(self.repos.first())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Workspaces {
    pub list: Vec<Workspace>,
    pub active: usize,
}

impl Workspaces {
    /// Read the saved workspaces. The first run makes a single "Personal"
    /// workspace holding the recently opened repositories.
    pub fn load() -> Self {
        support::read_setting(SETTING)
            .and_then(|text| Self::parse(&text))
            .unwrap_or_else(|| {
                let mut personal = Workspace::new("Personal", 0);
                personal.repos = support::recent_repos();
                personal.repos.sort();
                Self {
                    list: vec![personal],
                    active: 0,
                }
            })
    }

    pub fn save(&self) {
        support::write_setting(SETTING, &self.serialize());
    }

    fn parse(text: &str) -> Option<Self> {
        let mut list: Vec<Workspace> = Vec::new();
        let mut active = 0;
        for line in text.lines() {
            let mut fields = line.split('\t');
            match (fields.next(), fields.next()) {
                (Some("active"), Some(n)) => active = n.parse().unwrap_or(0),
                (Some("workspace"), Some(name)) => {
                    let color = fields.next().and_then(|c| c.parse().ok());
                    list.push(Workspace::new(name, color.unwrap_or(list.len())));
                }
                (Some("repo"), Some(path)) => list.last_mut()?.repos.push(path.into()),
                (Some("last"), Some(path)) => list.last_mut()?.last = Some(path.into()),
                _ => {}
            }
        }
        if list.is_empty() {
            return None;
        }
        active = active.min(list.len() - 1);
        Some(Self { list, active })
    }

    fn serialize(&self) -> String {
        let mut out = format!("active\t{}\n", self.active);
        for ws in &self.list {
            out += &format!("workspace\t{}\t{}\n", ws.name, ws.color);
            if let Some(last) = &ws.last {
                out += &format!("last\t{}\n", last.display());
            }
            for repo in &ws.repos {
                out += &format!("repo\t{}\n", repo.display());
            }
        }
        out
    }

    pub fn current(&self) -> &Workspace {
        &self.list[self.active]
    }

    /// The workspace a repository belongs to.
    pub fn find(&self, root: &Path) -> Option<usize> {
        self.list
            .iter()
            .position(|ws| ws.repos.iter().any(|p| p == root))
    }

    /// Record `root` as open: it stays in the workspace it belongs to (which
    /// becomes active) or joins the active one. Returns the workspace index.
    pub fn opened(&mut self, root: &Path) -> usize {
        let ix = match self.find(root) {
            Some(ix) => ix,
            None => {
                let ws = &mut self.list[self.active];
                ws.repos.push(root.to_path_buf());
                ws.repos.sort();
                self.active
            }
        };
        self.active = ix;
        self.list[ix].last = Some(root.to_path_buf());
        ix
    }

    /// Move a repository to another workspace.
    pub fn move_repo(&mut self, root: &Path, to: usize) {
        if to >= self.list.len() {
            return;
        }
        self.remove_repo(root);
        let ws = &mut self.list[to];
        ws.repos.push(root.to_path_buf());
        ws.repos.sort();
    }

    /// Take a repository out of its workspace (it stays on disk).
    pub fn remove_repo(&mut self, root: &Path) {
        for ws in &mut self.list {
            ws.repos.retain(|p| p != root);
            if ws.last.as_deref() == Some(root) {
                ws.last = None;
            }
        }
    }

    /// Add a workspace with the next unused color; returns its index.
    pub fn create(&mut self, name: &str) -> usize {
        let color = (0..COLORS)
            .find(|c| !self.list.iter().any(|ws| ws.color == *c))
            .unwrap_or(self.list.len() % COLORS);
        self.list.push(Workspace::new(name, color));
        self.list.len() - 1
    }

    /// Delete a workspace. The last one can't be deleted.
    pub fn delete(&mut self, ix: usize) -> bool {
        if self.list.len() <= 1 || ix >= self.list.len() {
            return false;
        }
        self.list.remove(ix);
        if self.active > ix || self.active >= self.list.len() {
            self.active = self.active.saturating_sub(1);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Workspaces {
        let mut work = Workspace::new("Work", 2);
        work.repos = vec!["/w/api".into(), "/w/web".into()];
        work.last = Some("/w/web".into());
        let mut side = Workspace::new("Side projects", 5);
        side.repos = vec!["/c/spor".into()];
        Workspaces {
            list: vec![work, side],
            active: 1,
        }
    }

    #[test]
    fn round_trips_through_text() {
        let ws = sample();
        assert_eq!(Workspaces::parse(&ws.serialize()), Some(ws));
    }

    #[test]
    fn parse_rejects_empty_and_clamps_active() {
        assert_eq!(Workspaces::parse(""), None);
        let ws = Workspaces::parse("active\t9\nworkspace\tSolo\t1\n").unwrap();
        assert_eq!(ws.active, 0);
        assert_eq!(ws.current().name, "Solo");
    }

    #[test]
    fn initials() {
        assert_eq!(Workspace::new("Side projects", 0).initials(), "SP");
        assert_eq!(Workspace::new("work", 0).initials(), "W");
        assert_eq!(Workspace::new("a b c", 0).initials(), "AC");
        assert_eq!(Workspace::new("  ", 0).initials(), "?");
    }

    #[test]
    fn opening_a_known_repo_switches_to_its_workspace() {
        let mut ws = sample();
        assert_eq!(ws.opened(Path::new("/w/api")), 0);
        assert_eq!(ws.active, 0);
        assert_eq!(ws.list[0].last, Some("/w/api".into()));
    }

    #[test]
    fn opening_a_new_repo_joins_the_active_workspace() {
        let mut ws = sample();
        assert_eq!(ws.opened(Path::new("/c/blog")), 1);
        assert_eq!(
            ws.list[1].repos,
            vec![PathBuf::from("/c/blog"), "/c/spor".into()]
        );
    }

    #[test]
    fn landing_prefers_last_then_first() {
        let mut ws = sample();
        assert_eq!(ws.list[0].landing(), Some(&"/w/web".into()));
        ws.remove_repo(Path::new("/w/web"));
        assert_eq!(ws.list[0].landing(), Some(&"/w/api".into()));
        assert_eq!(Workspace::new("Empty", 0).landing(), None);
    }

    #[test]
    fn move_and_delete() {
        let mut ws = sample();
        ws.move_repo(Path::new("/c/spor"), 0);
        assert!(ws.list[1].repos.is_empty());
        assert_eq!(ws.find(Path::new("/c/spor")), Some(0));
        let new = ws.create("Client");
        assert_eq!(ws.list[new].color, 0);
        assert!(ws.delete(0));
        assert_eq!(ws.active, 0);
        assert_eq!(ws.current().name, "Side projects");
        assert!(ws.delete(1));
        assert!(!ws.delete(0));
    }
}
