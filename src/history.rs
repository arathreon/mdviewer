//! The browsing history of one session (one window), as a tree of visits. It is never
//! saved: when the window closes, the history is lost.
//!
//! Only local Markdown files that the viewer shows are in the history. Web links, mail
//! links and files that open in File Explorer are not, because the viewer does not
//! show them.
//!
//! **A node is one visit, not one file.** Each visit has exactly one parent: the page
//! where the user clicked the link. Thus, the history is always a real tree, also when
//! notes link to each other in a circle. A file that the user opened from two notes is
//! in the tree two times, once below each note.
//!
//! The rules:
//! - A link adds a child below the current node, with two exceptions that add no node:
//!   - the file is the current file or a file above it in the same branch: the viewer
//!     goes up to that node. Thus, a circle A → B → A keeps the tree at A → B;
//!   - the current node already has a child with that file: the viewer goes to that
//!     child. Thus, more clicks between A and B do not make the tree larger.
//!
//!   In both cases, the file opens at the heading of the link, or else at the place
//!   where the user left it (as with "Back" and "Forward").
//! - "Open…" and drag and drop clear the history (the user's choice): the history
//!   starts again with only that file.
//! - "Back" goes to the parent. "Forward" goes to the child that was visited last.
//! - A click on a node in the panel goes to that visit. It adds no node.
//! - Old branches are never removed by a navigation (a browser removes the "forward"
//!   entries). Only the limit [`MAX_NODES`] removes the oldest nodes.

use std::path::{Path, PathBuf};

use crate::platform;

/// The maximum number of nodes. When the tree is larger, the oldest nodes are removed,
/// so that a long session cannot make the page (the history panel) very large.
pub const MAX_NODES: usize = 200;

/// The file and the scroll position of one visit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The Markdown file.
    pub path: PathBuf,
    /// The vertical scroll position in CSS pixels, as the page script last sent it.
    pub scroll: u32,
}

/// One visit in the tree.
#[derive(Debug)]
struct Node {
    /// The file and the scroll position.
    entry: Entry,
    /// The node of the page where the link was clicked; `None` for a root.
    parent: Option<usize>,
    /// The children, in the order of the visits.
    children: Vec<usize>,
    /// The child that was visited last: the target of "Forward".
    last_child: Option<usize>,
}

/// One line of the history panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row<'a> {
    /// The node id. A click in the panel sends it (see [`History::go_to`]).
    pub id: usize,
    /// The level in the tree: 0 for a root.
    pub depth: usize,
    /// The file.
    pub path: &'a Path,
    /// `true` for the current node.
    pub current: bool,
    /// `true` if the same file is on an earlier line (another visit of it).
    pub repeated: bool,
}

/// The history of the current session.
#[derive(Debug, Default)]
pub struct History {
    /// All nodes. The index is the node id; a removed node leaves `None`, so that the
    /// ids of the other nodes do not change.
    nodes: Vec<Option<Node>>,
    /// The root nodes, the oldest first.
    roots: Vec<usize>,
    /// The current node, or `None` before the first file.
    current: Option<usize>,
    /// The number of nodes that are not removed.
    live: usize,
}

impl History {
    /// A history that starts with the first file of the window, if there is one.
    pub fn new(first: Option<&Path>) -> History {
        let mut history = History::default();
        if let Some(path) = first {
            history.start_again(path);
        }
        history
    }

    /// A file from "Open…" or from drag and drop: the old history is removed, and the
    /// history starts again with only this file (also if it is the current file). The
    /// node ids start again at 0.
    pub fn start_again(&mut self, path: &Path) {
        *self = History::default();
        self.add(path, None);
    }

    /// A click on a link to `path`. `to_heading` is `true` if the link names a heading.
    ///
    /// If `path` is the current file, a file above it in the branch, or the file of a
    /// child of the current node, the viewer goes to that node: it opens at the
    /// heading, or else at the place where the user left it. Otherwise, a new child is
    /// added below the current node.
    pub fn follow_link(&mut self, path: &Path, to_heading: bool) {
        let Some(current) = self.current else {
            self.add(path, None);
            return;
        };
        match self.known_node(current, path) {
            Some(id) => {
                self.move_to(id);
                if to_heading {
                    // The page goes to the heading, not to the old position.
                    self.node_mut(id).entry.scroll = 0;
                }
            }
            None => self.add(path, Some(current)),
        }
    }

    /// The node for a link from `current` to `path` that is already in the tree: the
    /// current node or a node above it (the nearest first), else the most recent child
    /// of `current` with that file.
    fn known_node(&self, current: usize, path: &Path) -> Option<usize> {
        let mut at = Some(current);
        while let Some(id) = at {
            let node = self.node(id);
            if same_file(&node.entry.path, path) {
                return Some(id);
            }
            at = node.parent;
        }
        self.node(current)
            .children
            .iter()
            .rev()
            .copied()
            .find(|&child| same_file(&self.node(child).entry.path, path))
    }

    /// Goes to the parent of the current node. Gives the file to show, or `None` at a
    /// root.
    pub fn back(&mut self) -> Option<PathBuf> {
        let current = self.current?;
        let parent = self.node(current).parent?;
        self.node_mut(parent).last_child = Some(current);
        self.move_to(parent);
        Some(self.node(parent).entry.path.clone())
    }

    /// Goes to the child that was visited last. Gives the file to show, or `None` if
    /// the current node has no children.
    pub fn forward(&mut self) -> Option<PathBuf> {
        let child = self.node(self.current?).last_child?;
        self.move_to(child);
        Some(self.node(child).entry.path.clone())
    }

    /// Goes to the node `id` (a click in the history panel). Gives the file to show, or
    /// `None` if there is no such node (for example, it was removed).
    pub fn go_to(&mut self, id: usize) -> Option<PathBuf> {
        let path = self.nodes.get(id)?.as_ref()?.entry.path.clone();
        self.move_to(id);
        Some(path)
    }

    /// Gives `true` if the current node has a parent ("Back" is possible).
    pub fn can_go_back(&self) -> bool {
        self.current
            .is_some_and(|id| self.node(id).parent.is_some())
    }

    /// Gives `true` if the current node has a child ("Forward" is possible).
    pub fn can_go_forward(&self) -> bool {
        self.current
            .is_some_and(|id| self.node(id).last_child.is_some())
    }

    /// The file and the scroll position of the current node, or `None` before the
    /// first file.
    pub fn current(&self) -> Option<&Entry> {
        self.current.map(|id| &self.node(id).entry)
    }

    /// Keeps the scroll position of the current page (from the page script).
    pub fn set_scroll(&mut self, scroll: u32) {
        if let Some(entry) = self.current_entry_mut() {
            entry.scroll = scroll;
        }
    }

    /// The lines of the history panel: the tree in the order of the visits (each node
    /// before its children, the children in the order of the visits).
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows: Vec<Row<'_>> = Vec::with_capacity(self.live);
        // A stack of (node, depth); reversed, so that the first child comes out first.
        let mut stack: Vec<(usize, usize)> = self.roots.iter().rev().map(|&id| (id, 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            let node = self.node(id);
            let path = node.entry.path.as_path();
            let repeated = rows.iter().any(|row| same_file(row.path, path));
            rows.push(Row {
                id,
                depth,
                path,
                current: self.current == Some(id),
                repeated,
            });
            stack.extend(node.children.iter().rev().map(|&child| (child, depth + 1)));
        }
        rows
    }

    /// A node that exists. Node ids come only from this history, so a missing node is
    /// a programming error.
    fn node(&self, id: usize) -> &Node {
        self.nodes[id].as_ref().expect("node exists")
    }

    /// A node that exists, for changes.
    fn node_mut(&mut self, id: usize) -> &mut Node {
        self.nodes[id].as_mut().expect("node exists")
    }

    /// The entry of the current node, for changes.
    fn current_entry_mut(&mut self) -> Option<&mut Entry> {
        let id = self.current?;
        Some(&mut self.node_mut(id).entry)
    }

    /// Makes `id` the current node. Each node above it remembers the branch to it, so
    /// that "Forward" from there comes back here.
    fn move_to(&mut self, id: usize) {
        self.current = Some(id);
        let mut child = id;
        while let Some(parent) = self.node(child).parent {
            self.node_mut(parent).last_child = Some(child);
            child = parent;
        }
    }

    /// Adds a node below `parent` (or a root) and makes it the current node.
    fn add(&mut self, path: &Path, parent: Option<usize>) {
        let id = self.nodes.len();
        self.nodes.push(Some(Node {
            entry: Entry {
                path: path.to_path_buf(),
                scroll: 0,
            },
            parent,
            children: Vec::new(),
            last_child: None,
        }));
        self.live += 1;
        match parent {
            Some(parent) => self.node_mut(parent).children.push(id),
            None => self.roots.push(id),
        }
        self.move_to(id);
        while self.live > MAX_NODES {
            self.remove_oldest();
        }
    }

    /// Removes the oldest node that is not the current one. Its children take its place
    /// (below its parent, or as roots), so that a long chain of links can also become
    /// shorter.
    fn remove_oldest(&mut self) {
        let Some(id) =
            (0..self.nodes.len()).find(|&id| self.nodes[id].is_some() && self.current != Some(id))
        else {
            return;
        };
        let node = self.nodes[id].take().expect("node exists");
        self.live -= 1;
        for &child in &node.children {
            self.node_mut(child).parent = node.parent;
        }
        let siblings = match node.parent {
            Some(parent) => &mut self.node_mut(parent).children,
            None => &mut self.roots,
        };
        if let Some(place) = siblings.iter().position(|&sibling| sibling == id) {
            siblings.splice(place..=place, node.children.iter().copied());
        }
        if let Some(parent) = node.parent {
            let parent = self.node_mut(parent);
            if parent.last_child == Some(id) {
                parent.last_child = node.last_child.or(parent.children.last().copied());
            }
        }
    }
}

/// Gives `true` if two paths name the same file, with the name rules of the file
/// system ([`platform::same_name`]). On Windows, letter case is ignored, so `Note.md`
/// and `note.md` are the same file.
pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b || platform::same_name(a.as_os_str(), b.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test path.
    fn p(name: &str) -> PathBuf {
        PathBuf::from(format!(r"D:\notes\{name}.md"))
    }

    /// A history that starts at `first`, then follows links to `links` in this order.
    fn links(first: &str, links: &[&str]) -> History {
        let mut history = History::new(Some(&p(first)));
        for name in links {
            history.follow_link(&p(name), false);
        }
        history
    }

    /// The panel as text: one "name/depth" for each row, with "*" for the current row
    /// and "+" for a repeated file.
    fn panel(history: &History) -> Vec<String> {
        history
            .rows()
            .iter()
            .map(|row| {
                format!(
                    "{}/{}{}{}",
                    row.path.file_stem().unwrap().to_string_lossy(),
                    row.depth,
                    if row.current { "*" } else { "" },
                    if row.repeated { "+" } else { "" }
                )
            })
            .collect()
    }

    /// The file of the current node.
    fn at(history: &History) -> String {
        let path = &history.current().unwrap().path;
        path.file_stem().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn an_empty_history_cannot_go_anywhere() {
        let mut history = History::new(None);
        assert_eq!(history.current(), None);
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), None);
        assert_eq!(history.go_to(0), None);
        assert!(history.rows().is_empty());
        // No current node: the scroll position is ignored.
        history.set_scroll(10);
        assert_eq!(history.current(), None);
        // The first link without a page makes a root.
        history.follow_link(&p("a"), false);
        assert_eq!(panel(&history), ["a/0*"]);
    }

    #[test]
    fn links_make_children_and_back_and_forward_move_along_them() {
        let mut history = links("a", &["b", "c"]);
        assert_eq!(panel(&history), ["a/0", "b/1", "c/2*"]);
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());
        assert_eq!(history.back(), Some(p("b")));
        assert_eq!(history.back(), Some(p("a")));
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), Some(p("b")));
        assert_eq!(history.forward(), Some(p("c")));
        assert_eq!(history.forward(), None);
    }

    #[test]
    fn a_new_link_after_back_keeps_the_old_branch() {
        // A → B → C, back to A, then A → D.
        let mut history = links("a", &["b", "c"]);
        history.back();
        history.back();
        history.follow_link(&p("d"), false);
        assert_eq!(panel(&history), ["a/0", "b/1", "c/2", "d/1*"]);
        // "Forward" from A goes to the child that was visited last.
        assert_eq!(history.back(), Some(p("a")));
        assert_eq!(history.forward(), Some(p("d")));
    }

    #[test]
    fn a_circle_of_two_notes_goes_back_up_and_adds_no_node() {
        // A links to B, B links back to A.
        let mut history = links("a", &["b", "a"]);
        assert_eq!(panel(&history), ["a/0*", "b/1"]);
        assert_eq!(history.forward(), Some(p("b")));
        // Back and forth many times: the tree does not grow.
        for _ in 0..5 {
            history.follow_link(&p("a"), false);
            history.follow_link(&p("b"), false);
        }
        assert_eq!(panel(&history), ["a/0", "b/1*"]);
    }

    #[test]
    fn a_link_to_a_file_of_a_child_goes_to_that_child() {
        // A → B, back to A, A → C, back to A; then the link to B again.
        let mut history = links("a", &["b"]);
        history.set_scroll(250);
        history.back();
        history.follow_link(&p("c"), false);
        history.back();
        history.follow_link(&p("b"), false);
        assert_eq!(panel(&history), ["a/0", "b/1*", "c/1"]);
        // B opens at the place where the user left it.
        assert_eq!(history.current().unwrap().scroll, 250);
        // "Forward" from A now goes to B, the child that was visited last.
        history.back();
        assert_eq!(history.forward(), Some(p("b")));
    }

    #[test]
    fn a_longer_circle_goes_up_to_the_first_note() {
        let mut history = links("a", &["b", "c", "a"]);
        assert_eq!(panel(&history), ["a/0*", "b/1", "c/2"]);
        assert_eq!(history.forward(), Some(p("b")));
        assert_eq!(history.forward(), Some(p("c")));
        // Upper and lower case name the same file on Windows.
        history.follow_link(&PathBuf::from(r"d:\NOTES\B.md"), false);
        assert_eq!(panel(&history), ["a/0", "b/1*", "c/2"]);
    }

    #[test]
    fn a_file_from_another_branch_is_a_new_repeated_node() {
        // A → B, back to A, A → C, C → B: B is not above C, so it is a new visit.
        let mut history = links("a", &["b"]);
        history.back();
        history.follow_link(&p("c"), false);
        history.follow_link(&p("b"), false);
        assert_eq!(panel(&history), ["a/0", "b/1", "c/1", "b/2*+"]);
        assert_eq!(history.back(), Some(p("c")));
    }

    #[test]
    fn going_up_keeps_the_scroll_position_unless_the_link_names_a_heading() {
        let mut history = links("a", &[]);
        history.set_scroll(700);
        history.follow_link(&p("b"), false);
        assert_eq!(history.current().unwrap().scroll, 0);
        history.follow_link(&p("a"), false);
        assert_eq!(history.current().unwrap().scroll, 700);
        history.follow_link(&p("b"), false);
        history.set_scroll(300);
        history.follow_link(&p("a"), true);
        assert_eq!(history.current().unwrap().scroll, 0);
        // A link to a heading of the current file: the same node, at the heading.
        history.set_scroll(50);
        history.follow_link(&p("a"), true);
        assert_eq!(history.current().unwrap().scroll, 0);
        assert_eq!(panel(&history), ["a/0*", "b/1"]);
    }

    #[test]
    fn each_node_keeps_its_scroll_position() {
        let mut history = links("a", &[]);
        history.set_scroll(120);
        history.follow_link(&p("b"), false);
        history.set_scroll(40);
        history.back();
        assert_eq!(history.current().unwrap().scroll, 120);
        history.forward();
        assert_eq!(history.current().unwrap().scroll, 40);
    }

    #[test]
    fn open_and_drop_clear_the_history() {
        let mut history = links("a", &["b"]);
        history.start_again(&p("x"));
        assert_eq!(panel(&history), ["x/0*"]);
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
        // The old nodes are gone; the ids start again at 0.
        assert_eq!(history.go_to(1), None);
        assert_eq!(history.go_to(0), Some(p("x")));
        // The same file again also starts again, at the top.
        history.follow_link(&p("y"), false);
        history.back();
        history.set_scroll(90);
        history.start_again(&p("x"));
        assert_eq!(panel(&history), ["x/0*"]);
        assert_eq!(history.current().unwrap().scroll, 0);
        // A link from there makes a child as usual.
        history.follow_link(&p("a"), false);
        assert_eq!(panel(&history), ["x/0", "a/1*"]);
    }

    #[test]
    fn a_click_in_the_panel_goes_to_that_visit() {
        let mut history = links("a", &["b", "c"]);
        history.back();
        history.back();
        history.follow_link(&p("d"), false);
        // The ids are the places of the visits: a 0, b 1, c 2, d 3.
        assert_eq!(history.go_to(2), Some(p("c")));
        assert_eq!(panel(&history), ["a/0", "b/1", "c/2*", "d/1"]);
        assert_eq!(history.back(), Some(p("b")));
        // "Forward" from A now goes along the branch to C again.
        assert_eq!(history.back(), Some(p("a")));
        assert_eq!(history.forward(), Some(p("b")));
        assert_eq!(history.go_to(99), None);
        assert_eq!(at(&history), "b");
    }

    #[test]
    fn the_limit_removes_the_oldest_nodes_also_from_a_long_chain() {
        let names: Vec<String> = (0..MAX_NODES + 5).map(|i| format!("f{i}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let history = links(names[0], &names[1..]);
        let rows = history.rows();
        assert_eq!(rows.len(), MAX_NODES);
        // The chain is shorter: the oldest kept file is the new root.
        assert_eq!(rows[0].path, p("f5"));
        assert_eq!(rows[0].depth, 0);
        assert_eq!(at(&history), format!("f{}", MAX_NODES + 4));
        let mut history = history;
        let mut steps = 0;
        while history.back().is_some() {
            steps += 1;
        }
        assert_eq!(steps, MAX_NODES - 1);
    }

    #[test]
    fn removed_nodes_give_their_children_to_their_parent() {
        // A → B → C, back to A.
        let mut history = links("a", &["b", "c"]);
        history.back();
        history.back();
        // A is current, so the oldest other node is B. C moves up below A.
        history.remove_oldest();
        assert_eq!(panel(&history), ["a/0*", "c/1"]);
        assert_eq!(history.forward(), Some(p("c")));
        assert_eq!(history.go_to(1), None);
    }

    #[test]
    #[cfg(windows)]
    fn same_file_ignores_case_only() {
        assert!(same_file(&p("a"), &p("a")));
        assert!(same_file(
            Path::new(r"D:\Notes\Žluťoučký.md"),
            Path::new(r"d:\notes\žluťoučký.MD")
        ));
        assert!(!same_file(&p("a"), &p("b")));
        assert!(!same_file(Path::new(r"D:\a\x.md"), Path::new(r"D:\b\x.md")));
    }
}
