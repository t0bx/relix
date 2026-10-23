/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::input::LineInput;
use relix_core::{
    ColumnInfo, DataSourceId, ForeignKeyInfo, IndexInfo, ObjectInfo, ObjectKind, TableRef,
};
use std::collections::HashSet;

pub type NodeId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderKind {
    Tables,
    Views,
    Routines,
    Sequences,
    Triggers,
}

impl FolderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tables => "tables",
            Self::Views => "views",
            Self::Routines => "routines",
            Self::Sequences => "sequences",
            Self::Triggers => "triggers",
        }
    }

    pub fn of(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Table => Self::Tables,
            ObjectKind::View | ObjectKind::MaterializedView => Self::Views,
            ObjectKind::Function | ObjectKind::Procedure => Self::Routines,
            ObjectKind::Sequence => Self::Sequences,
            ObjectKind::Trigger => Self::Triggers,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailKind {
    Columns,
    Indexes,
    ForeignKeys,
}

impl DetailKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::Indexes => "indexes",
            Self::ForeignKeys => "foreign keys",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    DataSource(DataSourceId),
    Database {
        source: DataSourceId,
        name: String,
    },
    Schema {
        source: DataSourceId,
        database: String,
        name: String,
    },
    Folder {
        source: DataSourceId,
        folder: FolderKind,
    },
    Object {
        source: DataSourceId,
        info: ObjectInfo,
    },
    Detail {
        source: DataSourceId,
        table: TableRef,
        detail: DetailKind,
    },
    Column(ColumnInfo),
    Index(IndexInfo),
    ForeignKey(ForeignKeyInfo),
    KeySpace(DataSourceId),
    Message(String),
}

impl NodeKind {
    pub fn source(&self) -> Option<DataSourceId> {
        match self {
            Self::DataSource(source) | Self::KeySpace(source) => Some(*source),
            Self::Database { source, .. }
            | Self::Schema { source, .. }
            | Self::Folder { source, .. }
            | Self::Object { source, .. }
            | Self::Detail { source, .. } => Some(*source),
            _ => None,
        }
    }

    pub fn is_leaf(&self) -> bool {
        match self {
            Self::Column(_)
            | Self::Index(_)
            | Self::ForeignKey(_)
            | Self::KeySpace(_)
            | Self::Message(_) => true,
            Self::Object { info, .. } => !info.kind.is_relation(),
            _ => false,
        }
    }

    pub fn table(&self) -> Option<&TableRef> {
        match self {
            Self::Object { info, .. } if info.kind.is_relation() => Some(&info.object),
            Self::Detail { table, .. } => Some(table),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Children {
    NotLoaded,
    Loading,
    Loaded(Vec<NodeId>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExplorerNode {
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub depth: u16,
    pub children: Children,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewNode {
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
    pub kind: NodeKind,
    pub children: Option<Vec<NewNode>>,
}

impl NewNode {
    pub fn new(key: String, label: impl Into<String>, kind: NodeKind) -> Self {
        Self {
            key,
            label: label.into(),
            detail: None,
            kind,
            children: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_children(mut self, children: Vec<NewNode>) -> Self {
        self.children = Some(children);
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExplorerState {
    nodes: Vec<Option<ExplorerNode>>,
    roots: Vec<NodeId>,
    expanded: HashSet<String>,
    pub selected: usize,
    pub scroll: usize,
    pub filter: LineInput,
    pub filtering: bool,
}

impl ExplorerState {
    pub fn node(&self, id: NodeId) -> Option<&ExplorerNode> {
        self.nodes.get(id).and_then(Option::as_ref)
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut ExplorerNode> {
        self.nodes.get_mut(id).and_then(Option::as_mut)
    }

    fn insert(&mut self, node: ExplorerNode) -> NodeId {
        self.nodes.push(Some(node));
        self.nodes.len() - 1
    }

    pub fn find_key(&self, key: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|node| node.as_ref().is_some_and(|node| node.key == key))
    }

    pub fn root_for(&self, source: DataSourceId) -> Option<NodeId> {
        self.roots
            .iter()
            .copied()
            .find(|id| matches!(self.node(*id).map(|node| &node.kind), Some(NodeKind::DataSource(s)) if *s == source))
    }

    /// Synchronises the root nodes with the project's data sources, keeping already loaded
    /// subtrees of sources that still exist.
    pub fn set_roots(&mut self, sources: &[(DataSourceId, String, String)]) {
        let mut roots = Vec::new();
        for (source, label, detail) in sources {
            let id = match self.root_for(*source) {
                Some(id) => {
                    if let Some(node) = self.node_mut(id) {
                        node.label = label.clone();
                        node.detail = Some(detail.clone());
                    }
                    id
                }
                None => self.insert(ExplorerNode {
                    key: source.to_string(),
                    label: label.clone(),
                    detail: Some(detail.clone()),
                    kind: NodeKind::DataSource(*source),
                    parent: None,
                    depth: 0,
                    children: Children::NotLoaded,
                }),
            };
            roots.push(id);
        }
        let removed: Vec<NodeId> = self
            .roots
            .iter()
            .copied()
            .filter(|id| !roots.contains(id))
            .collect();
        for id in removed {
            self.drop_subtree(id);
        }
        self.roots = roots;
        self.clamp_selection();
    }

    fn drop_subtree(&mut self, id: NodeId) {
        if let Some(Children::Loaded(children)) = self.node(id).map(|node| node.children.clone()) {
            for child in children {
                self.drop_subtree(child);
            }
        }
        if let Some(slot) = self.nodes.get_mut(id) {
            *slot = None;
        }
    }

    pub fn is_expanded(&self, id: NodeId) -> bool {
        self.node(id)
            .is_some_and(|node| self.expanded.contains(&node.key))
    }

    pub fn expanded_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.expanded.iter().cloned().collect();
        keys.sort();
        keys
    }

    pub fn restore_expanded(&mut self, keys: impl IntoIterator<Item = String>) {
        self.expanded.extend(keys);
    }

    /// Marks the node expanded and reports whether its children still need loading.
    pub fn expand(&mut self, id: NodeId) -> bool {
        let Some(node) = self.node_mut(id) else {
            return false;
        };
        if node.kind.is_leaf() {
            return false;
        }
        let key = node.key.clone();
        let needs_load = matches!(node.children, Children::NotLoaded | Children::Failed(_));
        if needs_load {
            node.children = Children::Loading;
        }
        self.expanded.insert(key);
        needs_load
    }

    pub fn collapse(&mut self, id: NodeId) {
        if let Some(key) = self.node(id).map(|node| node.key.clone()) {
            self.expanded.remove(&key);
        }
    }

    pub fn set_children(&mut self, parent: NodeId, children: Vec<NewNode>) -> Vec<NodeId> {
        let Some(depth) = self.node(parent).map(|node| node.depth + 1) else {
            return Vec::new();
        };
        if let Some(Children::Loaded(old)) = self.node(parent).map(|node| node.children.clone()) {
            for child in old {
                self.drop_subtree(child);
            }
        }
        let ids = self.insert_children(parent, depth, children);
        self.clamp_selection();
        ids
    }

    fn insert_children(
        &mut self,
        parent: NodeId,
        depth: u16,
        children: Vec<NewNode>,
    ) -> Vec<NodeId> {
        let ids: Vec<NodeId> = children
            .into_iter()
            .map(|child| {
                let leaf = child.kind.is_leaf();
                let id = self.insert(ExplorerNode {
                    key: child.key,
                    label: child.label,
                    detail: child.detail,
                    kind: child.kind,
                    parent: Some(parent),
                    depth,
                    children: if leaf {
                        Children::Loaded(Vec::new())
                    } else {
                        Children::NotLoaded
                    },
                });
                if let Some(grandchildren) = child.children {
                    self.insert_children(id, depth + 1, grandchildren);
                }
                id
            })
            .collect();
        if let Some(node) = self.node_mut(parent) {
            node.children = Children::Loaded(ids.clone());
        }
        ids
    }

    pub fn set_failed(&mut self, id: NodeId, message: String) {
        if let Some(node) = self.node_mut(id) {
            node.children = Children::Failed(message);
        }
    }

    /// Forgets loaded children so the next expansion reloads them from the database.
    pub fn reset(&mut self, id: NodeId) {
        if let Some(Children::Loaded(children)) = self.node(id).map(|node| node.children.clone()) {
            for child in children {
                self.drop_subtree(child);
            }
        }
        if let Some(node) = self.node_mut(id) {
            node.children = Children::NotLoaded;
        }
        self.clamp_selection();
    }

    /// Expanded nodes whose children are not loaded yet, in tree order; used after a
    /// restart or reconnect to lazily re-open the previously expanded tree.
    pub fn pending_expansions(&self) -> Vec<NodeId> {
        self.visible()
            .into_iter()
            .filter(|id| {
                self.is_expanded(*id)
                    && self
                        .node(*id)
                        .is_some_and(|node| node.children == Children::NotLoaded)
            })
            .collect()
    }

    pub fn mark_loading(&mut self, id: NodeId) {
        if let Some(node) = self.node_mut(id) {
            node.children = Children::Loading;
        }
    }

    pub fn visible(&self) -> Vec<NodeId> {
        let filter = self.filter.text().trim().to_lowercase();
        let mut out = Vec::new();
        for root in &self.roots {
            self.collect_visible(*root, &filter, &mut out);
        }
        out
    }

    fn collect_visible(&self, id: NodeId, filter: &str, out: &mut Vec<NodeId>) -> bool {
        let Some(node) = self.node(id) else {
            return false;
        };
        let position = out.len();
        out.push(id);
        let mut descendant_matches = false;
        if let Children::Loaded(children) = &node.children
            && (self.expanded.contains(&node.key) || !filter.is_empty())
        {
            for child in children {
                descendant_matches |= self.collect_visible(*child, filter, out);
            }
        }
        let matches = filter.is_empty() || node.label.to_lowercase().contains(filter);
        let keep = matches || descendant_matches || node.parent.is_none();
        if !keep {
            out.truncate(position);
        } else if !filter.is_empty() && !self.expanded.contains(&node.key) && !descendant_matches {
            out.truncate(position + 1);
        }
        matches || descendant_matches
    }

    pub fn selected_node(&self) -> Option<NodeId> {
        self.visible().get(self.selected).copied()
    }

    pub fn select_node(&mut self, id: NodeId) {
        if let Some(index) = self.visible().iter().position(|candidate| *candidate == id) {
            self.selected = index;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        let count = self.visible().len();
        if count > 0 {
            self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
        }
    }

    pub fn select_parent(&mut self) {
        if let Some(parent) = self
            .selected_node()
            .and_then(|id| self.node(id))
            .and_then(|node| node.parent)
        {
            self.select_node(parent);
        }
    }

    fn clamp_selection(&mut self) {
        let count = self.visible().len();
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = Vec::new();
        let mut current = self.node(id).and_then(|node| node.parent);
        while let Some(parent) = current {
            chain.push(parent);
            current = self.node(parent).and_then(|node| node.parent);
        }
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> DataSourceId {
        DataSourceId::new()
    }

    fn child(key: &str, label: &str, kind: NodeKind) -> NewNode {
        NewNode::new(key.into(), label, kind)
    }

    fn schema(source: DataSourceId, name: &str) -> NodeKind {
        NodeKind::Schema {
            source,
            database: "db".into(),
            name: name.into(),
        }
    }

    #[test]
    fn lazy_expansion_and_visibility() {
        let id = source();
        let mut explorer = ExplorerState::default();
        explorer.set_roots(&[(id, "Local".into(), "PostgreSQL".into())]);
        let root = explorer.selected_node().unwrap();
        assert!(explorer.expand(root));
        assert!(!explorer.expand(root));
        explorer.set_children(
            root,
            vec![
                child("k/public", "public", schema(id, "public")),
                child("k/audit", "audit", schema(id, "audit")),
            ],
        );
        assert_eq!(explorer.visible().len(), 3);
        explorer.collapse(root);
        assert_eq!(explorer.visible().len(), 1);
    }

    #[test]
    fn filter_keeps_matching_paths() {
        let id = source();
        let mut explorer = ExplorerState::default();
        explorer.set_roots(&[(id, "Local".into(), "PostgreSQL".into())]);
        let root = explorer.selected_node().unwrap();
        explorer.expand(root);
        let schemas = explorer.set_children(
            root,
            vec![
                child("k/public", "public", schema(id, "public")),
                child("k/audit", "audit", schema(id, "audit")),
            ],
        );
        explorer.expand(schemas[0]);
        explorer.set_children(
            schemas[0],
            vec![child(
                "k/public/users",
                "users",
                NodeKind::Message("x".into()),
            )],
        );
        explorer.collapse(schemas[0]);
        explorer.filter.set("user");
        let labels: Vec<_> = explorer
            .visible()
            .iter()
            .map(|id| explorer.node(*id).unwrap().label.clone())
            .collect();
        assert_eq!(labels, vec!["Local", "public", "users"]);
    }

    #[test]
    fn preloaded_children_are_inserted_recursively() {
        let id = source();
        let mut explorer = ExplorerState::default();
        explorer.set_roots(&[(id, "Local".into(), "SQLite".into())]);
        let root = explorer.selected_node().unwrap();
        explorer.expand(root);
        let folder = child(
            "k/tables",
            "tables",
            NodeKind::Folder {
                source: id,
                folder: FolderKind::Tables,
            },
        )
        .with_children(vec![child(
            "k/tables/t",
            "t",
            NodeKind::Message("x".into()),
        )]);
        let ids = explorer.set_children(root, vec![folder]);
        explorer.expand(ids[0]);
        assert_eq!(explorer.visible().len(), 3);
        assert!(!explorer.expand(ids[0]));
    }

    #[test]
    fn expanded_keys_survive_reset_and_restore() {
        let id = source();
        let mut explorer = ExplorerState::default();
        explorer.set_roots(&[(id, "Local".into(), "PostgreSQL".into())]);
        let root = explorer.selected_node().unwrap();
        explorer.expand(root);
        let keys = explorer.expanded_keys();
        let mut restored = ExplorerState::default();
        restored.restore_expanded(keys);
        restored.set_roots(&[(id, "Local".into(), "PostgreSQL".into())]);
        assert_eq!(
            restored.pending_expansions(),
            vec![restored.root_for(id).unwrap()]
        );
        explorer.set_children(root, vec![child("k/a", "a", schema(id, "a"))]);
        explorer.reset(root);
        assert_eq!(explorer.visible().len(), 1);
        explorer.set_roots(&[]);
        assert!(explorer.visible().is_empty());
    }
}
