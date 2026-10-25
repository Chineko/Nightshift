#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId {
    slot: u32,
    generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    Container,
    Label,
    Button,
    Spacer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Widget {
    pub kind: WidgetKind,
    pub name: &'static str,
}

impl Widget {
    pub const fn new(name: &'static str, kind: WidgetKind) -> Self {
        Self { kind, name }
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

pub struct WidgetArena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
}

impl<T> Default for WidgetArena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> WidgetArena<T> {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn insert(&mut self, value: T) -> WidgetId {
        if let Some(slot_index) = self.free.pop() {
            let slot = &mut self.slots[slot_index];
            debug_assert!(slot.value.is_none());
            slot.value = Some(value);
            return WidgetId {
                slot: slot_index as u32,
                generation: slot.generation,
            };
        }

        let slot = self.slots.len();
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        WidgetId {
            slot: slot as u32,
            generation: 0,
        }
    }

    pub fn get(&self, id: WidgetId) -> Option<&T> {
        let slot = self.slots.get(id.slot as usize)?;
        (slot.generation == id.generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut T> {
        let slot = self.slots.get_mut(id.slot as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.value.as_mut()
    }

    pub fn remove(&mut self, id: WidgetId) -> Option<T> {
        let slot_index = id.slot as usize;
        let slot = self.slots.get_mut(slot_index)?;
        if slot.generation != id.generation {
            return None;
        }

        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(slot_index);
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_widget_ids_cannot_open_reused_drawers() {
        let mut widgets = WidgetArena::new();
        let old = widgets.insert(Widget::new("old", WidgetKind::Button));
        assert_eq!(widgets.remove(old).unwrap().name, "old");

        let replacement = widgets.insert(Widget::new("replacement", WidgetKind::Button));

        assert_ne!(old, replacement);
        assert!(widgets.get(old).is_none());
        assert_eq!(widgets.get(replacement).unwrap().name, "replacement");
    }

    #[test]
    fn live_widget_ids_support_mutation() {
        let mut widgets = WidgetArena::new();
        let id = widgets.insert(Widget::new("status", WidgetKind::Label));

        widgets.get_mut(id).unwrap().name = "sleep-status";

        assert_eq!(widgets.get(id).unwrap().name, "sleep-status");
    }
}
