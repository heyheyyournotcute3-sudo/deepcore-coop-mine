//! Content loading as a Bevy plugin: the registry becomes a resource at startup.

use std::path::PathBuf;

use bevy::prelude::*;

pub mod ids;
pub mod model;
pub mod registry;

pub use ids::{ContentId, IdTable};
pub use model::*;
pub use registry::{Block, ContentError, ContentRegistry, Item, Layer, Machine, Recipe, TechNode};

/// Default content directory, relative to the process working directory.
pub const DEFAULT_CONTENT_DIR: &str = "assets/data";

/// Where the content lives, overridable from the command line.
#[derive(Debug, Resource, Clone)]
pub struct ContentDir(pub PathBuf);

impl Default for ContentDir {
    fn default() -> Self {
        Self(PathBuf::from(DEFAULT_CONTENT_DIR))
    }
}

/// Loads the content registry into the app as a resource at startup.
///
/// Panics with the validator message if the content is invalid: content errors are developer
/// errors, and failing fast at startup is the documented behaviour.
pub struct ContentPlugin;

impl Plugin for ContentPlugin {
    fn build(&self, app: &mut App) {
        // Idempotent: binaries add the content plugin directly to control the order and again
        // through `SharedPlugin`, which would otherwise panic on the duplicate.
        if app.world().contains_resource::<ContentRegistry>() {
            return;
        }
        app.add_plugins(ContentRegistryLoader);
    }
}

/// The actual loader, so [`ContentPlugin`] stays idempotent.
struct ContentRegistryLoader;

impl Plugin for ContentRegistryLoader {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<ContentRegistry>() {
            let dir = app
                .world()
                .get_resource::<ContentDir>()
                .cloned()
                .unwrap_or_default();
            let registry = ContentRegistry::load_from_dir(&dir.0).unwrap_or_else(|error| {
                panic!(
                    "\n\nDeepcore content failed to load from {}:\n{error}\n\n",
                    dir.0.display()
                )
            });
            info!(
                "content loaded: {} blocks, {} items, {} recipes, {} machines, {} tech nodes, hash {:016x}",
                registry.blocks.len(),
                registry.items.len(),
                registry.recipes.len(),
                registry.machines.len(),
                registry.tech.len(),
                registry.content_hash
            );
            app.insert_resource(registry);
        }
    }
}
