use crate::git::Commit;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct GraphRow {
    pub commit: Commit,
    pub lane: usize,            // which column this commit sits in
    pub lanes_before: Vec<Option<String>>, // active lane contents before this commit
    pub lanes_after: Vec<Option<String>>,  // active lane contents after this commit
    pub branch_family: String,  // prefix like "feat" or "bug" for coloring
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

        rows.push(GraphRow {
            commit: commit.clone(),
            lane,
            lanes_before,
            lanes_after,
            branch_family,
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
