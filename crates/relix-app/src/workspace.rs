/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_project::SplitDirection;
use std::collections::BTreeMap;

pub type PaneId = u32;
pub type TabId = u32;

const MIN_RATIO: u16 = 15;
const MAX_RATIO: u16 = 85;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Area {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    fn split(self, direction: SplitDirection, ratio: u16) -> (Self, Self) {
        match direction {
            SplitDirection::Vertical => {
                let first = (u32::from(self.width) * u32::from(ratio) / 100) as u16;
                (
                    Self::new(self.x, self.y, first, self.height),
                    Self::new(self.x + first, self.y, self.width - first, self.height),
                )
            }
            SplitDirection::Horizontal => {
                let first = (u32::from(self.height) * u32::from(ratio) / 100) as u16;
                (
                    Self::new(self.x, self.y, self.width, first),
                    Self::new(self.x, self.y + first, self.width, self.height - first),
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutNode {
    Leaf(PaneId),
    Split {
        direction: SplitDirection,
        ratio: u16,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut leaves = first.leaves();
                leaves.extend(second.leaves());
                leaves
            }
        }
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == pane,
            Self::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    pub fn areas(&self, area: Area) -> Vec<(PaneId, Area)> {
        match self {
            Self::Leaf(id) => vec![(*id, area)],
            Self::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (first_area, second_area) = area.split(*direction, *ratio);
                let mut areas = first.areas(first_area);
                areas.extend(second.areas(second_area));
                areas
            }
        }
    }

    fn split_leaf(&mut self, target: PaneId, new_pane: PaneId, direction: SplitDirection) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                *self = Self::Split {
                    direction,
                    ratio: 50,
                    first: Box::new(Self::Leaf(target)),
                    second: Box::new(Self::Leaf(new_pane)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target, new_pane, direction)
                    || second.split_leaf(target, new_pane, direction)
            }
        }
    }

    /// Removes a leaf by replacing its parent split with the sibling subtree.
    fn remove_leaf(&mut self, target: PaneId) -> bool {
        let Self::Split { first, second, .. } = self else {
            return false;
        };
        if matches!(**first, Self::Leaf(id) if id == target) {
            *self = (**second).clone();
            return true;
        }
        if matches!(**second, Self::Leaf(id) if id == target) {
            *self = (**first).clone();
            return true;
        }
        first.remove_leaf(target) || second.remove_leaf(target)
    }

    /// Grows (positive delta) or shrinks the target pane inside its closest parent split.
    fn adjust(&mut self, target: PaneId, delta: i16) -> bool {
        let Self::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return false;
        };
        if first.adjust(target, delta) || second.adjust(target, delta) {
            return true;
        }
        let signed = if first.contains(target) {
            delta
        } else if second.contains(target) {
            -delta
        } else {
            return false;
        };
        let updated = i32::from(*ratio) + i32::from(signed);
        *ratio = updated.clamp(i32::from(MIN_RATIO), i32::from(MAX_RATIO)) as u16;
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<TabId>,
    pub active: usize,
}

impl Pane {
    fn new(id: PaneId) -> Self {
        Self {
            id,
            tabs: Vec::new(),
            active: 0,
        }
    }

    pub fn active_tab(&self) -> Option<TabId> {
        self.tabs.get(self.active).copied()
    }
}

#[derive(Debug, Clone)]
pub struct Workspace<T> {
    layout: LayoutNode,
    panes: BTreeMap<PaneId, Pane>,
    tabs: BTreeMap<TabId, T>,
    focused: PaneId,
    next_id: u32,
}

impl<T> Default for Workspace<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Workspace<T> {
    pub fn new() -> Self {
        let mut panes = BTreeMap::new();
        panes.insert(1, Pane::new(1));
        Self {
            layout: LayoutNode::Leaf(1),
            panes,
            tabs: BTreeMap::new(),
            focused: 1,
            next_id: 2,
        }
    }

    fn allocate_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn layout(&self) -> &LayoutNode {
        &self.layout
    }

    pub fn focused_pane(&self) -> PaneId {
        self.focused
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    pub fn pane_order(&self) -> Vec<PaneId> {
        self.layout.leaves()
    }

    pub fn focus_pane(&mut self, id: PaneId) {
        if self.panes.contains_key(&id) {
            self.focused = id;
        }
    }

    pub fn tab(&self, id: TabId) -> Option<&T> {
        self.tabs.get(&id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut T> {
        self.tabs.get_mut(&id)
    }

    pub fn tabs(&self) -> impl Iterator<Item = (TabId, &T)> {
        self.tabs.iter().map(|(id, tab)| (*id, tab))
    }

    pub fn tabs_mut(&mut self) -> impl Iterator<Item = (TabId, &mut T)> {
        self.tabs.iter_mut().map(|(id, tab)| (*id, tab))
    }

    pub fn active_tab_id(&self) -> Option<TabId> {
        self.panes.get(&self.focused)?.active_tab()
    }

    pub fn active_tab(&self) -> Option<&T> {
        self.tabs.get(&self.active_tab_id()?)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut T> {
        let id = self.active_tab_id()?;
        self.tabs.get_mut(&id)
    }

    pub fn pane_of(&self, tab: TabId) -> Option<PaneId> {
        self.panes
            .values()
            .find(|pane| pane.tabs.contains(&tab))
            .map(|pane| pane.id)
    }

    pub fn add_tab(&mut self, pane: PaneId, tab: T) -> TabId {
        let id = self.allocate_id();
        self.tabs.insert(id, tab);
        let pane_id = if self.panes.contains_key(&pane) {
            pane
        } else {
            self.focused
        };
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.tabs.push(id);
            pane.active = pane.tabs.len() - 1;
        }
        self.focused = pane_id;
        id
    }

    pub fn activate_tab(&mut self, tab: TabId) {
        if let Some(pane_id) = self.pane_of(tab)
            && let Some(pane) = self.panes.get_mut(&pane_id)
        {
            pane.active = pane.tabs.iter().position(|id| *id == tab).unwrap_or(0);
            self.focused = pane_id;
        }
    }

    pub fn remove_tab(&mut self, tab: TabId) -> Option<T> {
        let pane_id = self.pane_of(tab)?;
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            let index = pane.tabs.iter().position(|id| *id == tab).unwrap_or(0);
            pane.tabs.remove(index);
            if pane.active >= pane.tabs.len() || index < pane.active {
                pane.active = pane.active.saturating_sub(1);
            }
        }
        self.tabs.remove(&tab)
    }

    pub fn select_tab_index(&mut self, index: usize) {
        if let Some(pane) = self.panes.get_mut(&self.focused)
            && index < pane.tabs.len()
        {
            pane.active = index;
        }
    }

    pub fn cycle_tab(&mut self, forward: bool) {
        if let Some(pane) = self.panes.get_mut(&self.focused)
            && !pane.tabs.is_empty()
        {
            let count = pane.tabs.len();
            pane.active = if forward {
                (pane.active + 1) % count
            } else {
                (pane.active + count - 1) % count
            };
        }
    }

    pub fn split(&mut self, direction: SplitDirection) -> PaneId {
        let new_pane = self.allocate_id();
        self.layout.split_leaf(self.focused, new_pane, direction);
        self.panes.insert(new_pane, Pane::new(new_pane));
        self.focused = new_pane;
        new_pane
    }

    /// Closes a pane and returns its tabs so the caller can persist or discard them. The
    /// last remaining pane is never removed; its tabs are returned and it stays empty.
    pub fn close_pane(&mut self, id: PaneId) -> Vec<T> {
        let Some(pane) = self.panes.get(&id) else {
            return Vec::new();
        };
        let removed: Vec<T> = pane
            .tabs
            .clone()
            .into_iter()
            .filter_map(|tab| self.tabs.remove(&tab))
            .collect();
        if self.panes.len() == 1 {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.tabs.clear();
                pane.active = 0;
            }
            return removed;
        }
        self.layout.remove_leaf(id);
        self.panes.remove(&id);
        if self.focused == id {
            self.focused = self.layout.leaves().first().copied().unwrap_or(1);
        }
        removed
    }

    pub fn resize_focused(&mut self, delta: i16) -> bool {
        self.layout.adjust(self.focused, delta)
    }

    /// Moves focus to the nearest pane in `direction`, measured on a virtual canvas so the
    /// result does not depend on the current terminal size.
    pub fn focus_direction(&mut self, direction: Direction) -> bool {
        let areas = self.layout.areas(Area::new(0, 0, 1000, 1000));
        let Some(&(_, current)) = areas.iter().find(|(id, _)| *id == self.focused) else {
            return false;
        };
        let overlaps = |a1: u16, a2: u16, b1: u16, b2: u16| a1 < b2 && b1 < a2;
        let candidate = areas
            .iter()
            .filter(|(id, _)| *id != self.focused)
            .filter_map(|(id, area)| {
                let distance = match direction {
                    Direction::Left
                        if area.right() <= current.x
                            && overlaps(area.y, area.bottom(), current.y, current.bottom()) =>
                    {
                        current.x - area.right()
                    }
                    Direction::Right
                        if area.x >= current.right()
                            && overlaps(area.y, area.bottom(), current.y, current.bottom()) =>
                    {
                        area.x - current.right()
                    }
                    Direction::Up
                        if area.bottom() <= current.y
                            && overlaps(area.x, area.right(), current.x, current.right()) =>
                    {
                        current.y - area.bottom()
                    }
                    Direction::Down
                        if area.y >= current.bottom()
                            && overlaps(area.x, area.right(), current.x, current.right()) =>
                    {
                        area.y - current.bottom()
                    }
                    _ => return None,
                };
                Some((distance, *id))
            })
            .min();
        match candidate {
            Some((_, id)) => {
                self.focused = id;
                true
            }
            None => false,
        }
    }

    pub fn restore(
        layout: LayoutNode,
        panes: Vec<Pane>,
        tabs: BTreeMap<TabId, T>,
        focused: PaneId,
    ) -> Self {
        let next_id = panes
            .iter()
            .map(|pane| pane.id)
            .chain(tabs.keys().copied())
            .max()
            .unwrap_or(0)
            + 1;
        let panes: BTreeMap<PaneId, Pane> = panes.into_iter().map(|pane| (pane.id, pane)).collect();
        let focused = if panes.contains_key(&focused) {
            focused
        } else {
            layout.leaves().first().copied().unwrap_or(1)
        };
        Self {
            layout,
            panes,
            tabs,
            focused,
            next_id,
        }
    }

    pub fn allocate_restored_id(&mut self) -> u32 {
        self.allocate_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_open_activate_and_close() {
        let mut workspace: Workspace<&str> = Workspace::new();
        let pane = workspace.focused_pane();
        let a = workspace.add_tab(pane, "a");
        let b = workspace.add_tab(pane, "b");
        assert_eq!(workspace.active_tab(), Some(&"b"));
        workspace.cycle_tab(true);
        assert_eq!(workspace.active_tab_id(), Some(a));
        workspace.activate_tab(b);
        assert_eq!(workspace.remove_tab(b), Some("b"));
        assert_eq!(workspace.active_tab_id(), Some(a));
    }

    #[test]
    fn split_focus_resize_and_close() {
        let mut workspace: Workspace<&str> = Workspace::new();
        let left = workspace.focused_pane();
        workspace.add_tab(left, "left");
        let right = workspace.split(SplitDirection::Vertical);
        workspace.add_tab(right, "right");
        let bottom = workspace.split(SplitDirection::Horizontal);
        assert_eq!(workspace.pane_order(), vec![left, right, bottom]);
        assert!(workspace.focus_direction(Direction::Up));
        assert_eq!(workspace.focused_pane(), right);
        assert!(workspace.focus_direction(Direction::Left));
        assert_eq!(workspace.focused_pane(), left);
        assert!(!workspace.focus_direction(Direction::Left));
        assert!(workspace.resize_focused(10));
        let areas = workspace.layout().areas(Area::new(0, 0, 100, 40));
        assert_eq!(areas[0].1.width, 60);
        let removed = workspace.close_pane(right);
        assert_eq!(removed, vec!["right"]);
        assert_eq!(workspace.pane_order(), vec![left, bottom]);
    }

    #[test]
    fn last_pane_is_kept() {
        let mut workspace: Workspace<&str> = Workspace::new();
        let pane = workspace.focused_pane();
        workspace.add_tab(pane, "only");
        assert_eq!(workspace.close_pane(pane), vec!["only"]);
        assert_eq!(workspace.pane_order(), vec![pane]);
        assert_eq!(workspace.active_tab(), None);
    }

    #[test]
    fn ratios_are_clamped() {
        let mut workspace: Workspace<&str> = Workspace::new();
        workspace.split(SplitDirection::Vertical);
        for _ in 0..20 {
            workspace.resize_focused(10);
        }
        let LayoutNode::Split { ratio, .. } = workspace.layout() else {
            panic!("expected split");
        };
        assert_eq!(*ratio, MIN_RATIO);
    }
}
