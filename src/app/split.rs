//! The split editor: any number of panes, laid out as a tree of horizontal
//! and vertical splits.
//!
//! The active pane is the editor as ever (`self.editor`, `current_file`...).
//! The others show a file parked in `file_cache` or, when they show the file
//! being edited, a copy of its text (the mirror) with a cursor and a scroll
//! of their own. Touching another pane gives it the focus.

use super::*;
use crate::code_editor::Content;
use iced::widget::{column, mouse_area};
use std::collections::BTreeMap;

pub type PaneId = u32;

/// A copy of the active file's text for another pane.
pub struct Mirror {
    content: Content,
    /// The `text_rev` it was brought up to date for.
    rev: u64,
}

pub struct Pane {
    pub file: PathBuf,
    /// Set while this pane shows the file being edited without being the
    /// active pane.
    mirror: Option<Mirror>,
}

/// How the panes are arranged.
#[derive(Debug)]
pub enum Node {
    Leaf(PaneId),
    Branch {
        /// The two sides are next to each other (otherwise one above the other).
        side_by_side: bool,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    /// Replaces the leaf `id` by what `make` builds from it.
    fn replace_leaf(&mut self, id: PaneId, make: impl FnOnce(Node) -> Node) -> bool {
        match self {
            Node::Leaf(x) if *x == id => {
                let leaf = std::mem::replace(self, Node::Leaf(id));
                *self = make(leaf);
                true
            }
            Node::Leaf(_) => false,
            Node::Branch { first, second, .. } => {
                // Only the side that holds the leaf gets `make`.
                if first.contains(id) {
                    first.replace_leaf(id, make)
                } else {
                    second.replace_leaf(id, make)
                }
            }
        }
    }

    fn contains(&self, id: PaneId) -> bool {
        match self {
            Node::Leaf(x) => *x == id,
            Node::Branch { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    /// The tree without the leaf `id` (the branch it was in gives way to
    /// its other side); `None` if nothing is left.
    fn without(self, id: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(x) if x == id => None,
            Node::Leaf(x) => Some(Node::Leaf(x)),
            Node::Branch {
                side_by_side,
                first,
                second,
            } => match (first.without(id), second.without(id)) {
                (Some(a), Some(b)) => Some(Node::Branch {
                    side_by_side,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (a, b) => a.or(b),
            },
        }
    }

    fn leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Branch { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }
}

pub struct Split {
    pub layout: Node,
    pub panes: BTreeMap<PaneId, Pane>,
    pub active: PaneId,
    next_id: PaneId,
}

impl Split {
    /// The panes in the order they are drawn.
    #[cfg(test)]
    pub fn layout_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        self.layout.leaves(&mut ids);
        ids
    }
}

impl SimpleEditApp {
    /// Splits the active pane in two; `show` is the file the new pane takes
    /// (and the focus), the current file otherwise.
    pub(super) fn split_editor(
        &mut self,
        side_by_side: bool,
        show: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(current) = self.current_file.clone().filter(|p| !is_virtual(p)) else {
            self.set_status(t!("split.unavailable"), true);
            return Task::none();
        };
        let split = self.split.get_or_insert_with(|| {
            let mut panes = BTreeMap::new();
            panes.insert(
                0,
                Pane {
                    file: current.clone(),
                    mirror: None,
                },
            );
            Split {
                layout: Node::Leaf(0),
                panes,
                active: 0,
                next_id: 1,
            }
        });
        let new = split.next_id;
        split.next_id += 1;
        let file = show.clone().unwrap_or_else(|| current.clone());
        split.panes.insert(
            new,
            Pane {
                file: file.clone(),
                mirror: None,
            },
        );
        let active = split.active;
        split.layout.replace_leaf(active, |leaf| Node::Branch {
            side_by_side,
            first: Box::new(leaf),
            second: Box::new(Node::Leaf(new)),
        });
        if file != current {
            split.active = new;
            let task = self.activate(file);
            self.sync_split();
            return task;
        }
        self.sync_split();
        Task::none()
    }

    /// Gives the focus to a pane.
    pub(super) fn focus_pane(&mut self, id: PaneId) -> Task<Message> {
        let Some(split) = &mut self.split else {
            return Task::none();
        };
        let (from, Some(file)) = (split.active, split.panes.get(&id).map(|p| p.file.clone()))
        else {
            return Task::none();
        };
        if from == id {
            return Task::none();
        }
        if self.current_file.as_ref() == Some(&file) {
            // The same file: the panes trade their views of its text.
            let mirror = split.panes.get_mut(&id).and_then(|p| p.mirror.take());
            if let Some(mut mirror) = mirror {
                std::mem::swap(&mut self.editor.content, &mut mirror.content);
                mirror.rev = self.text_rev;
                if let Some(old) = split.panes.get_mut(&from) {
                    old.mirror = Some(mirror);
                }
            }
            split.active = id;
            return Task::none();
        }
        split.active = id;
        self.activate(file)
    }

    /// What was done in a pane that has not the focus: a scroll stays in
    /// the pane, anything else is a click or a key meant for it, so it takes
    /// the focus first.
    pub(super) fn split_action(
        &mut self,
        id: PaneId,
        action: text_editor::Action,
    ) -> Task<Message> {
        if let text_editor::Action::Scroll { .. } = action {
            if let Some(content) = self.pane_content_mut(id) {
                content.perform(action);
            }
            return Task::none();
        }
        let before = self.current_file.clone();
        let same_file = self
            .split
            .as_ref()
            .and_then(|s| s.panes.get(&id))
            .map(|p| &p.file)
            == before.as_ref();
        let task = self.focus_pane(id);
        // Typed text goes only where the focus has really moved (a file
        // that has to be read first takes the click with it).
        if same_file || self.current_file != before {
            let forwarded = self.handle(Message::EditorAction(action));
            return Task::batch([task, forwarded]);
        }
        task
    }

    pub(super) fn pane_content_mut(&mut self, id: PaneId) -> Option<&mut Content> {
        let pane = self.split.as_mut()?.panes.get_mut(&id)?;
        match &mut pane.mirror {
            Some(mirror) => Some(&mut mirror.content),
            None => self.file_cache.get_mut(&pane.file)?.content.as_mut(),
        }
    }

    /// Closes a pane (the active one if the focus is to be moved first).
    pub(super) fn close_pane(&mut self, id: PaneId) -> Task<Message> {
        let Some(split) = &self.split else {
            return Task::none();
        };
        let mut task = Task::none();
        if split.active == id {
            let mut leaves = Vec::new();
            split.layout.leaves(&mut leaves);
            if let Some(next) = leaves.into_iter().find(|l| *l != id) {
                task = self.focus_pane(next);
            }
        }
        self.drop_pane(id);
        task
    }

    fn drop_pane(&mut self, id: PaneId) {
        let Some(split) = self.split.take() else {
            return;
        };
        let Split {
            layout,
            mut panes,
            active,
            next_id,
        } = split;
        panes.remove(&id);
        self.split = match layout.without(id) {
            Some(Node::Leaf(_)) | None => None,
            Some(layout) => Some(Split {
                layout,
                panes,
                active,
                next_id,
            }),
        };
    }

    /// Keeps the panes true to the files after anything happened: the active
    /// pane shows the current file, the ones that show it too have a copy
    /// of its text, and a pane whose file was closed goes.
    pub(super) fn sync_split(&mut self) {
        let Some(current) = self.current_file.clone() else {
            return;
        };
        let pending = self.pending_open.is_some();
        let text_rev = self.text_rev;
        let Some(split) = &mut self.split else {
            return;
        };
        let active = split.active;
        let mut text: Option<String> = None;
        let mut gone = Vec::new();
        for (id, pane) in &mut split.panes {
            if *id == active {
                if !pending {
                    pane.file = current.clone();
                }
                continue;
            }
            if pane.file != current {
                pane.mirror = None;
                if !self.file_cache.contains_key(&pane.file) {
                    gone.push(*id);
                }
                continue;
            }
            let text = text.get_or_insert_with(|| crate::editor::buffer_text(&self.editor.content));
            match &mut pane.mirror {
                None => {
                    pane.mirror = Some(Mirror {
                        content: Content::with_text(text),
                        rev: text_rev,
                    });
                }
                Some(mirror) if mirror.rev != text_rev => {
                    // Its own cursor stays where it was.
                    let at = mirror.content.cursor_position();
                    let old = crate::editor::buffer_text(&mirror.content);
                    crate::editor::replace_text(&mut mirror.content, &old, text);
                    mirror.content.select_bytes(at.0, at.1, 0);
                    mirror.rev = text_rev;
                }
                Some(_) => {}
            }
        }
        for id in gone {
            self.drop_pane(id);
        }
    }

    /// The editor area: the active pane's `body` among the other panes.
    pub(super) fn with_split<'a>(
        &'a self,
        body: Element<'a, Message>,
        dark: bool,
    ) -> Element<'a, Message> {
        let Some(split) = &self.split else {
            return body;
        };
        let mut body = Some(body);
        self.node_view(split, &split.layout, &mut body, dark)
    }

    fn node_view<'a>(
        &'a self,
        split: &'a Split,
        node: &'a Node,
        body: &mut Option<Element<'a, Message>>,
        dark: bool,
    ) -> Element<'a, Message> {
        match node {
            Node::Leaf(id) => self.pane_view(split, *id, body, dark),
            Node::Branch {
                side_by_side,
                first,
                second,
            } => {
                let a = self.node_view(split, first, body, dark);
                let b = self.node_view(split, second, body, dark);
                let (a, b) = (
                    container(a).width(Length::Fill).height(Length::Fill),
                    container(b).width(Length::Fill).height(Length::Fill),
                );
                if *side_by_side {
                    row![
                        a,
                        container(Space::new().width(1))
                            .height(Length::Fill)
                            .style(se_theme::rule(dark)),
                        b
                    ]
                    .height(Length::Fill)
                    .into()
                } else {
                    column![a, rule(dark), b].height(Length::Fill).into()
                }
            }
        }
    }

    /// One pane: its header (file, close) and its text.
    fn pane_view<'a>(
        &'a self,
        split: &'a Split,
        id: PaneId,
        body: &mut Option<Element<'a, Message>>,
        dark: bool,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let Some(pane) = split.panes.get(&id) else {
            return Space::new().into();
        };
        let active = id == split.active;
        let header = row![
            button(
                text(file_label(&pane.file))
                    .size(12)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(if active { p.accent } else { p.muted })
                    .shaping(iced::widget::text::Shaping::Advanced)
            )
            .padding([4, 10])
            .on_press(Message::SplitFocus(id))
            .style(
                (se_theme::GhostButton {
                    dark,
                    active: false
                })
                .style()
            ),
            Space::new().width(Length::Fill),
            button(text("×").size(14).color(p.muted))
                .padding([0, 8])
                .on_press(Message::ClosePane(id))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: false
                    })
                    .style()
                ),
        ]
        .padding([2, 6])
        .align_y(Alignment::Center);
        let view: Element<Message> = if active {
            body.take().unwrap_or_else(|| Space::new().into())
        } else {
            let on_action = move |a| Message::SplitAction(id, a);
            match &pane.mirror {
                Some(mirror) => EditorState::pane_view(
                    &mirror.content,
                    self.editor.language.as_deref(),
                    &self.config,
                    on_action,
                ),
                None => match self
                    .file_cache
                    .get(&pane.file)
                    .and_then(|e| e.content.as_ref().map(|c| (c, e.language.as_deref())))
                {
                    Some((content, language)) => {
                        EditorState::pane_view(content, language, &self.config, on_action)
                    }
                    // Not read yet: a click reads it.
                    None => mouse_area(
                        container(text(t!("split.click_to_open").to_string()).color(p.muted))
                            .center(Length::Fill),
                    )
                    .on_press(Message::SplitFocus(id))
                    .into(),
                },
            }
        };
        column![header, rule(dark), view].into()
    }
}
