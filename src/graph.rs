use crate::color::branch_family;
use crate::git::Commit;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct GraphRow {
    pub commit: Commit,
    pub lane: usize,                        // which column this commit sits in
    pub lanes_before: Vec<Option<String>>,  // active lane contents before this commit
    pub lanes_after: Vec<Option<String>>,   // active lane contents after this commit
    pub branch_family: String,              // prefix like "feat" or "bug" for coloring
    pub lane_families: Vec<Option<String>>, // family per lane (for coloring │ connectors)
}

/// Assign each commit to a lane. Lane 0 is reserved for the main branch's
/// first-parent chain so it always renders as the leftmost column.
///
/// Algorithm:
///   walk commits in order (already topo/date-sorted by git log)
///   maintain `lanes: Vec<Option<hash>>` — each slot is the hash of the commit
///   we're waiting for in that lane
///   for each commit:
///     - if it's on the main chain, force lane 0
///     - else find the lane whose waiting hash matches this commit
///     - if no lane matches, open a new one (first free slot, min index >= 1)
///     - replace that lane's waiting hash with the first parent
///     - extra parents (merges) claim additional new lanes
///     - if no parents, the lane empties
pub fn assign_lanes(
    commits: &[Commit],
    main_chain: &HashSet<String>,
    remotes: &[String],
) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    // Reserve lane 0 for main even if main's tip isn't the newest commit.
    lanes.push(None);

    let mut rows = Vec::with_capacity(commits.len());

    // Track which branch family "owns" each lane, so children inherit color.
    let mut lane_family: HashMap<usize, String> = HashMap::new();

    // Infer family from the first branch ref on the commit (tags don't count).
    let family_of = |commit: &Commit| -> Option<String> {
        commit
            .refs
            .iter()
            .find(|r| !r.starts_with("tag:"))
            .map(|r| branch_family(r, remotes).to_string())
    };

    for commit in commits {
        let lanes_before = lanes.clone();

        // Find this commit's lane.
        let is_main = main_chain.contains(&commit.hash);
        let lane = if is_main {
            // Main always goes to lane 0. If lane 0 is occupied by something
            // else waiting, bump that occupant to a new lane.
            if let Some(Some(other)) = lanes.first().cloned() {
                if other != commit.hash {
                    let new_slot = first_free(&lanes, 1);
                    ensure_len(&mut lanes, new_slot + 1);
                    lanes[new_slot] = Some(other);
                    if let Some(fam) = lane_family.remove(&0) {
                        lane_family.insert(new_slot, fam);
                    }
                }
            }
            0
        } else {
            // Find a lane waiting for this commit.
            lanes
                .iter()
                .position(|l| l.as_deref() == Some(commit.hash.as_str()))
                .unwrap_or_else(|| {
                    // New branch tip — open a lane (skip 0, that's main's).
                    let slot = first_free(&lanes, 1);
                    ensure_len(&mut lanes, slot + 1);
                    slot
                })
        };

        // Establish family for this lane if we don't have one.
        if let std::collections::hash_map::Entry::Vacant(slot) = lane_family.entry(lane) {
            if let Some(fam) = family_of(commit) {
                slot.insert(fam);
            }
        }
        let branch_family = lane_family
            .get(&lane)
            .cloned()
            .unwrap_or_else(|| "_".to_string());

        // Update the lane for our parents.
        match commit.parents.len() {
            0 => {
                // Root commit — lane empties.
                if lane < lanes.len() {
                    lanes[lane] = None;
                }
                lane_family.remove(&lane);
            }
            _ => {
                // First parent continues in this lane.
                ensure_len(&mut lanes, lane + 1);
                lanes[lane] = Some(commit.parents[0].clone());

                // Merge parents: each extra parent claims a lane.
                // If one of them is already being waited for elsewhere, reuse.
                for parent in &commit.parents[1..] {
                    let existing = lanes
                        .iter()
                        .position(|l| l.as_deref() == Some(parent.as_str()));
                    if existing.is_none() {
                        let slot = first_free(&lanes, 1);
                        ensure_len(&mut lanes, slot + 1);
                        lanes[slot] = Some(parent.clone());
                        // Inherit family from the current commit's lane — looks nicer
                        // since the merge "pulls in" that branch.
                        lane_family.insert(slot, branch_family.clone());
                    }
                }
            }
        }

        // Compact trailing None lanes to keep the graph narrow.
        while lanes.len() > 1 && lanes.last() == Some(&None) {
            lane_family.remove(&(lanes.len() - 1));
            lanes.pop();
        }

        let lanes_after = lanes.clone();

        let num_lanes = lanes_after.len().max(lane + 1);
        let lane_families_snap: Vec<Option<String>> = (0..num_lanes)
            .map(|i| lane_family.get(&i).cloned())
            .collect();

        rows.push(GraphRow {
            commit: commit.clone(),
            lane,
            lanes_before,
            lanes_after,
            branch_family,
            lane_families: lane_families_snap,
        });
    }

    rows
}

fn first_free(lanes: &[Option<String>], start: usize) -> usize {
    lanes
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, l)| l.is_none())
        .map(|(i, _)| i)
        .unwrap_or_else(|| lanes.len().max(start))
}

fn ensure_len(lanes: &mut Vec<Option<String>>, n: usize) {
    while lanes.len() < n {
        lanes.push(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, parents: &[&str], refs: &[&str]) -> Commit {
        Commit {
            hash: hash.to_string(),
            short: hash.chars().take(7).collect(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            refs: refs.iter().map(|s| s.to_string()).collect(),
            head_ref: None,
            subject: format!("commit {hash}"),
            author: "test".to_string(),
            timestamp: 0,
        }
    }

    fn no_remotes() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn linear_main_chain_stays_in_lane_zero() {
        let commits = vec![
            commit("c", &["b"], &["main"]),
            commit("b", &["a"], &[]),
            commit("a", &[], &[]),
        ];
        let chain: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let rows = assign_lanes(&commits, &chain, &no_remotes());
        assert!(rows.iter().all(|r| r.lane == 0));
    }

    #[test]
    fn feature_branch_gets_its_own_lane() {
        // main: a <- b, feature: a <- f (f newest)
        let commits = vec![
            commit("f", &["a"], &["feat/x"]),
            commit("b", &["a"], &["main"]),
            commit("a", &[], &[]),
        ];
        let chain: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let rows = assign_lanes(&commits, &chain, &no_remotes());
        assert_ne!(rows[0].lane, 0, "feature tip must not sit in main's lane");
        assert_eq!(rows[1].lane, 0, "main tip pinned to lane 0");
        assert_eq!(rows[2].lane, 0, "shared root belongs to main's chain");
        assert_eq!(rows[0].branch_family, "feat");
    }

    #[test]
    fn merge_commit_opens_lane_for_second_parent() {
        // m is a merge of b (main) and f (feature).
        let commits = vec![
            commit("m", &["b", "f"], &["main"]),
            commit("b", &["a"], &[]),
            commit("f", &["a"], &["feat/x"]),
            commit("a", &[], &[]),
        ];
        let chain: HashSet<String> = ["a", "b", "m"].iter().map(|s| s.to_string()).collect();
        let rows = assign_lanes(&commits, &chain, &no_remotes());
        assert_eq!(rows[0].lane, 0);
        // The merge row must be waiting for both parents afterwards.
        let waiting: Vec<&str> = rows[0]
            .lanes_after
            .iter()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(waiting.contains(&"b"));
        assert!(waiting.contains(&"f"));
        // The feature commit lands in the lane that was waiting for it.
        assert_ne!(rows[2].lane, 0);
    }

    #[test]
    fn root_commit_frees_its_lane() {
        let commits = vec![commit("b", &["a"], &["main"]), commit("a", &[], &[])];
        let chain: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let rows = assign_lanes(&commits, &chain, &no_remotes());
        assert!(rows[1].lanes_after.iter().all(|l| l.is_none()));
    }

    #[test]
    fn local_branch_family_uses_prefix() {
        let commits = vec![commit("f", &[], &["feat/login"])];
        let rows = assign_lanes(&commits, &HashSet::new(), &no_remotes());
        assert_eq!(rows[0].branch_family, "feat");
    }

    #[test]
    fn remote_branch_family_strips_remote_only() {
        let commits = vec![commit("f", &[], &["origin/feat/login"])];
        let remotes = vec!["origin".to_string()];
        let rows = assign_lanes(&commits, &HashSet::new(), &remotes);
        assert_eq!(rows[0].branch_family, "feat");
    }
}
