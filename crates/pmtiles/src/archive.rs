// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! An archive's directories as a client walks them: the header, the root
//! and the leaves read so far, and where a tile is among them. No IO: the
//! host fetches the byte ranges this names and inflates them.

use std::fmt;

use crate::{Compression, Directory, DirectoryCache, FormatError, Header, Lookup, TileType, tile_id};

/// How deep leaf directories may nest before the archive is taken as corrupt.
pub const MAXIMUM_DIRECTORY_DEPTH: usize = 4;

/// The bytes one directory entry costs in the leaf cache.
const ENTRY_COST: usize = 24;

/// What the tiles of an archive must be for a client to read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveContents {
    /// Vector tiles: MVT, or a type the header does not name.
    VectorTiles,
    /// Model tiles: the type PMTiles has no name for.
    ModelTiles,
}

impl ArchiveContents {
    fn accepts(self, tile_type: TileType) -> bool {
        match self {
            ArchiveContents::VectorTiles => tile_type == TileType::Mvt || tile_type == TileType::Unknown,
            ArchiveContents::ModelTiles => tile_type == TileType::Unknown,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ArchiveContents::VectorTiles => "vector tiles",
            ArchiveContents::ModelTiles => "model tiles",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    Format(FormatError),
    /// The archive's tiles are not what the client reads.
    TileType { found: TileType, expected: ArchiveContents },
    /// The client reads gzip and uncompressed archives only.
    Compression(Compression),
    DirectoriesNestTooDeep,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::Format(error) => error.fmt(f),
            ArchiveError::TileType { found, expected } => write!(f, "tile type {found:?} is not {}", expected.name()),
            ArchiveError::Compression(compression) => write!(f, "compression {compression:?} is not supported"),
            ArchiveError::DirectoriesNestTooDeep => write!(f, "PMTiles: leaf directories nest too deep"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<FormatError> for ArchiveError {
    fn from(error: FormatError) -> ArchiveError {
        ArchiveError::Format(error)
    }
}

/// Where a tile is, or which leaf directory must be fetched to find out.
/// The offsets are absolute in the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Located {
    Tile { offset: u64, length: u32 },
    NeedLeaf { offset: u64, length: u32 },
    Missing,
}

pub struct ArchiveDirectories {
    header: Header,
    root: Directory,
    /// Leaf directories by their absolute byte offset in the archive.
    leaves: DirectoryCache,
}

impl ArchiveDirectories {
    /// Reads the header and checks that the archive holds `contents` in a
    /// compression the client reads: gzip or none.
    pub fn new(header_bytes: &[u8], contents: ArchiveContents) -> Result<ArchiveDirectories, ArchiveError> {
        let header = Header::decode(header_bytes)?;
        if !contents.accepts(header.tile_type) {
            return Err(ArchiveError::TileType { found: header.tile_type, expected: contents });
        }
        for compression in [header.internal_compression, header.tile_compression] {
            if !matches!(compression, Compression::None | Compression::Gzip | Compression::Unknown) {
                return Err(ArchiveError::Compression(compression));
            }
        }
        Ok(ArchiveDirectories { header, root: Directory::default(), leaves: DirectoryCache::new(DirectoryCache::DEFAULT_COST_LIMIT) })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The root directory's bytes, inflated.
    pub fn set_root(&mut self, bytes: &[u8]) -> Result<(), FormatError> {
        self.root = Directory::decode(bytes)?;
        Ok(())
    }

    /// A leaf directory's bytes, inflated, read at `absolute_offset`.
    pub fn add_leaf(&mut self, absolute_offset: u64, bytes: &[u8]) -> Result<(), FormatError> {
        let directory = Directory::decode(bytes)?;
        let cost = directory.entries.len() * ENTRY_COST;
        self.leaves.insert(absolute_offset, directory, cost);
        Ok(())
    }

    /// Forgets the leaf directories: the archive was replaced.
    pub fn clear_leaves(&mut self) {
        self.leaves.clear();
    }

    pub fn locate(&mut self, z: u8, x: u32, y: u32) -> Result<Located, ArchiveError> {
        let Some(id) = tile_id(z, x, y) else { return Ok(Located::Missing) };
        let mut lookup = self.root.lookup(id);
        for _ in 0..MAXIMUM_DIRECTORY_DEPTH {
            match lookup {
                Lookup::Tile(entry) => {
                    return Ok(Located::Tile { offset: self.header.tile_data_offset + entry.offset, length: entry.length });
                }
                Lookup::Leaf(entry) => {
                    let offset = self.header.leaf_directories_offset + entry.offset;
                    match self.leaves.get(offset) {
                        Some(leaf) => lookup = leaf.lookup(id),
                        None => return Ok(Located::NeedLeaf { offset, length: entry.length }),
                    }
                }
                Lookup::Missing => return Ok(Located::Missing),
            }
        }
        Err(ArchiveError::DirectoriesNestTooDeep)
    }
}
