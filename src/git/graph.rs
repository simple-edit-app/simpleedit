//! The commit graph: which lane each commit sits in and which lines join the
//! rows, computed from the parents alone.

/// A commit, as the log lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Seconds since the epoch.
    pub time: i64,
    pub subject: String,
    /// Branches and tags pointing at it.
    pub refs: Vec<String>,
}

/// What is drawn on a row: a dot in `lane`, lines from the row above that
/// end in it, lines that go on to the row below, and lanes that just cross.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    /// Lanes that cross the row untouched: (lane, color).
    pub through: Vec<(usize, usize)>,
    /// Lines from the top of the row (lane) to the dot.
    pub from_above: Vec<(usize, usize)>,
    /// Lines from the dot to the bottom of the row (lane).
    pub to_below: Vec<(usize, usize)>,
}

/// How many colors the lanes cycle through.
pub const COLORS: usize = 8;

/// The graph rows of a list of commits (newest first, parents after their
/// children, as `git log --topo-order` gives them).
pub fn layout(commits: &[Commit]) -> Vec<GraphRow> {
    // For each lane: the commit it waits for, and its color.
    let mut lanes: Vec<Option<(String, usize)>> = Vec::new();
    let mut next_color = 0usize;
    let mut take_color = || {
        let c = next_color % COLORS;
        next_color += 1;
        c
    };
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        let waiting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_ref().is_some_and(|(sha, _)| *sha == commit.sha))
            .map(|(i, _)| i)
            .collect();
        let lane = match waiting.first() {
            Some(&i) => i,
            None => free_lane(&mut lanes),
        };
        let color = match waiting.first() {
            Some(&i) => lanes[i].as_ref().map_or(0, |(_, c)| *c),
            None => take_color(),
        };
        let from_above: Vec<(usize, usize)> = waiting
            .iter()
            .filter_map(|&i| lanes[i].as_ref().map(|(_, c)| (i, *c)))
            .collect();
        // The lanes that wait for this commit end here.
        for &i in &waiting {
            lanes[i] = None;
        }
        let mut to_below = Vec::new();
        for (n, parent) in commit.parents.iter().enumerate() {
            let existing = lanes
                .iter()
                .position(|l| l.as_ref().is_some_and(|(sha, _)| sha == parent));
            match existing {
                Some(i) => {
                    let c = lanes[i].as_ref().map_or(color, |(_, c)| *c);
                    to_below.push((i, c));
                }
                None => {
                    // The first parent goes on in the commit's own lane.
                    let target = if n == 0 {
                        if lane >= lanes.len() {
                            lanes.resize(lane + 1, None);
                        }
                        lane
                    } else {
                        free_lane(&mut lanes)
                    };
                    let c = if n == 0 { color } else { take_color() };
                    lanes[target] = Some((parent.clone(), c));
                    to_below.push((target, c));
                }
            }
        }
        // Lanes still waiting cross this row.
        let through = lanes
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                !from_above.iter().any(|(l, _)| l == i)
                    && !to_below.iter().any(|(l, _)| l == i)
                    && *i != lane
            })
            .filter_map(|(i, l)| l.as_ref().map(|(_, c)| (i, *c)))
            .collect();
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(GraphRow {
            lane,
            color,
            through,
            from_above,
            to_below,
        });
    }
    rows
}

fn free_lane(lanes: &mut Vec<Option<(String, usize)>>) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(i) => i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

/// The widest row, in lanes.
pub fn width(rows: &[GraphRow]) -> usize {
    rows.iter()
        .map(|r| {
            r.through
                .iter()
                .map(|(l, _)| *l)
                .chain(r.from_above.iter().map(|(l, _)| *l))
                .chain(r.to_below.iter().map(|(l, _)| *l))
                .chain(std::iter::once(r.lane))
                .max()
                .unwrap_or(0)
                + 1
        })
        .max()
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, parents: &[&str]) -> Commit {
        Commit {
            sha: sha.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: "A".into(),
            email: "a@x".into(),
            time: 0,
            subject: sha.into(),
            refs: vec![],
        }
    }

    #[test]
    fn a_linear_history_stays_in_one_lane() {
        let rows = layout(&[commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])]);
        assert!(rows.iter().all(|r| r.lane == 0));
        assert_eq!(rows[0].to_below.len(), 1);
        assert!(rows[2].to_below.is_empty());
        assert_eq!(width(&rows), 1);
    }

    #[test]
    fn a_merge_opens_a_lane_that_closes_at_the_fork() {
        // m merges f (feature) into b; both come from a.
        //   m
        //   |\
        //   b f
        //   |/
        //   a
        let rows = layout(&[
            commit("m", &["b", "f"]),
            commit("b", &["a"]),
            commit("f", &["a"]),
            commit("a", &[]),
        ]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].to_below.len(), 2);
        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[2].lane, 1);
        // `b` keeps lane 0 going while `f` sits in lane 1: lane 1 crosses b's row.
        assert!(rows[1].through.iter().any(|(l, _)| *l == 1));
        // `f` joins lane 0 below its row: its line bends into the lane of `a`.
        assert_eq!(
            rows[2].to_below.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec![0]
        );
        assert_eq!(rows[3].from_above.len(), 1);
        assert_eq!(width(&rows), 2);
    }

    #[test]
    fn a_branch_head_without_children_takes_its_own_lane() {
        // Two tips on top of the same base: x and y both children of a.
        let rows = layout(&[commit("x", &["a"]), commit("y", &["a"]), commit("a", &[])]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(
            rows[1].to_below.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec![0]
        );
        assert_eq!(rows[2].from_above.len(), 1);
    }

    #[test]
    fn colors_differ_between_lanes_open_at_once() {
        let rows = layout(&[commit("x", &["a"]), commit("y", &["a"]), commit("a", &[])]);
        assert_ne!(rows[0].color, rows[1].color);
    }
}
