//! Inventories: stacks, slot arrays, and the transfer rules both sides use.

use bevy::prelude::*;

use crate::content::{ContentId, ContentRegistry};

/// Registers inventory resources.
pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingInventoryUpdates>();
    }
}

/// A stack of items. `count == 0` means the slot is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ItemStack {
    /// The item.
    pub item: ContentId,
    /// How many.
    pub count: u32,
}

impl ItemStack {
    /// An empty stack.
    pub const EMPTY: Self = Self {
        item: ContentId::NONE,
        count: 0,
    };

    /// Creates a stack, or `EMPTY` when the count is zero.
    pub fn new(item: ContentId, count: u32) -> Self {
        if count == 0 {
            Self::EMPTY
        } else {
            Self { item, count }
        }
    }

    /// Whether the slot is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.item == ContentId::NONE
    }

    /// Whether two stacks hold the same item.
    pub fn same_item(&self, other: &Self) -> bool {
        !self.is_empty() && self.item == other.item
    }

    /// Splits off up to `amount` items, returning the taken stack.
    pub fn take(&mut self, amount: u32) -> Self {
        let taken = amount.min(self.count);
        self.count -= taken;
        if self.count == 0 {
            self.item = ContentId::NONE;
        }
        Self::new(self.item, taken)
    }
}

/// An address inside a container: which slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SlotRef {
    /// A slot in the hotbar (0..9).
    Hotbar(u8),
    /// A slot in the backpack (0..36).
    Backpack(u8),
    /// A slot in the machine's input area.
    MachineInput(u8),
    /// A slot in the machine's output area.
    MachineOutput(u8),
    /// A slot in the machine's fuel area.
    MachineFuel(u8),
    /// A slot in a chest.
    Chest(u8),
    /// A machine upgrade module slot.
    Module(u8),
}

/// How a slot may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// Any item.
    Any,
    /// Output-only: can be taken but not inserted.
    Output,
    /// Fuel only.
    Fuel,
    /// Modules only.
    Module,
}

/// A player's inventory: 9 hotbar slots plus 36 backpack slots.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Inventory {
    /// Hotbar slots.
    pub hotbar: [ItemStack; 9],
    /// Backpack slots.
    pub backpack: Vec<ItemStack>,
    /// Currently selected hotbar slot.
    pub selected: u8,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            hotbar: [ItemStack::EMPTY; 9],
            backpack: vec![ItemStack::EMPTY; 36],
            selected: 0,
        }
    }
}

impl Inventory {
    /// Reads a slot.
    pub fn get(&self, slot: SlotRef) -> Option<&ItemStack> {
        match slot {
            SlotRef::Hotbar(index) => self.hotbar.get(index as usize),
            SlotRef::Backpack(index) => self.backpack.get(index as usize),
            _ => None,
        }
    }

    /// Mutably reads a slot.
    pub fn get_mut(&mut self, slot: SlotRef) -> Option<&mut ItemStack> {
        match slot {
            SlotRef::Hotbar(index) => self.hotbar.get_mut(index as usize),
            SlotRef::Backpack(index) => self.backpack.get_mut(index as usize),
            _ => None,
        }
    }

    /// The item in the selected hotbar slot.
    pub fn held(&self) -> ItemStack {
        self.hotbar
            .get(self.selected as usize)
            .copied()
            .unwrap_or(ItemStack::EMPTY)
    }

    /// Inserts items, filling existing stacks first and then empty slots.
    ///
    /// Returns the number of items that did not fit.
    pub fn insert(&mut self, item: ContentId, mut count: u32, content: &ContentRegistry) -> u32 {
        let max_stack = content.item(item).map_or(64, |item| item.stack);
        for stack in self.hotbar.iter_mut().chain(self.backpack.iter_mut()) {
            if count == 0 {
                break;
            }
            if stack.item == item && stack.count < max_stack {
                let room = max_stack - stack.count;
                let moved = room.min(count);
                stack.count += moved;
                count -= moved;
            }
        }
        for stack in self.hotbar.iter_mut().chain(self.backpack.iter_mut()) {
            if count == 0 {
                break;
            }
            if stack.is_empty() {
                let moved = max_stack.min(count);
                *stack = ItemStack::new(item, moved);
                count -= moved;
            }
        }
        count
    }

    /// Whether the inventory can accept all of `count` of `item`.
    pub fn can_insert(&self, item: ContentId, count: u32, content: &ContentRegistry) -> bool {
        let max_stack = content.item(item).map_or(64, |item| item.stack);
        let mut room = 0u32;
        for stack in self.hotbar.iter().chain(self.backpack.iter()) {
            if stack.item == item {
                room += max_stack.saturating_sub(stack.count);
            } else if stack.is_empty() {
                room += max_stack;
            }
            if room >= count {
                return true;
            }
        }
        room >= count
    }

    /// Removes up to `count` of an item. Returns how many were removed.
    pub fn remove(&mut self, item: ContentId, count: u32) -> u32 {
        let mut removed = 0;
        for stack in self.hotbar.iter_mut().chain(self.backpack.iter_mut()) {
            if removed == count {
                break;
            }
            if stack.item == item && !stack.is_empty() {
                let taken = (count - removed).min(stack.count);
                stack.count -= taken;
                removed += taken;
                if stack.count == 0 {
                    *stack = ItemStack::EMPTY;
                }
            }
        }
        removed
    }

    /// Total count of an item.
    pub fn count_of(&self, item: ContentId) -> u32 {
        self.hotbar
            .iter()
            .chain(self.backpack.iter())
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum()
    }

    /// The first slot holding the item.
    pub fn find(&self, item: ContentId) -> Option<SlotRef> {
        for (index, stack) in self.hotbar.iter().enumerate() {
            if stack.item == item && !stack.is_empty() {
                return Some(SlotRef::Hotbar(index as u8));
            }
        }
        for (index, stack) in self.backpack.iter().enumerate() {
            if stack.item == item && !stack.is_empty() {
                return Some(SlotRef::Backpack(index as u8));
            }
        }
        None
    }

    /// Every item and count, for the save file and the network update.
    pub fn snapshot(&self) -> Vec<(ContentId, u32)> {
        self.hotbar
            .iter()
            .chain(self.backpack.iter())
            .filter(|stack| !stack.is_empty())
            .map(|stack| (stack.item, stack.count))
            .collect()
    }

    /// Whether the inventory is completely empty.
    pub fn is_empty(&self) -> bool {
        self.hotbar
            .iter()
            .chain(self.backpack.iter())
            .all(|stack| stack.is_empty())
    }
}

/// Moves items between two containers, following the transfer rules.
///
/// `kind_from` and `kind_to` are the slot kinds, which decide whether the move is legal.
pub fn transfer(
    from: &mut ItemStack,
    to: &mut ItemStack,
    kind_to: SlotKind,
    max_stack: u32,
    amount: Option<u32>,
) -> u32 {
    if from.is_empty() || kind_to == SlotKind::Output {
        return 0;
    }
    let requested = amount.unwrap_or(from.count);
    if to.is_empty() {
        let moved = requested.min(max_stack);
        *to = ItemStack::new(from.item, moved);
        from.count -= moved;
        if from.count == 0 {
            *from = ItemStack::EMPTY;
        }
        moved
    } else if to.item == from.item && to.count < max_stack {
        let moved = requested.min(max_stack - to.count);
        to.count += moved;
        from.count -= moved;
        if from.count == 0 {
            *from = ItemStack::EMPTY;
        }
        moved
    } else {
        0
    }
}

/// Inventory updates queued for the owning client, flushed by the server each tick.
#[derive(Debug, Default, Resource)]
pub struct PendingInventoryUpdates {
    /// Updates keyed by player entity.
    pub updates: Vec<(Entity, Inventory)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn insert_fills_stacks_then_empty_slots() {
        let content = content();
        let item = content.item_by_key("coal").expect("coal exists").id;
        let mut inventory = Inventory::default();
        assert_eq!(inventory.insert(item, 10, &content), 0);
        assert_eq!(inventory.count_of(item), 10);
        assert_eq!(inventory.insert(item, 200, &content), 0);
        assert_eq!(inventory.count_of(item), 210);
        // 45 slots × 64 = 2880 capacity.
        let overflow = inventory.insert(item, 5000, &content);
        assert_eq!(overflow, 5000 - (45 * 64 - 210));
    }

    #[test]
    fn remove_and_find() {
        let content = content();
        let item = content.item_by_key("raw_iron").expect("exists").id;
        let mut inventory = Inventory::default();
        inventory.insert(item, 5, &content);
        assert!(inventory.find(item).is_some());
        assert_eq!(inventory.remove(item, 2), 2);
        assert_eq!(inventory.count_of(item), 3);
        assert_eq!(inventory.remove(item, 10), 3);
        assert!(inventory.is_empty());
    }

    #[test]
    fn transfer_respects_stack_limit() {
        let mut from = ItemStack::new(ContentId(4), 40);
        let mut to = ItemStack::EMPTY;
        assert_eq!(transfer(&mut from, &mut to, SlotKind::Any, 64, None), 40);
        assert!(from.is_empty());
        assert_eq!(to.count, 40);

        let mut from = ItemStack::new(ContentId(4), 40);
        let mut to = ItemStack::new(ContentId(4), 50);
        assert_eq!(transfer(&mut from, &mut to, SlotKind::Any, 64, None), 14);
        assert_eq!(from.count, 26);
        assert_eq!(to.count, 64);
    }

    #[test]
    fn transfer_into_output_is_refused() {
        let mut from = ItemStack::new(ContentId(4), 10);
        let mut to = ItemStack::EMPTY;
        assert_eq!(transfer(&mut from, &mut to, SlotKind::Output, 64, None), 0);
    }

    proptest! {
        #[test]
        fn insert_never_loses_items(count in 0u32..3000) {
            let content = content();
            let item = content.item_by_key("coal").unwrap().id;
            let mut inventory = Inventory::default();
            let overflow = inventory.insert(item, count, &content);
            prop_assert_eq!(inventory.count_of(item) + overflow, count);
        }

        #[test]
        fn transfer_conserves_total(amount_a in 0u32..64, amount_b in 0u32..64) {
            let mut from = ItemStack::new(ContentId(4), amount_a);
            let mut to = ItemStack::new(ContentId(4), amount_b);
            let before = amount_a + amount_b;
            transfer(&mut from, &mut to, SlotKind::Any, 64, None);
            let after = from.count + to.count;
            prop_assert_eq!(before, after);
            prop_assert!(to.count <= 64);
        }
    }
}
