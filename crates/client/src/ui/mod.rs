//! egui screens: main menu, inventory, machine UI, tech tree, chat and objectives.
//!
//! Immediate mode is the right tool here: dense slot grids and a zoomable node graph are much
//! less code in egui than in retained-mode UI, and none of it touches the simulation.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use deepcore_shared::content::{ContentId, ContentRegistry, MachineEnergy};
use deepcore_shared::inventory::ItemStack;
use deepcore_shared::protocol::{ClientIntent, ContainerId, SlotAddress};

pub mod browser;

use crate::input::LocalInput;
use crate::net::SessionState;

/// Registers the UI.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiState>()
            .init_resource::<UiIntents>()
            .add_systems(EguiPrimaryContextPass, (draw_ui, capture_pointer).chain())
            .add_systems(Update, flush_ui_intents)
            .add_plugins(browser::BrowserPlugin)
            .add_systems(Update, (apply_ui_state, handle_objectives));
    }
}

/// Which screen is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// Playing, HUD only.
    #[default]
    Playing,
    /// Main menu.
    MainMenu,
    /// Inventory and crafting.
    Inventory,
    /// A machine's slots.
    Machine,
    /// The tech tree.
    TechTree,
}

/// Client-side UI state.
#[derive(Debug, Resource)]
pub struct UiState {
    /// The open screen.
    pub screen: Screen,
    /// The machine screen position, when `Screen::Machine`.
    pub machine_pos: Option<deepcore_shared::voxel::BlockPos>,
    /// Chat input buffer.
    pub chat_buffer: String,
    /// Whether the chat box has focus.
    pub chat_open: bool,
    /// Tech tree zoom.
    pub tech_zoom: f32,
    /// Objectives completed by this client, for the guided list.
    pub objectives_done: Vec<usize>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            screen: Screen::Playing,
            machine_pos: None,
            chat_buffer: String::new(),
            chat_open: false,
            tech_zoom: 1.0,
            objectives_done: Vec::new(),
        }
    }
}

/// The guided objective list for the first hour.
pub const OBJECTIVES: &[(&str, &str)] = &[
    (
        "Punch down to the Shale Beds",
        "Mine 16 blocks of dirt below your feet.",
    ),
    (
        "Collect coal, copper and iron",
        "Look for dark seams and green or reddish ore in stone.",
    ),
    (
        "Build a Workbench",
        "Open the inventory and hand-craft a Workbench Kit.",
    ),
    (
        "Research Hand Tools",
        "Deliver timber at the Research Desk.",
    ),
    ("Fire clay into bricks", "Build a Brick Kiln and burn clay."),
    (
        "Smelt your first iron ingot",
        "Build a Stone Furnace and burn coal.",
    ),
    ("Build an Anvil", "Forge iron plates and rods."),
    (
        "Build a Blast Furnace",
        "Make steel from iron, coal and limestone.",
    ),
    (
        "Forge the Hardened Steel Pickaxe",
        "Steel opens the Granite Depths.",
    ),
];

fn capture_pointer(
    mut contexts: EguiContexts,
    mut input: ResMut<LocalInput>,
    mut ui: ResMut<UiState>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let Ok(context) = contexts.ctx_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyE) {
        ui.screen = if ui.screen == Screen::Inventory {
            Screen::Playing
        } else {
            Screen::Inventory
        };
    }
    if keys.just_pressed(KeyCode::KeyT) {
        ui.screen = if ui.screen == Screen::TechTree {
            Screen::Playing
        } else {
            Screen::TechTree
        };
    }
    if keys.just_pressed(KeyCode::Enter) {
        ui.chat_open = true;
    }
    if keys.just_pressed(KeyCode::Escape) && ui.chat_open {
        ui.chat_open = false;
    }
    let wants_pointer = context.is_pointer_over_egui() || ui.screen != Screen::Playing;
    input.ui_open = wants_pointer;
}

/// Sends the intents the UI queued this frame.
#[derive(Debug, Default, Resource)]
pub struct UiIntents {
    /// Queued intents.
    pub intents: Vec<ClientIntent>,
}

/// Drains the UI's queued intents onto the network.
fn flush_ui_intents(mut ui: ResMut<UiIntents>, mut writer: MessageWriter<ClientIntent>) {
    for intent in ui.intents.drain(..) {
        writer.write(intent);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_ui(
    mut contexts: EguiContexts,
    mut ui: ResMut<UiState>,
    mut intents: ResMut<UiIntents>,
    session: Res<SessionState>,
    content: Res<ContentRegistry>,
    mut input: ResMut<LocalInput>,
    mut browser: ResMut<browser::ServerBrowser>,
    mut join_address: Local<String>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    if ui.screen == Screen::MainMenu {
        draw_main_menu(ctx, &mut ui, &session, &mut browser, &mut join_address);
        return;
    }

    // Crosshair-adjacent info is Bevy UI; egui only draws screens.
    if ui.screen == Screen::Inventory {
        draw_inventory(ctx, &mut ui, &session, &content, &mut intents);
    }
    if ui.screen == Screen::TechTree {
        draw_tech_tree(ctx, &mut ui, &session, &content, &mut intents);
    }
    if ui.screen == Screen::Machine {
        draw_machine(ctx, &mut ui, &session, &content, &mut intents);
    }
    if ui.chat_open {
        draw_chat(ctx, &mut ui, &session, &mut intents);
    }
    draw_objectives(ctx, &session);

    // Only a real screen (inventory, machine, tech tree, chat) takes the pointer away from the
    // crosshair. The objectives panel is display-only and never grabs input.
    let screen_open = ui.screen != Screen::Playing || ui.chat_open;
    input.captured = !screen_open && !ctx.is_pointer_over_egui();
}

fn draw_main_menu(
    ctx: &mut egui::Context,
    ui: &mut UiState,
    session: &SessionState,
    browser: &mut browser::ServerBrowser,
    join_address: &mut String,
) {
    egui::Window::new("Deepcore")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, -40.0])
        .collapsible(false)
        .resizable(true)
        .show(ctx, |panel| {
            panel.heading("DEEPCORE");
            panel.label("A co-op mine from the topsoil to the mantle.");
            panel.add_space(10.0);

            panel.horizontal(|row| {
                row.label("Address");
                row.text_edit_singleline(join_address);
                if row.button("Join by IP").clicked() {
                    browser.join_request = Some(join_address.clone());
                }
            });
            if panel.button("Host game (LAN)").clicked() {
                ui.screen = Screen::Playing;
            }

            panel.separator();
            if browser.enabled() {
                panel.label(format!("Lobby: {}", browser.lobby_url));
                if let Some(user) = &browser.username {
                    panel.label(format!("Logged in as {user}"));
                } else {
                    panel.horizontal(|row| {
                        row.label("Account");
                        row.text_edit_singleline(&mut ui.chat_buffer);
                        if row.button("Log in").clicked() {
                            let password = ui.chat_buffer.clone();
                            browser.login("player", &password);
                        }
                    });
                }
                if panel.button("Refresh servers").clicked() {
                    browser.fetch_servers();
                }
                for server in &browser.servers {
                    panel.horizontal(|row| {
                        row.label(format!(
                            "{}  ({}/{}){}",
                            server.name,
                            server.players,
                            server.max_players,
                            if server.requires_account {
                                "  [account]"
                            } else {
                                ""
                            }
                        ));
                        if row.button("Join").clicked() {
                            browser.join_request = Some(server.address.clone());
                        }
                    });
                }
                panel.label(&browser.status);
            } else {
                panel.label("No lobby configured — join by IP or host a LAN game.");
            }

            if let Some(error) = &session.error {
                panel.colored_label(egui::Color32::RED, error);
            }
        });
}

fn draw_inventory(
    ctx: &mut egui::Context,
    ui: &mut UiState,
    session: &SessionState,
    content: &ContentRegistry,
    intents: &mut UiIntents,
) {
    let Some(inventory) = &session.inventory else {
        return;
    };
    egui::Window::new("Inventory")
        .resizable(false)
        .show(ctx, |panel| {
            panel.label("Hotbar");
            panel.horizontal(|row| {
                for slot in 0..9 {
                    slot_button(
                        row,
                        &inventory.hotbar[slot],
                        content,
                        slot == inventory.selected as usize,
                        |clicked| {
                            if clicked {
                                intents
                                    .intents
                                    .push(ClientIntent::SelectSlot { slot: slot as u8 });
                            }
                        },
                    );
                }
            });
            panel.separator();
            panel.label("Backpack");
            for row_index in 0..6 {
                panel.horizontal(|row| {
                    for column in 0..6 {
                        let slot = row_index * 6 + column;
                        let stack = inventory.backpack.get(slot).copied().unwrap_or_default();
                        slot_button(row, &stack, content, false, |clicked| {
                            if clicked {
                                intents.intents.push(ClientIntent::MoveItems {
                                    from: SlotAddress {
                                        container: ContainerId::Backpack,
                                        index: slot as u8,
                                    },
                                    to: SlotAddress {
                                        container: ContainerId::Hotbar,
                                        index: 0,
                                    },
                                    count: None,
                                });
                            }
                        });
                    }
                });
            }
            panel.separator();
            panel.label("Crafting (workbench recipes)");
            let hand = content.hand_machine;
            if let Some(recipes) = content.recipes_by_machine.get(&hand) {
                for recipe_id in recipes {
                    let Some(recipe) = content.recipe(*recipe_id) else {
                        continue;
                    };
                    let locked = recipe
                        .tech
                        .is_some_and(|tech| !session.tech.researched.contains(&tech));
                    let inputs: Vec<String> = recipe
                        .inputs
                        .iter()
                        .map(|(item, count)| {
                            format!(
                                "{} ×{}",
                                content.item(*item).map_or("?", |item| item.name.as_str()),
                                count
                            )
                        })
                        .collect();
                    let outputs: Vec<String> = recipe
                        .outputs
                        .iter()
                        .map(|(item, count)| {
                            format!(
                                "{} ×{}",
                                content.item(*item).map_or("?", |item| item.name.as_str()),
                                count
                            )
                        })
                        .collect();
                    panel.horizontal(|row| {
                        let label = format!("{} → {}", inputs.join(" + "), outputs.join(" + "));
                        if row.add_enabled(!locked, egui::Button::new(label)).clicked() {
                            intents.intents.push(ClientIntent::Craft {
                                recipe: *recipe_id,
                                count: 1,
                            });
                        }
                        if locked {
                            row.label("locked");
                        }
                    });
                }
            }
            panel.separator();
            panel.label("Gear");
            panel.horizontal(|row| {
                row.label(format!(
                    "Suit: {}  Mask: {}  Tool: {}",
                    gear_name(content, inventory.gear.suit),
                    gear_name(content, inventory.gear.mask),
                    gear_name(content, inventory.gear.instrument),
                ));
            });
            if panel.button("Close").clicked() {
                ui.screen = Screen::Playing;
            }
        });
}

fn slot_button(
    row: &mut egui::Ui,
    stack: &ItemStack,
    content: &ContentRegistry,
    selected: bool,
    mut on_click: impl FnMut(bool),
) {
    let label = if stack.is_empty() {
        "  ".to_owned()
    } else {
        let name = content
            .item(stack.item)
            .map_or("?", |item| item.name.as_str());
        if stack.count > 1 {
            format!("{name}\n{}", stack.count)
        } else {
            name.to_owned()
        }
    };
    let button = egui::Button::new(label).min_size(egui::vec2(64.0, 48.0));
    let button = if selected {
        button.fill(egui::Color32::from_rgb(70, 60, 20))
    } else {
        button
    };
    if row.add(button).clicked() {
        on_click(true);
    }
}

fn gear_name(content: &ContentRegistry, item: Option<ContentId>) -> String {
    item.and_then(|item| content.item(item))
        .map_or_else(|| "-".to_owned(), |item| item.name.clone())
}

fn draw_machine(
    ctx: &mut egui::Context,
    ui: &mut UiState,
    session: &SessionState,
    content: &ContentRegistry,
    intents: &mut UiIntents,
) {
    let Some(screen) = &session.machine_screen else {
        return;
    };
    let machine_name = content
        .machine(screen.machine)
        .map_or("Machine", |machine| machine.name.as_str());
    egui::Window::new(machine_name)
        .resizable(false)
        .show(ctx, |panel| {
            panel.label(format!("State: {}", screen.idle));
            panel.add(egui::ProgressBar::new(screen.progress).show_percentage());
            if let Some(machine) = content.machine(screen.machine) {
                panel.label(match machine.energy {
                    MachineEnergy::Fuel { slots } => format!("Fuel machine ({slots} slot)"),
                    MachineEnergy::Steam { units_per_sec } => {
                        format!("Steam: {units_per_sec:.1}/s")
                    }
                    MachineEnergy::Electric { kw } => format!("Electric: {kw:.0} kW"),
                    MachineEnergy::Manual => "Manual".to_owned(),
                });
            }
            panel.separator();
            panel.label("Inputs");
            panel.horizontal(|row| {
                for (index, stack) in screen.inputs.iter().enumerate() {
                    slot_button(row, stack, content, false, |clicked| {
                        if clicked {
                            intents.intents.push(ClientIntent::MoveItems {
                                from: SlotAddress {
                                    container: ContainerId::MachineInput,
                                    index: index as u8,
                                },
                                to: SlotAddress {
                                    container: ContainerId::Backpack,
                                    index: 0,
                                },
                                count: None,
                            });
                        }
                    });
                }
            });
            panel.label("Fuel");
            panel.horizontal(|row| {
                for (index, stack) in screen.fuel.iter().enumerate() {
                    slot_button(row, stack, content, false, |_| {
                        let _ = index;
                    });
                }
            });
            panel.label("Outputs");
            panel.horizontal(|row| {
                for (index, stack) in screen.outputs.iter().enumerate() {
                    slot_button(row, stack, content, false, |clicked| {
                        if clicked {
                            intents.intents.push(ClientIntent::MoveItems {
                                from: SlotAddress {
                                    container: ContainerId::MachineOutput,
                                    index: index as u8,
                                },
                                to: SlotAddress {
                                    container: ContainerId::Backpack,
                                    index: 0,
                                },
                                count: None,
                            });
                        }
                    });
                }
            });
            if panel.button("Close").clicked() {
                ui.screen = Screen::Playing;
                intents.intents.push(ClientIntent::CloseMachine);
            }
        });
}

fn draw_tech_tree(
    ctx: &mut egui::Context,
    ui: &mut UiState,
    session: &SessionState,
    content: &ContentRegistry,
    intents: &mut UiIntents,
) {
    egui::Window::new("Tech Tree")
        .default_size([720.0, 520.0])
        .resizable(true)
        .show(ctx, |panel| {
            panel.horizontal(|row| {
                row.label("Zoom");
                row.add(egui::Slider::new(&mut ui.tech_zoom, 0.5..=2.0));
            });
            panel.separator();
            egui::ScrollArea::vertical().show(panel, |scroll| {
                for era in 0..=5u8 {
                    let nodes: Vec<&deepcore_shared::content::TechNode> =
                        content.tech.iter().filter(|node| node.era == era).collect();
                    if nodes.is_empty() {
                        continue;
                    }
                    scroll.label(egui::RichText::new(format!("Era {era}")).strong());
                    for node in nodes {
                        let researched = session.tech.researched.contains(&node.id);
                        let available = node
                            .parents
                            .iter()
                            .all(|parent| session.tech.researched.contains(parent));
                        let colour = if researched {
                            egui::Color32::from_rgb(80, 160, 90)
                        } else if available {
                            egui::Color32::from_rgb(200, 180, 80)
                        } else {
                            egui::Color32::from_rgb(110, 110, 120)
                        };
                        scroll.horizontal(|row| {
                            row.colored_label(colour, &node.name);
                            let cost: Vec<String> = node
                                .cost
                                .iter()
                                .map(|(item, count)| {
                                    format!(
                                        "{} ×{}",
                                        content.item(*item).map_or("?", |item| item.name.as_str()),
                                        count
                                    )
                                })
                                .collect();
                            row.label(cost.join(", "));
                            if !researched && available && row.button("Deposit all").clicked() {
                                intents.intents.push(ClientIntent::ResearchDeposit {
                                    node: node.id,
                                    items: node.cost.clone(),
                                });
                            }
                        });
                    }
                }
            });
        });
}

fn draw_chat(
    ctx: &mut egui::Context,
    ui: &mut UiState,
    session: &SessionState,
    intents: &mut UiIntents,
) {
    egui::Window::new("Chat")
        .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -12.0])
        .collapsible(false)
        .title_bar(false)
        .resizable(false)
        .show(ctx, |panel| {
            egui::ScrollArea::vertical()
                .max_height(90.0)
                .stick_to_bottom(true)
                .show(panel, |scroll| {
                    for line in session.chat.iter().rev().take(8).rev() {
                        scroll.label(format!("<{}> {}", line.from, line.text));
                    }
                });
            panel.horizontal(|row| {
                let response = row.text_edit_singleline(&mut ui.chat_buffer);
                if response.lost_focus() && row.input(|input| input.key_pressed(egui::Key::Enter)) {
                    let text = std::mem::take(&mut ui.chat_buffer);
                    if !text.trim().is_empty() {
                        intents.intents.push(ClientIntent::Chat { text });
                    }
                }
            });
        });
}

fn draw_objectives(ctx: &mut egui::Context, session: &SessionState) {
    egui::Window::new("Objectives")
        .anchor(egui::Align2::RIGHT_TOP, [-12.0, 90.0])
        .resizable(false)
        .title_bar(false)
        .interactable(false)
        .show(ctx, |panel| {
            for (index, (title, hint)) in OBJECTIVES.iter().enumerate() {
                let done = index < session_objective_progress(session);
                let marker = if done { "✔" } else { "•" };
                panel.label(format!("{marker} {title}"));
                if !done && index == session_objective_progress(session) {
                    panel.small(*hint);
                }
            }
        });
}

/// How many objectives the session's progress implies.
fn session_objective_progress(session: &SessionState) -> usize {
    session.tech.researched.len().min(OBJECTIVES.len())
}

fn apply_ui_state(mut ui: ResMut<UiState>, session: Res<SessionState>) {
    if ui.screen == Screen::Machine && session.machine_screen.is_none() {
        ui.screen = Screen::Playing;
    }
}

fn handle_objectives() {}
