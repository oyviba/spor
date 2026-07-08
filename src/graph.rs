use crate::git::Commit;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct GraphRow {
    pub commit: Commit,
    pub lane: usize,                       // which column this commit sits in
    pub lanes_before: Vec<Option<String>>, // active lane contents before this commit
    pub lanes_after: Vec<Option<String>>,  // active lane contents after this commit
    pub branch_family: String,             // prefix like "feat" or "bug" for coloring
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
pub fn assign_lanes(commits: &[Commit], main_chain: &HashSet<String>) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    // Reserve lane 0 for main even if main's tip isn't the newest commit.
    lanes.push(None);

    let mut rows = Vec::with_capacity(commits.len());

    // Track which branch family "owns" each lane, so children inherit color.
    let mut lane_family: HashMap<usize, String> = HashMap::new();

    // Infer family from refs on the commit.
    fn family_of(commit: &Commit) -> Option<String> {
        for r in &commit.refs {
            if r.starts_with("tag:") {
                continue;
            }
            // Strip remote prefix like "origin/"
            let name = r.splitn(2, '/').nth(1).unwrap_or(r);
            if let Some(prefix) = name.split('/').next() {
                if name.contains('/') {
                    return Some(prefix.to_string());
                }
            }
            return Some(name.to_string()); // branch with no prefix: use full name
        }
        None
    }

    for commit in commits {
        let lanes_before = lanes.clone();

        // Find this commit's lane.
        let is_main = main_chain.contains(&commit.hash);
        let lane = if is_main {
            // Main always goes to lane 0. If lane 0 is occupied by something
            // else waiting, bump that occupant to a new lane.
            if let Some(Some(other)) = lanes.get(0).cloned() {
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
        if !lane_family.contains_key(&lane) {
            if let Some(fam) = family_of(commit) {
                lane_family.insert(lane, fam);
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
                    let existing = lanes.iter().position(|l| l.as_deref() == Some(parent.as_str()));
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
        let lane_families_snap: Vec<Option<String>> =
            (0..num_lanes).map(|i| lane_family.get(&i).cloned()).collect();

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
    for i in start..lanes.len() {
        if lanes[i].is_none() {
            return i;
        }
    }
    lanes.len().max(start)
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
            short: hash.to_string(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            refs: refs.iter().map(|s| s.to_string()).collect(),
            head_ref: None,
            subject: format!("commit {hash}"),
            author: "test".to_string(),
            timestamp: 0,
        }
    }

    fn chain(hashes: &[&str]) -> HashSet<String> {
        hashes.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn linear_main_history_stays_in_lane_zero() {
        // Newest first, like git log output.
        let commits = vec![
            commit("c", &["b"], &["main"]),
            commit("b", &["a"], &[]),
            commit("a", &[], &[]),
        ];
        let rows = assign_lanes(&commits, &chain(&["a", "b", "c"]));
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.lane == 0));
        assert_eq!(rows[0].branch_family, "main");
    }

    #[test]
    fn feature_branch_gets_its_own_lane() {
        let commits = vec![
            commit("f1", &["m1"], &["origin/feat/login"]),
            commit("m1", &["m0"], &["main"]),
            commit("m0", &[], &[]),
        ];
        let rows = assign_lanes(&commits, &chain(&["m0", "m1"]));
        assert_eq!(rows[0].lane, 1, "feature tip must not take main's lane");
        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[2].lane, 0);
        // Family comes from the ref with the remote prefix stripped.
        assert_eq!(rows[0].branch_family, "feat");
    }

    #[test]
    fn merge_commit_opens_lane_for_second_parent() {
        let commits = vec![
            commit("m2", &["m1", "f1"], &["main"]),
            commit("f1", &["m0"], &["origin/feat/x"]),
            commit("m1", &["m0"], &[]),
            commit("m0", &[], &[]),
        ];
        let rows = assign_lanes(&commits, &chain(&["m0", "m1", "m2"]));
        assert_eq!(rows[0].lane, 0); // merge sits on main
        assert_eq!(rows[1].lane, 1); // merged branch got the lane opened by the merge
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[3].lane, 0);
        // After the merge row, lane 1 waits for the second parent.
        assert_eq!(rows[0].lanes_after.get(1), Some(&Some("f1".to_string())));
    }

    #[test]
    fn main_bumps_squatter_out_of_lane_zero() {
        // A feature commit is newest; its parent chain would claim lane 0's
        // wait slot only if lanes were assigned naively. Main must stay at 0.
        let commits = vec![
            commit("f2", &["f1"], &["origin/feat/x"]),
            commit("m1", &["m0"], &["main"]),
            commit("f1", &["m0"], &[]),
            commit("m0", &[], &[]),
        ];
        let rows = assign_lanes(&commits, &chain(&["m0", "m1"]));
        let lane_of = |h: &str| rows.iter().find(|r| r.commit.hash == h).unwrap().lane;
        assert_eq!(lane_of("m1"), 0);
        assert_eq!(lane_of("m0"), 0);
        assert_ne!(lane_of("f2"), 0);
        assert_eq!(lane_of("f1"), lane_of("f2"), "branch stays in one lane");
    }

    #[test]
    fn root_commit_frees_its_lane() {
        let commits = vec![commit("a", &[], &["main"])];
        let rows = assign_lanes(&commits, &chain(&["a"]));
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].lanes_after, vec![None]);
    }

    #[test]
    fn unlabeled_lane_falls_back_to_orphan_family() {
        let commits = vec![commit("a", &[], &[])];
        let rows = assign_lanes(&commits, &chain(&["a"]));
        assert_eq!(rows[0].branch_family, "_");
    }
}
