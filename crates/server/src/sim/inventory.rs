//! Server inventory: intent handling and the per-owner update messages.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry, GearSlot, ItemCategory};
use deepcore_shared::inventory::{Inventory, ItemStack, SlotKind, SlotRef, transfer};
use deepcore_shared::protocol::{
    ClientIntent, ContainerId, FromClient, InventoryUpdate, ServerNotice, SlotAddress,
};
use tracing::debug;

use crate::sim::player::{OwnedBy, OwnsPlayer, Player, PlayerState};

/// Builds the message describing a player's inventory.
pub fn inventory_update(state: &PlayerState) -> InventoryUpdate {
    InventoryUpdate {
        hotbar: state.inventory.hotbar,
        backpack: state.inventory.backpack.clone(),
        selected: state.inventory.selected,
        gear: state.gear,
    }
}

/// Sends every player's inventory to its owner. Runs on `Changed<PlayerState>`.
pub fn sync_inventories(
    players: Query<(&OwnedBy, &PlayerState), Changed<PlayerState>>,
    mut writer: MessageWriter<ToClients<InventoryUpdate>>,
) {
    for (owner, state) in &players {
        writer.write(ToClients {
            targets: SendTargets::Single(ClientId::Client(owner.0)),
            message: inventory_update(state),
        });
    }
}

/// Handles inventory intents: hotbar selection, moves, drops, gear.
pub fn handle_inventory_intents(
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut players: Query<&mut PlayerState, With<Player>>,
    clients: Query<&OwnsPlayer>,
    content: Res<ContentRegistry>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
) {
    for FromClient { client_id, message } in intents.read() {
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok(mut state) = players.get_mut(owner.0) else {
            continue;
        };
        match message {
            ClientIntent::SelectSlot { slot } => {
                if (*slot as usize) < state.inventory.hotbar.len() {
                    state.inventory.selected = *slot;
                }
            }
            ClientIntent::MoveItems { from, to, count } => {
                move_items(&mut state.inventory, *from, *to, *count, &content);
            }
            ClientIntent::DropHeld => {
                let selected = state.inventory.selected as usize;
                let held = state.inventory.hotbar[selected];
                if !held.is_empty() {
                    state.inventory.hotbar[selected] = ItemStack::EMPTY;
                    debug!("player dropped {} x{}", held.item, held.count);
                }
            }
            ClientIntent::EquipGear { slot } => {
                equip_gear(&mut state, *slot, &content, &mut notices, *client_id);
            }
            ClientIntent::UnequipGear { slot } => {
                unequip_gear(&mut state, *slot, &content);
            }
            _ => {}
        }
    }
}

fn slot_ref(address: SlotAddress) -> Option<SlotRef> {
    match address.container {
        ContainerId::Hotbar => Some(SlotRef::Hotbar(address.index)),
        ContainerId::Backpack => Some(SlotRef::Backpack(address.index)),
        ContainerId::MachineInput => Some(SlotRef::MachineInput(address.index)),
        ContainerId::MachineOutput => Some(SlotRef::MachineOutput(address.index)),
        ContainerId::MachineFuel => Some(SlotRef::MachineFuel(address.index)),
        ContainerId::MachineModule => Some(SlotRef::Module(address.index)),
        ContainerId::Chest => Some(SlotRef::Chest(address.index)),
        ContainerId::Gear => None,
    }
}

/// Moves items between two player slots.
pub fn move_items(
    inventory: &mut Inventory,
    from: SlotAddress,
    to: SlotAddress,
    count: Option<u32>,
    content: &ContentRegistry,
) {
    let (Some(from_ref), Some(to_ref)) = (slot_ref(from), slot_ref(to)) else {
        return;
    };
    if from_ref == to_ref {
        return;
    }
    // Copy both stacks out, run the shared transfer rule, write both back. Copying two 8-byte
    // stacks is cheaper than any split-borrow dance and cannot alias.
    let max_stack = inventory
        .get(from_ref)
        .map(|stack| content.item(stack.item).map_or(64, |item| item.stack))
        .unwrap_or(64);
    let mut source = inventory.get(from_ref).copied().unwrap_or(ItemStack::EMPTY);
    let mut destination = inventory.get(to_ref).copied().unwrap_or(ItemStack::EMPTY);
    transfer(
        &mut source,
        &mut destination,
        SlotKind::Any,
        max_stack,
        count,
    );
    if let Some(slot) = inventory.get_mut(from_ref) {
        *slot = source;
    }
    if let Some(slot) = inventory.get_mut(to_ref) {
        *slot = destination;
    }
}

fn equip_gear(
    state: &mut PlayerState,
    backpack_slot: u8,
    content: &ContentRegistry,
    notices: &mut MessageWriter<ToClients<ServerNotice>>,
    client: ClientId,
) {
    let slot = SlotRef::Backpack(backpack_slot);
    let Some(stack) = state.inventory.get(slot).copied() else {
        return;
    };
    if stack.is_empty() {
        return;
    }
    let Some(ItemCategory::Gear {
        slot: gear_slot, ..
    }) = content.item(stack.item).map(|item| &item.category)
    else {
        notices.write(ToClients {
            targets: SendTargets::Single(client),
            message: ServerNotice::Notification {
                text: "That item is not gear".into(),
                severity: 1,
            },
        });
        return;
    };
    let gear_slot = *gear_slot;
    let previous = state.gear.get(gear_slot);
    let new_gear = match gear_slot {
        GearSlot::Suit => &mut state.gear.suit,
        GearSlot::Mask => &mut state.gear.mask,
        GearSlot::Instrument => &mut state.gear.instrument,
    };
    *new_gear = Some(stack.item);
    // Put the previous gear (or an empty stack) back into the backpack slot.
    *state.inventory.get_mut(slot).expect("checked above") =
        previous.map_or(ItemStack::EMPTY, |item| ItemStack::new(item, 1));
}

fn unequip_gear(state: &mut PlayerState, gear_slot: u8, content: &ContentRegistry) {
    let slot = match gear_slot {
        0 => GearSlot::Suit,
        1 => GearSlot::Mask,
        _ => GearSlot::Instrument,
    };
    let Some(item) = state.gear.get(slot) else {
        return;
    };
    if state.inventory.insert(item, 1, content) == 0 {
        match slot {
            GearSlot::Suit => state.gear.suit = None,
            GearSlot::Mask => state.gear.mask = None,
            GearSlot::Instrument => state.gear.instrument = None,
        }
    }
}

/// Puts a freshly mined stack into the inventory, returning what did not fit.
pub fn grant_items(
    state: &mut PlayerState,
    item: ContentId,
    count: u32,
    content: &ContentRegistry,
) -> u32 {
    state.inventory.insert(item, count, content)
}
