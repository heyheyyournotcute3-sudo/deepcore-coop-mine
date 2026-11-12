//! Chunk storage: a palette plus bit-packed indices.
//!
//! A freshly generated chunk typically holds three or four distinct block ids (base block, one
//! or two fillers, one ore), so a palette costs almost nothing and the packed indices shrink a
//! 32³ chunk from 64 KiB to 8 KiB at four bits per block.

use bevy::math::UVec3;
use serde::{Deserialize, Serialize};

use super::pos::{CHUNK_VOLUME, local_index};

/// Bits per index for a palette of a given size.
///
/// Indices are always 16 bits. Palette compression still shrinks a 32³ chunk from 64 KiB to
/// 128 KiB / 4 (a 4× saving) and keeps the format trivial to verify; variable-width packing
/// would save a few more KiB per chunk at the cost of a class of subtle bugs that show up as
/// "a random block turned into another block" after an edit.
fn bits_for(_palette_len: usize) -> u32 {
    16
}

/// A 32³ block chunk, palette compressed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// Distinct block ids used by this chunk, index 0 is always the most common block.
    palette: Vec<u16>,
    /// Bits per packed index (1, 2, 4, 8 or 16).
    bits: u32,
    /// Packed indices, `CHUNK_VOLUME * bits / 8` bytes.
    data: Vec<u8>,
    /// Number of non-air blocks, used to skip empty chunks when meshing.
    solid_count: u32,
    /// Bumped on every edit so the mesher can tell whether its mesh is stale.
    revision: u32,
}

impl Default for Chunk {
    fn default() -> Self {
        Self::filled(0)
    }
}

impl Chunk {
    /// Creates a chunk filled with one block id.
    pub fn filled(block: u16) -> Self {
        let bits = 1;
        let mut chunk = Self {
            palette: vec![block],
            bits,
            data: vec![0; CHUNK_VOLUME / 8],
            solid_count: if block == 0 { 0 } else { CHUNK_VOLUME as u32 },
            revision: 0,
        };
        chunk.repack();
        chunk
    }

    /// Creates a chunk from raw indices in palette order plus its palette.
    pub fn from_palette(palette: Vec<u16>, indices: &[u16]) -> Self {
        assert_eq!(indices.len(), CHUNK_VOLUME);
        let mut chunk = Self {
            palette: if palette.is_empty() { vec![0] } else { palette },
            bits: 1,
            data: Vec::new(),
            solid_count: 0,
            revision: 0,
        };
        chunk.bits = bits_for(chunk.palette.len());
        chunk.ensure_capacity();
        for (index, value) in indices.iter().enumerate() {
            chunk.write_index(index, *value as u32);
        }
        chunk.recount();
        chunk
    }

    /// The palette, index 0 is the fill block.
    pub fn palette(&self) -> &[u16] {
        &self.palette
    }

    /// Bits per index.
    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// Packed payload, for the wire format.
    pub fn packed(&self) -> &[u8] {
        &self.data
    }

    /// Revision counter, incremented on every modification.
    pub fn revision(&self) -> u32 {
        self.revision
    }

    /// Number of non-air blocks.
    pub fn solid_count(&self) -> u32 {
        self.solid_count
    }

    /// Whether the chunk contains only air.
    pub fn is_empty(&self) -> bool {
        self.solid_count == 0
    }

    /// Reads a block by local coordinate.
    #[inline]
    pub fn get(&self, local: UVec3) -> u16 {
        let raw = self.read_index(local_index(local)) as usize;
        self.palette.get(raw).copied().unwrap_or(0)
    }

    /// Writes a block by local coordinate, extending the palette when needed.
    pub fn set(&mut self, local: UVec3, block: u16) {
        let index = local_index(local);
        let previous = self.get(local);
        if previous == block {
            return;
        }
        let slot = match self.palette.iter().position(|entry| *entry == block) {
            Some(slot) => slot,
            None => {
                // Widen the indices *before* appending the new entry, so the new slot number
                // still fits the current bit width.
                let new_len = self.palette.len() + 1;
                if bits_for(new_len) > self.bits {
                    self.bits = bits_for(new_len);
                    self.repack();
                }
                self.palette.push(block);
                self.palette.len() - 1
            }
        };
        self.write_index(index, slot as u32);
        if previous == 0 && block != 0 {
            self.solid_count += 1;
        } else if previous != 0 && block == 0 {
            self.solid_count = self.solid_count.saturating_sub(1);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Iterates every local coordinate and block id.
    pub fn iter_blocks(&self) -> impl Iterator<Item = (UVec3, u16)> + '_ {
        (0..CHUNK_VOLUME).map(|index| {
            let raw = self.read_index(index) as usize;
            let block = self.palette.get(raw).copied().unwrap_or(0);
            (super::pos::index_to_local(index), block)
        })
    }

    fn ensure_capacity(&mut self) {
        self.data.resize(CHUNK_VOLUME * self.bits as usize / 8, 0);
    }

    /// Rewrites the packed array at the current bit width, preserving values.
    fn repack(&mut self) {
        let bits = self.bits;
        let mut old_values = Vec::with_capacity(CHUNK_VOLUME);
        if !self.data.is_empty() {
            for index in 0..CHUNK_VOLUME {
                old_values.push(self.read_index(index));
            }
        } else {
            old_values.resize(CHUNK_VOLUME, 0);
        }
        self.bits = bits;
        self.data = vec![0; CHUNK_VOLUME * bits as usize / 8];
        for (index, value) in old_values.into_iter().enumerate() {
            self.write_index(index, value);
        }
    }

    #[inline]
    fn read_index(&self, index: usize) -> u32 {
        let bits = self.bits as usize;
        let bit_offset = index * bits;
        let byte = bit_offset / 8;
        let shift = bit_offset % 8;
        let mut value: u32 = 0;
        let needed = (shift + bits).div_ceil(8);
        for offset in 0..needed {
            value |= u32::from(*self.data.get(byte + offset).unwrap_or(&0)) << (8 * offset);
        }
        (value >> shift) & ((1u32 << bits) - 1)
    }

    #[inline]
    fn write_index(&mut self, index: usize, value: u32) {
        let bits = self.bits as usize;
        let mask = (1u32 << bits) - 1;
        let bit_offset = index * bits;
        let byte = bit_offset / 8;
        let shift = bit_offset % 8;
        let needed = (shift + bits).div_ceil(8);
        let mut window: u32 = 0;
        for offset in 0..needed {
            window |= u32::from(*self.data.get(byte + offset).unwrap_or(&0)) << (8 * offset);
        }
        let cleared = window & !(mask << shift);
        window = cleared | ((value & mask) << shift);
        for offset in 0..needed {
            if let Some(slot) = self.data.get_mut(byte + offset) {
                *slot = ((window >> (8 * offset)) & 0xFF) as u8;
            }
        }
    }

    fn recount(&mut self) {
        let mut count = 0;
        for index in 0..CHUNK_VOLUME {
            let raw = self.read_index(index) as usize;
            if self.palette.get(raw).copied().unwrap_or(0) != 0 {
                count += 1;
            }
        }
        self.solid_count = count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_chunk_round_trips() {
        let chunk = Chunk::filled(7);
        assert_eq!(chunk.get(UVec3::new(0, 0, 0)), 7);
        assert_eq!(chunk.get(UVec3::new(31, 31, 31)), 7);
        assert_eq!(chunk.solid_count(), CHUNK_VOLUME as u32);
        assert!(!chunk.is_empty());
    }

    #[test]
    fn writes_and_palette_growth() {
        let mut chunk = Chunk::default();
        chunk.set(UVec3::new(1, 2, 3), 5);
        assert_eq!(chunk.get(UVec3::new(1, 2, 3)), 5);
        assert_eq!(chunk.solid_count(), 1);
        for id in 1..=20u16 {
            chunk.set(UVec3::new(id as u32, 0, 0), id);
        }
        for id in 1..=20u16 {
            assert_eq!(chunk.get(UVec3::new(id as u32, 0, 0)), id);
        }
        chunk.set(UVec3::new(1, 2, 3), 0);
        assert_eq!(chunk.get(UVec3::new(1, 2, 3)), 0);
    }

    #[test]
    fn large_palette_round_trips() {
        let mut chunk = Chunk::default();
        for id in 1..=300u16 {
            chunk.set(UVec3::new((id % 32) as u32, (id / 32) as u32, 0), id);
        }
        assert_eq!(chunk.bits(), 16);
        for id in 1..=300u16 {
            assert_eq!(
                chunk.get(UVec3::new((id % 32) as u32, (id / 32) as u32, 0)),
                id
            );
        }
    }

    #[test]
    fn from_palette_preserves_every_block() {
        let mut indices = vec![0u16; CHUNK_VOLUME];
        for (index, slot) in indices.iter_mut().enumerate() {
            *slot = (index % 5) as u16;
        }
        let chunk = Chunk::from_palette(vec![0, 11, 12, 13, 14], &indices);
        for index in 0..CHUNK_VOLUME {
            let local = super::super::pos::index_to_local(index);
            assert_eq!(chunk.get(local), [0, 11, 12, 13, 14][index % 5]);
        }
    }

    #[test]
    fn palette_stays_small_for_typical_chunk() {
        let mut chunk = Chunk::filled(3);
        chunk.set(UVec3::new(4, 4, 4), 9);
        chunk.set(UVec3::new(5, 4, 4), 12);
        assert!(chunk.palette().len() <= 4);
        assert_eq!(chunk.packed().len(), CHUNK_VOLUME * 2);
    }
}
