//! The HUD: crosshair, hotbar, vitals, depth meter, Geiger counter and notifications.
//!
//! Built from plain Bevy UI nodes and text, which keeps it cheap and always on screen while egui
//! handles the menus and inventories.

use bevy::prelude::*;
use bevy::text::{FontSize, Justify, TextLayout};
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::hazards::geiger_rate;

use crate::input::{LocalInput, Targeting};
use crate::net::SessionState;

/// Registers the HUD.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_hud).add_systems(
            Update,
            (
                update_crosshair,
                update_hotbar,
                update_vitals,
                update_depth,
                update_notifications,
                update_debug_overlay,
            ),
        );
    }
}

/// Text handles for the HUD elements.
#[derive(Debug, Resource)]
pub struct HudState {
    /// Crosshair.
    pub crosshair: Entity,
    /// Vitals text.
    pub vitals: Entity,
    /// Depth text.
    pub depth: Entity,
    /// Hotbar slot labels.
    pub hotbar: Vec<Entity>,
    /// Notification area.
    pub notifications: Entity,
    /// Debug overlay (F3).
    pub debug: Entity,
    /// The "too hard" warning.
    pub warning: Entity,
    /// Whether the debug overlay is visible.
    pub debug_visible: bool,
    /// Seconds since the last Geiger click.
    pub geiger_timer: f32,
}

/// Marker for HUD text that follows the local player.
#[derive(Debug, Component)]
pub struct HudText;

fn setup_hud(mut commands: Commands) {
    let crosshair = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                width: px(14),
                height: px(14),
                margin: UiRect::all(px(-7)),
                border: UiRect::all(px(2)),
                ..default()
            },
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.85)),
            BackgroundColor(Color::NONE),
        ))
        .id();

    let vitals = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(Color::srgba(0.92, 0.95, 1.0, 0.95)),
            Node {
                position_type: PositionType::Absolute,
                left: px(18),
                bottom: px(110),
                ..default()
            },
            HudText,
        ))
        .id();

    let depth = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(Color::srgba(0.85, 0.90, 1.0, 0.9)),
            Node {
                position_type: PositionType::Absolute,
                right: px(18),
                top: px(16),
                ..default()
            },
            TextLayout::justify(Justify::Right),
            HudText,
        ))
        .id();

    let mut hotbar = Vec::new();
    for slot in 0..9 {
        let entity = commands
            .spawn((
                Node {
                    width: px(54),
                    height: px(54),
                    border: UiRect::all(px(2)),
                    margin: UiRect::horizontal(px(2)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.25)),
                BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.55)),
                children![(
                    Text::new(format!("{}", slot + 1)),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(Color::srgba(0.8, 0.85, 0.9, 0.8)),
                )],
            ))
            .id();
        hotbar.push(entity);
    }
    // The hotbar row: one container, with the nine slot nodes as children.
    let row = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(20),
            left: percent(50),
            margin: UiRect::left(px(-260)),
            width: px(520),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .id();
    for entity in &hotbar {
        commands.entity(*entity).insert(ChildOf(row));
    }

    let warning = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(20.0),
                ..default()
            },
            TextColor(Color::srgb(1.0, 0.35, 0.30)),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(58),
                margin: UiRect::left(px(-180)),
                ..default()
            },
            HudText,
        ))
        .id();

    let notifications = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(Color::srgba(0.95, 0.95, 0.9, 0.95)),
            Node {
                position_type: PositionType::Absolute,
                left: px(18),
                top: px(16),
                ..default()
            },
            HudText,
        ))
        .id();

    let debug = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::srgba(0.75, 1.0, 0.8, 0.9)),
            Node {
                position_type: PositionType::Absolute,
                right: px(18),
                bottom: px(18),
                ..default()
            },
            TextLayout::justify(Justify::Right),
            Visibility::Hidden,
            HudText,
        ))
        .id();

    commands.insert_resource(HudState {
        crosshair,
        vitals,
        depth,
        hotbar,
        notifications,
        debug,
        warning,
        debug_visible: false,
        geiger_timer: 0.0,
    });
}

fn update_crosshair(
    targeting: Res<Targeting>,
    hud: Res<HudState>,
    mut borders: Query<&mut BorderColor>,
    mut texts: Query<&mut Text>,
) {
    let colour = if targeting.too_hard {
        Color::srgb(1.0, 0.25, 0.2)
    } else if targeting.block.is_some() {
        Color::srgb(1.0, 1.0, 1.0)
    } else {
        Color::srgba(1.0, 1.0, 1.0, 0.35)
    };
    if let Ok(mut border) = borders.get_mut(hud.crosshair) {
        *border = BorderColor::all(colour);
    }
    if let Ok(mut text) = texts.get_mut(hud.warning) {
        if targeting.too_hard {
            text.0 = format!("Too hard — needs Tier {}", targeting.required_tier);
        } else if !text.0.is_empty() {
            text.0.clear();
        }
    }
}

fn update_hotbar(
    session: Res<SessionState>,
    hud: Res<HudState>,
    content: Res<ContentRegistry>,
    mut texts: Query<&mut Text>,
    mut borders: Query<&mut BorderColor>,
) {
    let Some(inventory) = &session.inventory else {
        return;
    };
    for (slot, entity) in hud.hotbar.iter().enumerate() {
        let stack = inventory.hotbar.get(slot).copied().unwrap_or_default();
        let label = if stack.is_empty() {
            String::new()
        } else {
            let name = content
                .item(stack.item)
                .map_or("?".to_owned(), |item| item.name.clone());
            if stack.count > 1 {
                format!("{name}\n×{}", stack.count)
            } else {
                name
            }
        };
        if let Ok(mut text) = texts.get_mut(*entity) {
            // The slot number lives in the child text; the parent holds the count.
            if text.0 != label && !label.is_empty() {
                text.0 = label;
            }
        }
        if let Ok(mut border) = borders.get_mut(*entity) {
            *border = if slot == inventory.selected as usize {
                BorderColor::all(Color::srgb(1.0, 0.85, 0.3))
            } else {
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.25))
            };
        }
    }
}

fn update_vitals(session: Res<SessionState>, hud: Res<HudState>, mut texts: Query<&mut Text>) {
    let Ok(mut text) = texts.get_mut(hud.vitals) else {
        return;
    };
    let Some(inventory) = &session.inventory else {
        text.0 = "Connecting…".to_owned();
        return;
    };
    let _ = inventory;
    let vitals = &session.tech;
    let _ = vitals;
    let health = session_vitals(&session);
    text.0 = format!(
        "Health  {:.0}\nOxygen  {:.0}\nDose    {:.0}\nSuit    {}",
        health.0, health.1, health.2, health.3
    );
}

/// `(health, oxygen, dose, gear summary)` from the session.
fn session_vitals(session: &SessionState) -> (f32, f32, f32, String) {
    let gear = session
        .inventory
        .as_ref()
        .map(|inventory| format!("{:?}", inventory.gear.suit.is_some()))
        .unwrap_or_else(|| "-".to_owned());
    (session.health(), session.oxygen(), session.dose(), gear)
}

fn update_depth(
    session: Res<SessionState>,
    hud: Res<HudState>,
    content: Res<ContentRegistry>,
    players: Query<&Transform, With<crate::LocalPlayer>>,
    mut texts: Query<&mut Text>,
) {
    let _ = &session;
    let Ok(mut text) = texts.get_mut(hud.depth) else {
        return;
    };
    let Ok(transform) = players.single() else {
        return;
    };
    let depth = deepcore_shared::voxel::SURFACE_Y as f32 - transform.translation.y;
    let layer = if depth < 0.0 {
        "Surface".to_owned()
    } else {
        content
            .layer_at_depth(depth as u32)
            .map(|layer| layer.name.clone())
            .unwrap_or_else(|| "Bedrock".to_owned())
    };
    let geiger = geiger_rate(session.dose_rate());
    text.0 = format!("Depth {depth:.0} m\n{layer}\nGeiger {geiger:.0} c/s");
}

fn update_notifications(
    session: Res<SessionState>,
    hud: Res<HudState>,
    mut texts: Query<&mut Text>,
) {
    let Ok(mut text) = texts.get_mut(hud.notifications) else {
        return;
    };
    let mut lines: Vec<String> = session
        .notifications
        .iter()
        .map(|(message, _, age)| {
            let alpha = (1.0 - age / 6.0).clamp(0.0, 1.0);
            format!("{message}  [{alpha:.0}]")
        })
        .collect();
    for line in session.chat.iter().rev().take(4).rev() {
        lines.push(format!("<{}> {}", line.from, line.text));
    }
    text.0 = lines.join("\n");
}

#[allow(clippy::too_many_arguments)]
fn update_debug_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut hud: ResMut<HudState>,
    session: Res<SessionState>,
    prediction: Res<crate::prediction::PredictionState>,
    chunks: Res<deepcore_shared::voxel::LoadedChunks>,
    mut texts: Query<&mut Text>,
    mut visibility: Query<&mut Visibility>,
    time: Res<Time>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    input: Res<LocalInput>,
) {
    if keys.just_pressed(KeyCode::F3) {
        hud.debug_visible = !hud.debug_visible;
        if let Ok(mut vis) = visibility.get_mut(hud.debug) {
            *vis = if hud.debug_visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if !hud.debug_visible {
        return;
    }
    hud.geiger_timer += time.delta_secs();
    let fps = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed())
        .unwrap_or(0.0);
    let Ok(mut text) = texts.get_mut(hud.debug) else {
        return;
    };
    let stats = session
        .stats
        .as_ref()
        .map(|stats| {
            format!(
                "{} electric / {} steam networks",
                stats.electric.len(),
                stats.steam.len()
            )
        })
        .unwrap_or_else(|| "no power stats".to_owned());
    text.0 = format!(
        "FPS {fps:.0}\nchunks {}\npos {:.1} {:.1} {:.1}\ncorrections {}\n{}\nyaw {:.2} pitch {:.2}",
        chunks.len(),
        prediction.body.position.x,
        prediction.body.position.y,
        prediction.body.position.z,
        prediction.corrections,
        stats,
        input.yaw,
        input.pitch,
    );
}
