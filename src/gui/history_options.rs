//! How the History graph looks: row height, lane width, which columns and
//! refs are shown. Set from the options menu in the History header and
//! stored as `key<TAB>value` lines in the config directory.

use super::support;

const SETTING: &str = "history";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Density {
    Compact,
    Normal,
    Comfortable,
}

impl Density {
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::Compact, "Compact"),
        (Self::Normal, "Normal"),
        (Self::Comfortable, "Comfortable"),
    ];

    pub fn row_height(self) -> f32 {
        match self {
            Self::Compact => 22.,
            Self::Normal => 26.,
            Self::Comfortable => 32.,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Normal => "normal",
            Self::Comfortable => "comfortable",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LaneWidth {
    Narrow,
    Normal,
    Wide,
}

impl LaneWidth {
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::Narrow, "Narrow"),
        (Self::Normal, "Normal"),
        (Self::Wide, "Wide"),
    ];

    pub fn px(self) -> f32 {
        match self {
            Self::Narrow => 9.,
            Self::Normal => 12.,
            Self::Wide => 16.,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Normal => "normal",
            Self::Wide => "wide",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HistoryOptions {
    pub density: Density,
    pub lane_width: LaneWidth,
    /// Branches and tags in their own column rather than before the subject.
    pub branch_column: bool,
    pub author: bool,
    pub date: bool,
    pub sha: bool,
    /// Labels for branches that exist only on a remote.
    pub remote_branches: bool,
    pub tags: bool,
    /// Draw the default branch's lane thicker.
    pub bold_default: bool,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            density: Density::Normal,
            lane_width: LaneWidth::Normal,
            branch_column: true,
            author: true,
            date: true,
            sha: true,
            remote_branches: true,
            tags: true,
            bold_default: true,
        }
    }
}

impl HistoryOptions {
    pub fn saved() -> Self {
        Self::parse(&support::read_setting(SETTING).unwrap_or_default())
    }

    pub fn save(&self) {
        support::write_setting(SETTING, &self.serialize());
    }

    fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('\t') else {
                continue;
            };
            let on = value != "0";
            match key {
                "density" => {
                    if let Some((d, _)) = Density::ALL.iter().find(|(d, _)| d.key() == value) {
                        o.density = *d;
                    }
                }
                "lanes" => {
                    if let Some((w, _)) = LaneWidth::ALL.iter().find(|(w, _)| w.key() == value) {
                        o.lane_width = *w;
                    }
                }
                "branch-column" => o.branch_column = on,
                "author" => o.author = on,
                "date" => o.date = on,
                "sha" => o.sha = on,
                "remote-branches" => o.remote_branches = on,
                "tags" => o.tags = on,
                "bold-default" => o.bold_default = on,
                _ => {}
            }
        }
        o
    }

    fn serialize(&self) -> String {
        let flag = |b: bool| if b { "1" } else { "0" };
        format!(
            "density\t{}\nlanes\t{}\nbranch-column\t{}\nauthor\t{}\ndate\t{}\nsha\t{}\n\
             remote-branches\t{}\ntags\t{}\nbold-default\t{}\n",
            self.density.key(),
            self.lane_width.key(),
            flag(self.branch_column),
            flag(self.author),
            flag(self.date),
            flag(self.sha),
            flag(self.remote_branches),
            flag(self.tags),
            flag(self.bold_default),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_text() {
        let o = HistoryOptions {
            density: Density::Compact,
            lane_width: LaneWidth::Wide,
            branch_column: false,
            author: true,
            date: false,
            sha: true,
            remote_branches: false,
            tags: true,
            bold_default: false,
        };
        assert_eq!(HistoryOptions::parse(&o.serialize()), o);
    }

    #[test]
    fn unknown_or_missing_values_keep_defaults() {
        let o = HistoryOptions::parse("density\thuge\nnonsense\nauthor\t0\n");
        assert_eq!(o.density, Density::Normal);
        assert!(!o.author);
        assert!(o.sha);
    }
}
