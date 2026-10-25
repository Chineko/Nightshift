use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use taffy::prelude::*;

use crate::widget::WidgetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayoutNode(NodeId);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct LayoutEngine {
    tree: TaffyTree<()>,
    widgets: HashMap<WidgetId, LayoutNode>,
    parents: HashMap<LayoutNode, LayoutNode>,
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::new(),
            widgets: HashMap::new(),
            parents: HashMap::new(),
        }
    }

    pub fn leaf(&mut self, width: f32, height: f32) -> Result<LayoutNode> {
        self.tree
            .new_leaf(Style {
                size: Size {
                    width: length(width),
                    height: length(height),
                },
                ..Default::default()
            })
            .map(LayoutNode)
            .context("creating Nightshift layout leaf")
    }

    pub fn column(&mut self, children: &[LayoutNode], gap: f32) -> Result<LayoutNode> {
        self.flex(children, gap, FlexDirection::Column)
    }

    pub fn row(&mut self, children: &[LayoutNode], gap: f32) -> Result<LayoutNode> {
        self.flex(children, gap, FlexDirection::Row)
    }

    fn flex(
        &mut self,
        children: &[LayoutNode],
        gap: f32,
        direction: FlexDirection,
    ) -> Result<LayoutNode> {
        let child_ids = children.iter().map(|child| child.0).collect::<Vec<_>>();
        let parent = self
            .tree
            .new_with_children(
                Style {
                    flex_direction: direction,
                    gap: Size {
                        width: length(gap),
                        height: length(gap),
                    },
                    ..Default::default()
                },
                &child_ids,
            )
            .map(LayoutNode)
            .context("creating Nightshift flex container")?;
        for child in children {
            self.parents.insert(*child, parent);
        }
        Ok(parent)
    }

    pub fn bind(&mut self, widget: WidgetId, node: LayoutNode) {
        self.widgets.insert(widget, node);
    }

    pub fn compute(&mut self, root: LayoutNode, width: f32, height: f32) -> Result<()> {
        self.tree
            .compute_layout(
                root.0,
                Size {
                    width: AvailableSpace::Definite(width),
                    height: AvailableSpace::Definite(height),
                },
            )
            .context("computing Nightshift UI layout")
    }

    pub fn rect(&self, node: LayoutNode) -> Result<LayoutRect> {
        let layout = self
            .tree
            .layout(node.0)
            .context("reading Nightshift UI layout")?;
        let mut x = layout.location.x;
        let mut y = layout.location.y;
        let mut current = node;

        while let Some(parent) = self.parents.get(&current).copied() {
            let parent_layout = self
                .tree
                .layout(parent.0)
                .context("reading Nightshift parent layout")?;
            x += parent_layout.location.x;
            y += parent_layout.location.y;
            current = parent;
        }

        Ok(LayoutRect {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
        })
    }

    pub fn rect_for_widget(&self, widget: WidgetId) -> Result<LayoutRect> {
        let node = self
            .widgets
            .get(&widget)
            .copied()
            .ok_or_else(|| anyhow!("widget has no Nightshift layout node"))?;
        self.rect(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::{Widget, WidgetArena, WidgetKind};

    #[test]
    fn columns_space_children_without_manual_coordinates() {
        let mut layout = LayoutEngine::new();
        let first = layout.leaf(120.0, 40.0).unwrap();
        let second = layout.leaf(120.0, 60.0).unwrap();
        let root = layout.column(&[first, second], 12.0).unwrap();

        layout.compute(root, 400.0, 300.0).unwrap();

        assert_eq!(layout.rect(first).unwrap().y, 0.0);
        assert_eq!(layout.rect(second).unwrap().y, 52.0);
    }

    #[test]
    fn rows_space_children_without_clipboard_math() {
        let mut layout = LayoutEngine::new();
        let first = layout.leaf(80.0, 32.0).unwrap();
        let second = layout.leaf(100.0, 32.0).unwrap();
        let root = layout.row(&[first, second], 8.0).unwrap();

        layout.compute(root, 400.0, 200.0).unwrap();

        assert_eq!(layout.rect(first).unwrap().x, 0.0);
        assert_eq!(layout.rect(second).unwrap().x, 88.0);
    }

    #[test]
    fn nested_rectangles_are_root_relative() {
        let mut layout = LayoutEngine::new();
        let top = layout.leaf(200.0, 30.0).unwrap();
        let nested_first = layout.leaf(80.0, 20.0).unwrap();
        let nested_second = layout.leaf(80.0, 20.0).unwrap();
        let nested = layout.column(&[nested_first, nested_second], 10.0).unwrap();
        let root = layout.column(&[top, nested], 15.0).unwrap();

        layout.compute(root, 200.0, 95.0).unwrap();

        assert_eq!(layout.rect(nested_first).unwrap().y, 45.0);
        assert_eq!(layout.rect(nested_second).unwrap().y, 75.0);
    }

    #[test]
    fn widget_ids_resolve_the_same_geometry_used_for_drawing() {
        let mut widgets = WidgetArena::new();
        let button = widgets.insert(Widget::new("sleep", WidgetKind::Button));
        let mut layout = LayoutEngine::new();
        let node = layout.leaf(240.0, 72.0).unwrap();
        layout.bind(button, node);
        layout.compute(node, 240.0, 72.0).unwrap();

        assert_eq!(
            layout.rect_for_widget(button).unwrap(),
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 240.0,
                height: 72.0,
            }
        );
    }
}
