use anyhow::{Context, Result};
use taffy::prelude::*;

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
        self.tree
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
            .context("creating Nightshift flex container")
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
        Ok(LayoutRect {
            x: layout.location.x,
            y: layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
