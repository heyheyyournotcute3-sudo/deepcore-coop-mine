//! Procedural texture atlas: 16×16 tiles generated from `blocks.ron`.
//!
//! No art is downloaded. Each block's `color`, `pattern` and `emissive` fields are turned into a
//! tile, and the tiles are packed into one atlas image. The atlas layout is deterministic from
//! the block id, so the client and any future tool agree on which tile belongs to which block.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use deepcore_shared::content::{ContentRegistry, Pattern};

/// Tile size in pixels.
pub const TILE: u32 = 16;

/// Tiles per atlas row.
pub const ATLAS_COLUMNS: u32 = 16;

/// The generated atlas.
#[derive(Debug, Resource, Clone)]
pub struct BlockAtlas {
    /// The image handle.
    pub image: Handle<Image>,
    /// The material handle using the atlas.
    pub material: Handle<StandardMaterial>,
    /// Number of tiles.
    pub tiles: u32,
}

impl BlockAtlas {
    /// UV rectangle of a block's tile: `(min, max)`.
    pub fn tile_uv(&self, block: u16) -> (Vec2, Vec2) {
        let index = block as u32;
        let column = index % ATLAS_COLUMNS;
        let row = index / ATLAS_COLUMNS;
        let width = TILE as f32 / (ATLAS_COLUMNS * TILE) as f32;
        let height = TILE as f32 / (ATLAS_COLUMNS * TILE) as f32;
        let min = Vec2::new(column as f32 * width, 1.0 - (row + 1) as f32 * height);
        (min, min + Vec2::new(width, height))
    }
}

/// Builds the atlas image for the given content.
pub fn build_atlas_image(content: &ContentRegistry) -> Image {
    let tile_count = content.blocks.len().max(1) as u32;
    let rows = tile_count.div_ceil(ATLAS_COLUMNS);
    let size = UVec2::new(ATLAS_COLUMNS * TILE, rows.max(1) * TILE);
    let mut data = vec![0u8; (size.x * size.y * 4) as usize];

    for block in &content.blocks {
        let index = block.id.0 as u32;
        let column = index % ATLAS_COLUMNS;
        let row = index / ATLAS_COLUMNS;
        for y in 0..TILE {
            for x in 0..TILE {
                let pixel = shade(block.pattern, block.color, block.emissive, x, y, index);
                let px = column * TILE + x;
                let py = row * TILE + y;
                let offset = ((py * size.x + px) * 4) as usize;
                if offset + 3 < data.len() {
                    data[offset] = pixel[0];
                    data[offset + 1] = pixel[1];
                    data[offset + 2] = pixel[2];
                    data[offset + 3] = 255;
                }
            }
        }
    }

    Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// A deterministic hash for per-tile speckle, so the same block always looks the same.
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut value =
        x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B) ^ seed.wrapping_mul(0xC2B2_AE35);
    value ^= value >> 13;
    value = value.wrapping_mul(0x27D4_EB2D);
    value ^= value >> 15;
    (value & 0xFFFF) as f32 / 65535.0
}

fn shade(
    pattern: Pattern,
    color: (f32, f32, f32),
    emissive: f32,
    x: u32,
    y: u32,
    seed: u32,
) -> [u8; 3] {
    let mut rgb = [color.0, color.1, color.2];
    let noise = hash(x, y, seed);
    let noise2 = hash(x / 2, y / 2, seed ^ 0x1234);
    match pattern {
        Pattern::Solid => {}
        Pattern::Speckle => {
            let factor = 0.85 + noise * 0.3;
            for channel in &mut rgb {
                *channel *= factor;
            }
        }
        Pattern::OreFleck((r, g, b)) => {
            let fleck = noise > 0.72;
            if fleck {
                rgb = [r, g, b];
            } else {
                let factor = 0.9 + noise2 * 0.15;
                for channel in &mut rgb {
                    *channel *= factor;
                }
            }
        }
        Pattern::Grain => {
            let factor = 0.8 + ((y as f32 / TILE as f32) * 0.25) + noise * 0.1;
            for channel in &mut rgb {
                *channel *= factor;
            }
        }
        Pattern::Veins => {
            let vein = f32::from(u8::from((x + y).is_multiple_of(7)));
            for channel in &mut rgb {
                *channel *= 1.0 + vein * 0.2;
            }
        }
        Pattern::Glow => {
            let pulse = 0.7 + noise * 0.3;
            for channel in &mut rgb {
                *channel = (*channel * pulse).min(1.0) + emissive * 0.5;
            }
        }
        Pattern::Bricks => {
            let brick_row = y / 4;
            let offset = (brick_row % 2) * 4;
            let mortar = y.is_multiple_of(4) || (x + offset).is_multiple_of(8);
            if mortar {
                for channel in &mut rgb {
                    *channel *= 0.6;
                }
            } else {
                let factor = 0.9 + noise * 0.15;
                for channel in &mut rgb {
                    *channel *= factor;
                }
            }
        }
        Pattern::Bands => {
            let band = ((y / 3) % 2) as f32;
            for channel in &mut rgb {
                *channel *= 0.85 + band * 0.2 + noise * 0.05;
            }
        }
        Pattern::Fluid => {
            let wave = (x as f32 * 0.6 + noise).sin() * 0.1;
            for channel in &mut rgb {
                *channel = (*channel + wave).clamp(0.0, 1.0);
            }
        }
    }
    [
        (rgb[0].clamp(0.0, 1.0) * 255.0) as u8,
        (rgb[1].clamp(0.0, 1.0) * 255.0) as u8,
        (rgb[2].clamp(0.0, 1.0) * 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn atlas_covers_every_block() {
        let content = content();
        let image = build_atlas_image(&content);
        let size = image.size();
        let rows = (content.blocks.len() as u32).div_ceil(ATLAS_COLUMNS);
        assert_eq!(size.x, ATLAS_COLUMNS * TILE);
        assert_eq!(size.y, rows * TILE);
        assert_eq!(
            image.data.as_ref().map(Vec::len),
            Some((size.x * size.y * 4) as usize)
        );
    }

    #[test]
    fn tile_uvs_are_inside_the_atlas() {
        let content = content();
        let atlas = BlockAtlas {
            image: Handle::default(),
            material: Handle::default(),
            tiles: content.blocks.len() as u32,
        };
        for block in &content.blocks {
            let (min, max) = atlas.tile_uv(block.id.0);
            assert!(min.x >= 0.0 && min.y >= 0.0);
            assert!(max.x <= 1.0 && max.y <= 1.0);
            assert!(max.x > min.x && max.y > min.y);
        }
    }

    #[test]
    fn atlas_is_deterministic() {
        let content = content();
        let a = build_atlas_image(&content);
        let b = build_atlas_image(&content);
        assert_eq!(a.data, b.data);
    }
}
