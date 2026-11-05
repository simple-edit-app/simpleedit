//! The Log tab on screen: the branches, the commit graph, a commit's files
//! and details.

use std::collections::BTreeMap;

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_editor, text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Theme};

use super::log::{short, Action, Avail, Dialog, NameKind};
use super::stash::format_time;
use super::{GitMsg, Panel};
use crate::app::Message;
use crate::git::graph::{GraphRow, COLORS};
use crate::git::history::{self, Branch, PlanAction, PlanItem, SearchMode};
use crate::theme as se_theme;

const ROW_H: f32 = 24.0;
const LANE_W: f32 = 12.0;

fn lane_color(index: usize, dark: bool) -> Color {
    const DARK: [(u8, u8, u8); COLORS] = [
        (0x6e, 0xa8, 0xff),
        (0x8f, 0xd1, 0x8f),
        (0xff, 0x9b, 0x6b),
        (0xc6, 0x8a, 0xe8),
        (0xe6, 0xc0, 0x6a),
        (0x5e, 0xcf, 0xd1),
        (0xf0, 0x6b, 0x6b),
        (0xa8, 0xb5, 0xc8),
    ];
    const LIGHT: [(u8, u8, u8); COLORS] = [
        (0x1b, 0x5f, 0xc6),
        (0x2e, 0x7d, 0x32),
        (0xd2, 0x56, 0x1f),
        (0x8e, 0x3a, 0xb8),
        (0x9a, 0x6a, 0x00),
        (0x00, 0x7c, 0x85),
        (0xc6, 0x28, 0x28),
        (0x5c, 0x61, 0x70),
    ];
    let (r, g, b) = if dark { DARK } else { LIGHT }[index % COLORS];
    Color::from_rgb8(r, g, b)
}

// ─── The graph cell of a row ────────────────────────────────────────────────

struct Cell {
    row: GraphRow,
    dark: bool,
}

impl<M> canvas::Program<M> for Cell {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let x = |lane: usize| 8.0 + lane as f32 * LANE_W;
        let (top, mid, bottom) = (0.0, bounds.height / 2.0, bounds.height);
        let line = |from: Point, to: Point, color: usize| {
            (
                Path::line(from, to),
                Stroke::default()
                    .with_color(lane_color(color, self.dark))
                    .with_width(1.6),
            )
        };
        let mut lines = Vec::new();
        for &(lane, color) in &self.row.through {
            lines.push(line(
                Point::new(x(lane), top),
                Point::new(x(lane), bottom),
                color,
            ));
        }
        for &(lane, color) in &self.row.from_above {
            lines.push(line(
                Point::new(x(lane), top),
                Point::new(x(self.row.lane), mid),
                color,
            ));
        }
        for &(lane, color) in &self.row.to_below {
            lines.push(line(
                Point::new(x(self.row.lane), mid),
                Point::new(x(lane), bottom),
                color,
            ));
        }
        for (path, stroke) in lines {
            frame.stroke(&path, stroke);
        }
        frame.fill(
            &Path::circle(Point::new(x(self.row.lane), mid), 3.6),
            lane_color(self.row.color, self.dark),
        );
        vec![frame.into_geometry()]
    }
}

fn graph_cell<'a>(row: &GraphRow, lanes: usize, dark: bool) -> Element<'a, Message> {
    Canvas::new(Cell {
        row: row.clone(),
        dark,
    })
    .width(Length::Fixed(lanes as f32 * LANE_W + 8.0))
    .height(Length::Fixed(ROW_H))
    .into()
}

// ─── The branch tree ────────────────────────────────────────────────────────

/// A folder of branches, or a branch.
#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    /// The ref this node stands for, when it is a branch or a tag.
    leaf: Option<Leaf>,
}

#[derive(Clone)]
struct Leaf {
    /// What `git log` takes.
    rev: String,
    branch: Option<Branch>,
}

fn insert(node: &mut Node, parts: &[&str], leaf: Leaf) {
    match parts.split_first() {
        None => node.leaf = Some(leaf),
        Some((head, rest)) => insert(
            node.children.entry(head.to_string()).or_default(),
            rest,
            leaf,
        ),
    }
}

impl Panel {
    pub(super) fn log_tab(&self, dark: bool) -> Element<'_, Message> {
        let middle: Element<Message> = match &self.log.dialog {
            Some(dialog) => self.dialog_view(dialog, dark),
            None => self.commit_list(dark),
        };
        row![
            self.branch_tree(dark),
            container(Space::new().width(1).height(Length::Fill)).style(se_theme::rule(dark)),
            middle,
            container(Space::new().width(1).height(Length::Fill)).style(se_theme::rule(dark)),
            self.commit_details(dark),
        ]
        .into()
    }

    fn branch_tree(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let filter = self.log.filter.to_lowercase();
        let mut column_ = column![].spacing(1);

        let head_row = self.tree_row(
            0,
            "HEAD (Current Branch)".to_string(),
            self.log.branch.as_deref() == Some("HEAD"),
            Some("HEAD".to_string()),
            None,
            dark,
        );
        column_ = column_.push(head_row);

        let Some(refs) = &self.log.refs else {
            return self.tree_frame(column_, dark);
        };
        let mut local = Node::default();
        let mut remote = Node::default();
        let mut tags = Node::default();
        for branch in &refs.branches {
            if !filter.is_empty() && !branch.name.to_lowercase().contains(&filter) {
                continue;
            }
            let parts: Vec<&str> = branch.name.split('/').collect();
            let leaf = Leaf {
                rev: branch.name.clone(),
                branch: Some(branch.clone()),
            };
            insert(
                if branch.remote {
                    &mut remote
                } else {
                    &mut local
                },
                &parts,
                leaf,
            );
        }
        for tag in &refs.tags {
            if !filter.is_empty() && !tag.to_lowercase().contains(&filter) {
                continue;
            }
            let parts: Vec<&str> = tag.split('/').collect();
            insert(
                &mut tags,
                &parts,
                Leaf {
                    rev: tag.clone(),
                    branch: None,
                },
            );
        }
        for (key, label, node) in [
            ("local", t!("git.local").to_string(), &local),
            ("remote", t!("git.remote").to_string(), &remote),
            ("tags", t!("git.tags").to_string(), &tags),
        ] {
            if node.children.is_empty() {
                continue;
            }
            let folded = self.log.folded.contains(key) && filter.is_empty();
            column_ = column_.push(self.folder_row(0, key.to_string(), label, folded, dark));
            if !folded {
                column_ = self.tree_children(column_, node, key, 1, !filter.is_empty(), dark);
            }
        }
        let _ = p;
        self.tree_frame(column_, dark)
    }

    fn tree_frame<'a>(
        &'a self,
        content: iced::widget::Column<'a, Message>,
        dark: bool,
    ) -> Element<'a, Message> {
        column![
            container(
                text_input(&t!("git.branch_filter"), &self.log.filter)
                    .on_input(|t| Message::Git(GitMsg::BranchFilter(t)))
                    .size(12.5)
                    .padding([4, 8])
                    .style(se_theme::Field::new(dark).style()),
            )
            .padding([6, 8]),
            scrollable(container(content).padding([2, 6]))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill),
        ]
        .width(Length::Fixed(250.0))
        .into()
    }

    fn tree_children<'a>(
        &'a self,
        mut column_: iced::widget::Column<'a, Message>,
        node: &Node,
        path: &str,
        depth: usize,
        expand_all: bool,
        dark: bool,
    ) -> iced::widget::Column<'a, Message> {
        for (name, child) in &node.children {
            let key = format!("{path}/{name}");
            if let Some(leaf) = &child.leaf {
                let selected = self.log.branch.as_deref() == Some(leaf.rev.as_str());
                column_ = column_.push(self.tree_row(
                    depth,
                    name.clone(),
                    selected,
                    Some(leaf.rev.clone()),
                    leaf.branch.clone(),
                    dark,
                ));
            }
            if !child.children.is_empty() {
                let folded = self.log.folded.contains(&key) && !expand_all;
                if child.leaf.is_none() {
                    column_ = column_.push(self.folder_row(
                        depth,
                        key.clone(),
                        name.clone(),
                        folded,
                        dark,
                    ));
                }
                if !folded {
                    column_ = self.tree_children(column_, child, &key, depth + 1, expand_all, dark);
                }
            }
        }
        column_
    }

    fn folder_row(
        &self,
        depth: usize,
        key: String,
        label: String,
        folded: bool,
        dark: bool,
    ) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        button(
            row![
                Space::new().width(depth as f32 * 14.0),
                text(if folded { "▸" } else { "▾" })
                    .size(11)
                    .color(p.muted)
                    .width(12),
                text(label).size(12.5),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([2, 4])
        .width(Length::Fill)
        .on_press(Message::Git(GitMsg::ToggleGroup(key)))
        .style(
            (se_theme::GhostButton {
                dark,
                active: false,
            })
            .style(),
        )
        .into()
    }

    /// A branch, a tag or HEAD in the tree.
    fn tree_row(
        &self,
        depth: usize,
        label: String,
        selected: bool,
        rev: Option<String>,
        branch: Option<Branch>,
        dark: bool,
    ) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let mut line = row![
            Space::new().width(depth as f32 * 14.0 + 16.0),
            text(label).size(12.5),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        if let Some(b) = &branch {
            if b.current {
                line = line.push(text("●").size(9).color(p.accent));
            }
            if b.ahead > 0 {
                line = line.push(text(format!("↑{}", b.ahead)).size(11).color(p.accent));
            }
            if b.behind > 0 {
                line = line.push(text(format!("↓{}", b.behind)).size(11).color(p.muted));
            }
        }
        let mut target = button(line).padding([2, 4]).width(Length::Fill).style(
            (se_theme::GhostButton {
                dark,
                active: selected,
            })
            .style(),
        );
        if let Some(rev) = rev.clone() {
            target = target.on_press(Message::Git(GitMsg::BranchClicked(rev)));
        }
        match (branch, rev) {
            (Some(branch), _) => self.branch_menu(target.into(), branch, dark),
            _ => target.into(),
        }
    }

    fn branch_menu<'a>(
        &'a self,
        target: Element<'a, Message>,
        branch: Branch,
        dark: bool,
    ) -> Element<'a, Message> {
        let checkout = t!("git.checkout").to_string();
        let diff = t!("git.diff_tree").to_string();
        let merge = t!("git.merge_into", name = branch.name.as_str()).to_string();
        let rebase = t!("git.rebase_onto", name = branch.name.as_str()).to_string();
        let name = branch.name.clone();
        let current = branch.current;
        crate::pointer_area::context_menu(target, move || {
            let item = |label: String, msg: Option<GitMsg>| -> Element<'static, Message> {
                let mut b = button(text(label).size(13))
                    .padding([7, 10])
                    .width(Length::Fixed(230.0))
                    .style(
                        (se_theme::MenuItem {
                            dark,
                            radius: se_theme::R_SM,
                            highlighted: false,
                        })
                        .style(),
                    );
                if let Some(m) = msg {
                    b = b.on_press(Message::Git(m));
                }
                b.into()
            };
            container(
                column![
                    item(
                        checkout.clone(),
                        (!current).then(|| GitMsg::Checkout(name.clone())),
                    ),
                    item(diff.clone(), Some(GitMsg::BranchDiff(name.clone()))),
                    item(
                        merge.clone(),
                        (!current).then(|| GitMsg::MergeBranch(name.clone())),
                    ),
                    item(
                        rebase.clone(),
                        (!current).then(|| GitMsg::RebaseOnto(name.clone())),
                    ),
                ]
                .spacing(1)
                .padding(6),
            )
            .style(se_theme::card(dark))
            .into()
        })
    }

    // ─── The commits ────────────────────────────────────────────────────────

    fn commit_list(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let log = &self.log;
        let avail = log.availability();
        let mut list = column![].spacing(0);
        list = list.push(
            container(
                row![
                    text_input(&t!("git.search_placeholder"), &log.search)
                        .on_input(|t| Message::Git(GitMsg::LogSearch(t)))
                        .on_submit(Message::Git(GitMsg::LogSearchRun))
                        .size(12.5)
                        .padding([3, 8])
                        .style(se_theme::Field::new(dark).style()),
                    pick_list(SearchMode::ALL, Some(log.search_mode), |mode| {
                        Message::Git(GitMsg::LogSearchMode(mode))
                    })
                    .text_size(12)
                    .padding([3, 8])
                    .width(Length::Fixed(130.0))
                    .style(se_theme::Select { dark }.style())
                    .menu_style(se_theme::Select { dark }.menu_style()),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([4, 8]),
        );
        if let Some(file) = &log.file {
            list = list.push(
                row![
                    text(t!("git.file_history_of", file = file.as_str()).to_string())
                        .size(12)
                        .color(se_theme::accent_color(dark))
                        .width(Length::Fill)
                        .wrapping(iced::widget::text::Wrapping::None),
                    button(text("×").size(13))
                        .padding([0, 8])
                        .on_press(Message::Git(GitMsg::ClearLogFile))
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style(),
                        ),
                ]
                .padding([4, 8])
                .align_y(Alignment::Center),
            );
        }
        // Only the rows around the part in view are built.
        let rows = vec![ROW_H; log.commits.len()];
        let (first, last, before, after) =
            super::diff_view::visible(&rows, log.view_top.get(), log.view_height.get().max(1400.0));
        list = list.push(Space::new().height(before));
        for (i, commit) in log
            .commits
            .iter()
            .enumerate()
            .skip(first)
            .take(last - first)
        {
            let selected = log.selected.contains(&i);
            let mut line = row![graph_cell(&log.rows[i], log.lanes, dark)]
                .spacing(6)
                .align_y(Alignment::Center);
            for badge in commit.refs.iter().take(2) {
                line = line.push(ref_badge(badge, dark));
            }
            if commit.refs.len() > 2 {
                line = line.push(
                    text(format!("+{}", commit.refs.len() - 2))
                        .size(11)
                        .color(p.muted),
                );
            }
            line = line
                .push(
                    container(
                        text(commit.subject.as_str())
                            .size(12.5)
                            .wrapping(iced::widget::text::Wrapping::None),
                    )
                    .width(Length::Fill)
                    .clip(true),
                )
                .push(
                    container(
                        text(commit.author.as_str())
                            .size(12)
                            .color(p.muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    )
                    .width(Length::Fixed(120.0))
                    .clip(true),
                )
                .push(
                    container(
                        text(format_time(commit.time))
                            .size(12)
                            .color(p.muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    )
                    .width(Length::Fixed(112.0)),
                );
            let target = button(line)
                .padding([0, 6])
                .height(Length::Fixed(ROW_H))
                .width(Length::Fill)
                .on_press(Message::Git(GitMsg::SelectCommit(i)))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: selected,
                    })
                    .style(),
                );
            // The menu opens on the right press; the row is selected first.
            let menu = self.commit_menu(target.into(), avail, dark);
            list = list.push(crate::floating::RightClick::new(menu, 0.0, move |_| {
                Message::Git(GitMsg::SelectForMenu(i))
            }));
        }
        list = list.push(Space::new().height(after));
        if log.commits.is_empty() {
            list = list.push(
                container(
                    text(t!("git.no_commits").to_string())
                        .size(12.5)
                        .color(p.muted),
                )
                .padding(12),
            );
        } else if log.commits.len() >= log.limit {
            list = list.push(
                container(
                    button(text(t!("git.load_more").to_string()).size(12.5))
                        .padding([4, 12])
                        .on_press(Message::Git(GitMsg::LoadMoreCommits))
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style(),
                        ),
                )
                .padding(8),
            );
        }
        scrollable(container(list).padding([2, 4]))
            .id(super::log::scroll_id())
            .on_scroll(|viewport| {
                Message::Git(GitMsg::LogScrolled(
                    viewport.absolute_offset().y as i32,
                    viewport.bounds().height as i32,
                ))
            })
            .style(se_theme::Scrollbar { dark }.style())
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The right-click menu of the selected commits: what can't be done with
    /// this selection is grayed out.
    fn commit_menu<'a>(
        &'a self,
        target: Element<'a, Message>,
        avail: Avail,
        dark: bool,
    ) -> Element<'a, Message> {
        use super::log::LogAction;
        use crate::git::ResetMode;
        let item = |key: String, msg: GitMsg, enabled: bool| (key, msg, enabled);
        let labels = vec![
            item(t!("git.copy_hash").to_string(), GitMsg::CopyHash, true),
            item(
                t!("git.cherry_pick").to_string(),
                GitMsg::Rewrite(Action::CherryPick),
                avail.cherry_pick,
            ),
            item(
                t!("git.revert").to_string(),
                GitMsg::Rewrite(Action::Revert),
                avail.revert,
            ),
            item(
                t!("git.drop").to_string(),
                GitMsg::Rewrite(Action::Drop),
                avail.drop,
            ),
            item(
                t!("git.squash").to_string(),
                GitMsg::Rewrite(Action::Squash),
                avail.squash,
            ),
            item(
                t!("git.reword").to_string(),
                GitMsg::Rewrite(Action::Reword),
                avail.reword,
            ),
            item(
                t!("git.log_rebase_from").to_string(),
                GitMsg::LogAction(LogAction::RebaseFrom),
                avail.rebase,
            ),
            item(
                t!("git.log_branch_here").to_string(),
                GitMsg::LogAction(LogAction::BranchHere),
                avail.one,
            ),
            item(
                t!("git.log_tag_here").to_string(),
                GitMsg::LogAction(LogAction::TagHere),
                avail.one,
            ),
            item(
                t!("git.log_checkout_here").to_string(),
                GitMsg::LogAction(LogAction::CheckoutHere),
                avail.one,
            ),
            item(
                t!("git.log_reset_soft").to_string(),
                GitMsg::LogAction(LogAction::Reset(ResetMode::Soft)),
                avail.one,
            ),
            item(
                t!("git.log_reset_mixed").to_string(),
                GitMsg::LogAction(LogAction::Reset(ResetMode::Mixed)),
                avail.one,
            ),
            item(
                t!("git.log_reset_hard").to_string(),
                GitMsg::LogAction(LogAction::Reset(ResetMode::Hard)),
                avail.one,
            ),
            item(
                t!("git.log_compare_head").to_string(),
                GitMsg::LogAction(LogAction::CompareHead),
                avail.one,
            ),
            item(
                t!("git.log_compare_two").to_string(),
                GitMsg::LogAction(LogAction::CompareSelected),
                avail.two,
            ),
        ];
        crate::pointer_area::context_menu(target, move || {
            let mut menu = column![].spacing(1).padding(6);
            for (label, message, enabled) in labels.iter().cloned() {
                let mut b = button(text(label).size(13))
                    .padding([7, 10])
                    .width(Length::Fixed(230.0))
                    .style(
                        (se_theme::MenuItem {
                            dark,
                            radius: se_theme::R_SM,
                            highlighted: false,
                        })
                        .style(),
                    );
                if enabled {
                    b = b.on_press(Message::Git(message));
                }
                menu = menu.push(b);
            }
            container(menu).style(se_theme::card(dark)).into()
        })
    }

    // ─── The details ────────────────────────────────────────────────────────

    fn commit_details(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let log = &self.log;
        let chosen = log.selected_commits();
        let content: Element<Message> = match chosen.len() {
            0 => container(
                text(t!("git.select_commit").to_string())
                    .size(12.5)
                    .color(p.muted),
            )
            .center(Length::Fill)
            .into(),
            1 => {
                let commit = chosen[0];
                let mut files = column![].spacing(1);
                for (i, change) in log.files.iter().enumerate() {
                    let (dir, name) = match change.path.rsplit_once('/') {
                        Some((dir, name)) => (dir, name),
                        None => ("", change.path.as_str()),
                    };
                    files = files.push(
                        button(
                            row![
                                text(change.kind.letter())
                                    .size(11)
                                    .font(se_theme::MONO)
                                    .color(super::view::kind_color(change.kind, dark))
                                    .width(12),
                                crate::file_icons::file(std::path::Path::new(name), dark),
                                text(name).size(12.5),
                                text(dir)
                                    .size(11.5)
                                    .color(p.muted)
                                    .wrapping(iced::widget::text::Wrapping::None),
                            ]
                            .spacing(8)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 8])
                        .width(Length::Fill)
                        .on_press(Message::Git(GitMsg::CommitFileDiff(i)))
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style(),
                        ),
                    );
                }
                let mut info = column![
                    text(log.message.as_str()).size(12.5),
                    text(format!(
                        "{} {} <{}>",
                        short(&commit.sha),
                        commit.author,
                        commit.email
                    ))
                    .size(12)
                    .color(p.muted),
                    text(t!("git.on_date", date = format_time(commit.time)).to_string())
                        .size(12)
                        .color(p.muted),
                ]
                .spacing(4);
                if !log.contained_in.is_empty() {
                    let shown: Vec<&str> = log
                        .contained_in
                        .iter()
                        .take(4)
                        .map(String::as_str)
                        .collect();
                    let more = log.contained_in.len().saturating_sub(4);
                    let mut line = shown.join(", ");
                    if more > 0 {
                        line.push_str(&format!(" +{more}"));
                    }
                    info = info.push(
                        text(
                            t!("git.in_branches", n = log.contained_in.len(), list = line)
                                .to_string(),
                        )
                        .size(12)
                        .color(p.muted),
                    );
                }
                column![
                    scrollable(container(files).padding([4, 6]))
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::FillPortion(3)),
                    container(Space::new().height(1))
                        .width(Length::Fill)
                        .style(se_theme::rule(dark)),
                    scrollable(container(info).padding([8, 12]))
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::FillPortion(2)),
                ]
                .into()
            }
            n => container(
                text(t!("git.n_selected", n = n).to_string())
                    .size(12.5)
                    .color(p.muted),
            )
            .center(Length::Fill)
            .into(),
        };
        container(content).width(Length::Fixed(340.0)).into()
    }

    // ─── A question before a rewrite ────────────────────────────────────────

    /// The commits of an interactive rebase, oldest first: what to do with
    /// each, and in what order.
    fn rebase_dialog<'a>(&'a self, items: &'a [PlanItem], dark: bool) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let mut list = column![].spacing(2);
        for (i, item) in items.iter().enumerate() {
            let step = |label: &'static str, up: bool, enabled: bool| {
                let mut b = button(text(label).size(12)).padding([2, 7]).style(
                    (se_theme::GhostButton {
                        dark,
                        active: false,
                    })
                    .style(),
                );
                if enabled {
                    b = b.on_press(Message::Git(GitMsg::RebaseMove(i, up)));
                }
                b
            };
            let dropped = item.action == PlanAction::Drop;
            let what: Element<'a, Message> = if item.action == PlanAction::Reword {
                text_input("", &item.subject)
                    .on_input(move |t| Message::Git(GitMsg::RebaseSubject(i, t)))
                    .size(12.5)
                    .padding([3, 8])
                    .style(se_theme::Field::new(dark).style())
                    .into()
            } else {
                text(item.subject.as_str())
                    .size(12.5)
                    .color(if dropped { p.muted } else { p.text })
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into()
            };
            list = list.push(
                row![
                    step("↑", true, i > 0),
                    step("↓", false, i + 1 < items.len()),
                    pick_list(PlanAction::ALL, Some(item.action), move |a| {
                        Message::Git(GitMsg::RebaseSet(i, a))
                    })
                    .text_size(12)
                    .padding([3, 8])
                    .width(Length::Fixed(118.0))
                    .style(se_theme::Select { dark }.style())
                    .menu_style(se_theme::Select { dark }.menu_style()),
                    text(super::log::short(&item.sha))
                        .size(11.5)
                        .font(se_theme::MONO)
                        .color(p.muted)
                        .width(58),
                    container(what).width(Length::Fill).clip(true),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        let problem = history::check_plan(items).err();
        let mut start = button(text(t!("git.rebase_start").to_string()).size(12.5))
            .padding([5, 14])
            .style((se_theme::Primary { dark }).style());
        if problem.is_none() {
            start = start.on_press(Message::Git(GitMsg::DialogOk));
        }
        container(
            column![
                text(t!("git.rebase_title").to_string())
                    .size(13)
                    .font(se_theme::UI_SEMIBOLD),
                text(t!("git.rebase_help").to_string())
                    .size(11.5)
                    .color(p.muted),
                scrollable(list)
                    .style(se_theme::Scrollbar { dark }.style())
                    .height(Length::Fill),
                match problem {
                    Some(error) => text(error).size(12).color(se_theme::error_color(dark)),
                    None => text(""),
                },
                row![
                    start,
                    button(text(t!("git.cancel").to_string()).size(12.5))
                        .padding([5, 14])
                        .on_press(Message::Git(GitMsg::DialogCancel))
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false
                            })
                            .style()
                        ),
                ]
                .spacing(8),
            ]
            .spacing(8),
        )
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn dialog_view<'a>(&'a self, dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        if let Dialog::Rebase { items, .. } = dialog {
            return self.rebase_dialog(items, dark);
        }
        let (title, body, pushed): (String, Element<'a, Message>, bool) = match dialog {
            Dialog::Message {
                action,
                content,
                pushed,
                ..
            } => (
                match action {
                    Action::Squash => t!("git.squash_title").to_string(),
                    _ => t!("git.reword_title").to_string(),
                },
                text_editor(content)
                    .on_action(|a| Message::Git(GitMsg::DialogMessage(a)))
                    .size(13)
                    .height(Length::Fixed(150.0))
                    .into(),
                *pushed,
            ),
            Dialog::Confirm { shas, pushed, .. } => (
                t!("git.drop_title", n = shas.len()).to_string(),
                text(t!("git.drop_text").to_string())
                    .size(12.5)
                    .color(p.muted)
                    .into(),
                *pushed,
            ),
            Dialog::Name { kind, sha, input } => (
                match kind {
                    NameKind::Branch => {
                        t!("git.name_branch_title", sha = super::log::short(sha)).to_string()
                    }
                    NameKind::Tag => {
                        t!("git.name_tag_title", sha = super::log::short(sha)).to_string()
                    }
                },
                text_input(&t!("git.new_branch_name"), input)
                    .on_input(|t| Message::Git(GitMsg::DialogName(t)))
                    .on_submit(Message::Git(GitMsg::DialogOk))
                    .size(13)
                    .padding([6, 10])
                    .style(se_theme::Field::new(dark).style())
                    .into(),
                false,
            ),
            Dialog::Reset { sha, mode } => (
                t!("git.reset_title", sha = super::log::short(sha)).to_string(),
                text(match mode {
                    crate::git::ResetMode::Soft => t!("git.reset_text_soft").to_string(),
                    crate::git::ResetMode::Mixed => t!("git.reset_text_mixed").to_string(),
                    crate::git::ResetMode::Hard => t!("git.reset_text_hard").to_string(),
                })
                .size(12.5)
                .color(if *mode == crate::git::ResetMode::Hard {
                    se_theme::error_color(dark)
                } else {
                    p.muted
                })
                .into(),
                false,
            ),
            Dialog::Rebase { .. } => unreachable!("shown by `rebase_dialog`"),
        };
        let mut form = column![text(title).size(13).font(se_theme::UI_SEMIBOLD), body].spacing(10);
        if pushed {
            form = form.push(
                text(t!("git.pushed_warning").to_string())
                    .size(12)
                    .color(se_theme::error_color(dark)),
            );
        }
        form = form.push(
            row![
                button(text(t!("git.ok").to_string()).size(12.5))
                    .padding([5, 14])
                    .on_press_maybe(
                        (!matches!(dialog, Dialog::Name { input, .. } if input.trim().is_empty()))
                            .then_some(Message::Git(GitMsg::DialogOk))
                    )
                    .style((se_theme::Primary { dark }).style()),
                button(text(t!("git.cancel").to_string()).size(12.5))
                    .padding([5, 14])
                    .on_press(Message::Git(GitMsg::DialogCancel))
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false
                        })
                        .style()
                    ),
            ]
            .spacing(8),
        );
        container(form).padding(16).width(Length::Fill).into()
    }
}

/// The label of a branch or tag next to a commit.
fn ref_badge<'a>(decoration: &str, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let (label, color) = if let Some(tag) = decoration.strip_prefix("tag: ") {
        (
            tag.to_string(),
            if dark {
                Color::from_rgb8(0xe6, 0xc0, 0x6a)
            } else {
                Color::from_rgb8(0x9a, 0x6a, 0x00)
            },
        )
    } else if let Some(branch) = decoration.strip_prefix("HEAD -> ") {
        (branch.to_string(), p.accent)
    } else if decoration.contains('/') {
        (
            decoration.to_string(),
            if dark {
                Color::from_rgb8(0xc6, 0x8a, 0xe8)
            } else {
                Color::from_rgb8(0x8e, 0x3a, 0xb8)
            },
        )
    } else if decoration == "HEAD" {
        ("HEAD".to_string(), p.accent)
    } else {
        (
            decoration.to_string(),
            if dark {
                Color::from_rgb8(0x8f, 0xd1, 0x8f)
            } else {
                Color::from_rgb8(0x2e, 0x7d, 0x32)
            },
        )
    };
    container(
        text(label)
            .size(11)
            .color(color)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .padding([0, 5])
    .max_width(140)
    .clip(true)
    .style(move |_: &Theme| iced::widget::container::Style {
        border: iced::Border {
            color: Color { a: 0.7, ..color },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}
