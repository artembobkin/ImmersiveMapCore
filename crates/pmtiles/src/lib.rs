// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The PMTiles v3 format as pure functions over bytes: the header, the
//! directory with its lookup, and the Hilbert tile id. No IO and no
//! decompression: the caller fetches the byte ranges and inflates them
//! (in the browser, `fetch` with a Range header and `DecompressionStream`).
//! A port of the Swift `PMTiles` module of ImmersiveMap.

use std::fmt;

pub mod archive;

pub use archive::{ArchiveContents, ArchiveDirectories, ArchiveError, Located};

/// The length of the fixed header at the start of every archive.
pub const HEADER_LENGTH: usize = 127;

/// The largest zoom a tile id can address.
pub const MAXIMUM_ZOOM: u8 = 31;

/// The most entries one directory may declare, a guard against a corrupt count.
pub const MAXIMUM_ENTRY_COUNT: u64 = 10_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    BadMagic,
    UnsupportedVersion(u8),
    UnsupportedCompression(u8),
    Truncated,
    MalformedDirectory(&'static str),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::BadMagic => write!(f, "not a PMTiles archive"),
            FormatError::UnsupportedVersion(v) => write!(f, "PMTiles version {v} is not supported"),
            FormatError::UnsupportedCompression(c) => write!(f, "compression {c} is not supported"),
            FormatError::Truncated => write!(f, "the bytes end before the structure does"),
            FormatError::MalformedDirectory(why) => write!(f, "malformed directory: {why}"),
        }
    }
}

impl std::error::Error for FormatError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Compression {
    Unknown = 0,
    None = 1,
    Gzip = 2,
    Brotli = 3,
    Zstd = 4,
}

impl Compression {
    fn from_byte(byte: u8) -> Result<Self, FormatError> {
        Ok(match byte {
            0 => Compression::Unknown,
            1 => Compression::None,
            2 => Compression::Gzip,
            3 => Compression::Brotli,
            4 => Compression::Zstd,
            other => return Err(FormatError::UnsupportedCompression(other)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TileType {
    Unknown = 0,
    Mvt = 1,
    Png = 2,
    Jpeg = 3,
    Webp = 4,
    Avif = 5,
}

impl TileType {
    fn from_byte(byte: u8) -> Self {
        match byte {
            1 => TileType::Mvt,
            2 => TileType::Png,
            3 => TileType::Jpeg,
            4 => TileType::Webp,
            5 => TileType::Avif,
            _ => TileType::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub root_directory_offset: u64,
    pub root_directory_length: u64,
    pub metadata_offset: u64,
    pub metadata_length: u64,
    pub leaf_directories_offset: u64,
    pub leaf_directories_length: u64,
    pub tile_data_offset: u64,
    pub tile_data_length: u64,
    pub addressed_tile_count: u64,
    pub tile_entry_count: u64,
    pub tile_content_count: u64,
    pub is_clustered: bool,
    pub internal_compression: Compression,
    pub tile_compression: Compression,
    pub tile_type: TileType,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub min_lon_e7: i32,
    pub min_lat_e7: i32,
    pub max_lon_e7: i32,
    pub max_lat_e7: i32,
    pub center_zoom: u8,
    pub center_lon_e7: i32,
    pub center_lat_e7: i32,
}

impl Header {
    pub const MAGIC: &'static [u8; 7] = b"PMTiles";

    pub fn decode(bytes: &[u8]) -> Result<Header, FormatError> {
        if bytes.len() < HEADER_LENGTH {
            return Err(FormatError::Truncated);
        }
        if &bytes[0..7] != Self::MAGIC {
            return Err(FormatError::BadMagic);
        }
        if bytes[7] != 3 {
            return Err(FormatError::UnsupportedVersion(bytes[7]));
        }
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        let i32_at = |o: usize| i32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        Ok(Header {
            root_directory_offset: u64_at(8),
            root_directory_length: u64_at(16),
            metadata_offset: u64_at(24),
            metadata_length: u64_at(32),
            leaf_directories_offset: u64_at(40),
            leaf_directories_length: u64_at(48),
            tile_data_offset: u64_at(56),
            tile_data_length: u64_at(64),
            addressed_tile_count: u64_at(72),
            tile_entry_count: u64_at(80),
            tile_content_count: u64_at(88),
            is_clustered: bytes[96] == 1,
            internal_compression: Compression::from_byte(bytes[97])?,
            tile_compression: Compression::from_byte(bytes[98])?,
            tile_type: TileType::from_byte(bytes[99]),
            min_zoom: bytes[100],
            max_zoom: bytes[101],
            min_lon_e7: i32_at(102),
            min_lat_e7: i32_at(106),
            max_lon_e7: i32_at(110),
            max_lat_e7: i32_at(114),
            center_zoom: bytes[118],
            center_lon_e7: i32_at(119),
            center_lat_e7: i32_at(123),
        })
    }
}

/// The id of tile `z/x/y`: the tiles of every lower zoom, then the tile's
/// distance along the zoom's Hilbert curve. `None` off the zoom's grid.
pub fn tile_id(z: u8, x: u32, y: u32) -> Option<u64> {
    if z > MAXIMUM_ZOOM {
        return None;
    }
    let side = 1u64 << z;
    let (mut tx, mut ty) = (x as u64, y as u64);
    if tx >= side || ty >= side {
        return None;
    }
    // Tiles at all lower zooms: (4^z - 1) / 3.
    let mut accumulator = ((1u64 << (2 * z as u64)) - 1) / 3;
    let mut s = side / 2;
    while s > 0 {
        let rx = tx & s;
        let ry = ty & s;
        accumulator += ((3 * rx) ^ ry) * s;
        if ry == 0 {
            if rx != 0 {
                tx = side - 1 - tx;
                ty = side - 1 - ty;
            }
            std::mem::swap(&mut tx, &mut ty);
        }
        s /= 2;
    }
    Some(accumulator)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub tile_id: u64,
    pub offset: u64,
    pub length: u32,
    pub run_length: u32,
}

impl Entry {
    /// An entry of run length 0 points at a leaf directory, not at a tile.
    pub fn is_leaf_pointer(&self) -> bool {
        self.run_length == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Tile(Entry),
    Leaf(Entry),
    Missing,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Directory {
    pub entries: Vec<Entry>,
}

struct VarintReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl VarintReader<'_> {
    fn read(&mut self) -> Result<u64, FormatError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *self.bytes.get(self.position).ok_or(FormatError::Truncated)?;
            self.position += 1;
            if shift >= 64 {
                return Err(FormatError::MalformedDirectory("varint is too long"));
            }
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }
}

impl Directory {
    /// Decodes a directory's (already decompressed) bytes: the entry count,
    /// then the columns of tile id deltas, run lengths, lengths and offsets.
    pub fn decode(bytes: &[u8]) -> Result<Directory, FormatError> {
        let mut reader = VarintReader { bytes, position: 0 };
        let count = reader.read()?;
        if count > MAXIMUM_ENTRY_COUNT {
            return Err(FormatError::MalformedDirectory("entry count is out of range"));
        }
        let count = count as usize;
        let mut entries = Vec::with_capacity(count);
        let mut last_id = 0u64;
        for _ in 0..count {
            let delta = reader.read()?;
            last_id = last_id
                .checked_add(delta)
                .ok_or(FormatError::MalformedDirectory("tile id overflow"))?;
            entries.push(Entry { tile_id: last_id, offset: 0, length: 0, run_length: 0 });
        }
        for entry in entries.iter_mut() {
            entry.run_length = u32::try_from(reader.read()?)
                .map_err(|_| FormatError::MalformedDirectory("run length out of range"))?;
        }
        for entry in entries.iter_mut() {
            entry.length = u32::try_from(reader.read()?)
                .map_err(|_| FormatError::MalformedDirectory("length out of range"))?;
        }
        for index in 0..count {
            let stored = reader.read()?;
            entries[index].offset = if stored == 0 && index > 0 {
                let previous = entries[index - 1];
                previous.offset + previous.length as u64
            } else if stored == 0 {
                return Err(FormatError::MalformedDirectory("zero offset on the first entry"));
            } else {
                stored - 1
            };
        }
        Ok(Directory { entries })
    }

    pub fn lookup(&self, tile_id: u64) -> Lookup {
        // The last entry whose id is not above the tile's.
        let index = self.entries.partition_point(|e| e.tile_id <= tile_id);
        if index == 0 {
            return Lookup::Missing;
        }
        let entry = self.entries[index - 1];
        if entry.is_leaf_pointer() {
            return Lookup::Leaf(entry);
        }
        if tile_id - entry.tile_id < entry.run_length as u64 {
            Lookup::Tile(entry)
        } else {
            Lookup::Missing
        }
    }
}


/// The leaf directories an archive client has read, held up to a cost limit
/// and evicted least recently used first. A leaf is keyed by its offset,
/// which names it uniquely within one archive. Recency is a tick on each
/// slot: a read stays O(1), and the scan for the oldest runs only when an
/// insertion goes over the limit.
#[derive(Debug, Default)]
pub struct DirectoryCache {
    cost_limit: usize,
    slots: std::collections::HashMap<u64, (Directory, usize, u64)>,
    tick: u64,
    total_cost: usize,
}

impl DirectoryCache {
    /// Leaf directories held in memory: an entry costs about 24 bytes, so
    /// this is hundreds of thousands of entries.
    pub const DEFAULT_COST_LIMIT: usize = 16 * 1024 * 1024;

    pub fn new(cost_limit: usize) -> DirectoryCache {
        DirectoryCache { cost_limit, ..Default::default() }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn total_cost(&self) -> usize {
        self.total_cost
    }

    /// The directory at `offset`, marked as the most recently used.
    pub fn get(&mut self, offset: u64) -> Option<&Directory> {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        let slot = self.slots.get_mut(&offset)?;
        slot.2 = tick;
        Some(&slot.0)
    }

    /// Stores `directory`, then evicts the least recently used until the
    /// total fits. A directory costlier than the whole limit is not kept.
    pub fn insert(&mut self, offset: u64, directory: Directory, cost: usize) {
        if let Some(previous) = self.slots.remove(&offset) {
            self.total_cost -= previous.1;
        }
        if cost > self.cost_limit {
            return;
        }
        self.tick = self.tick.wrapping_add(1);
        self.slots.insert(offset, (directory, cost, self.tick));
        self.total_cost += cost;
        while self.total_cost > self.cost_limit {
            let Some((&oldest, _)) = self.slots.iter().min_by_key(|(_, s)| s.2) else { break };
            if let Some(removed) = self.slots.remove(&oldest) {
                self.total_cost -= removed.1;
            }
        }
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.total_cost = 0;
    }
}

/// One tile for [`write_archive`]: its bytes as they are to be stored
/// (already compressed by the archive's tile compression).
pub struct WriterTile {
    pub z: u8,
    pub x: u32,
    pub y: u32,
    pub data: Vec<u8>,
}

/// The largest root directory a reader finds in its first 16 KiB fetch with the header.
pub const INITIAL_FETCH_BYTES: usize = 16_384;

fn append_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Serializes a clustered archive with one root directory, no leaf
/// directories, uncompressed directories and empty metadata. For fixtures
/// and tools, the counterpart of the Swift `PMTilesWriter`, except that the
/// tiles arrive already compressed (`tile_compression` names how).
pub fn write_archive(mut tiles: Vec<WriterTile>, tile_compression: Compression, tile_type: TileType) -> Result<Vec<u8>, String> {
    if tiles.is_empty() {
        return Err("no tiles to write".into());
    }
    let mut ids = Vec::with_capacity(tiles.len());
    for t in &tiles {
        ids.push(tile_id(t.z, t.x, t.y).ok_or_else(|| format!("tile {}/{}/{} lies outside its zoom", t.z, t.x, t.y))?);
    }
    let mut order: Vec<usize> = (0..tiles.len()).collect();
    order.sort_by_key(|&i| ids[i]);

    let mut directory = Vec::new();
    append_varint(tiles.len() as u64, &mut directory);
    let mut last = 0u64;
    for &i in &order {
        append_varint(ids[i] - last, &mut directory);
        last = ids[i];
    }
    for _ in &order {
        append_varint(1, &mut directory);
    }
    for &i in &order {
        append_varint(tiles[i].data.len() as u64, &mut directory);
    }
    for (n, _) in order.iter().enumerate() {
        // Zero: the tile follows the one before it. Otherwise the offset plus one.
        append_varint(if n == 0 { 1 } else { 0 }, &mut directory);
    }
    if HEADER_LENGTH + directory.len() > INITIAL_FETCH_BYTES {
        return Err(format!("the root directory is {} bytes and leaf directories are not written", directory.len()));
    }
    let metadata = b"{}".to_vec();
    let mut data = Vec::new();
    for &i in &order {
        data.append(&mut tiles[i].data);
    }
    let min_zoom = tiles.iter().map(|t| t.z).min().unwrap();
    let max_zoom = tiles.iter().map(|t| t.z).max().unwrap();

    let root_offset = HEADER_LENGTH as u64;
    let metadata_offset = root_offset + directory.len() as u64;
    let leaf_offset = metadata_offset + metadata.len() as u64;
    let data_offset = leaf_offset;
    let mut out = Vec::with_capacity(data_offset as usize + data.len());
    out.extend_from_slice(Header::MAGIC);
    out.push(3);
    for v in [
        root_offset,
        directory.len() as u64,
        metadata_offset,
        metadata.len() as u64,
        leaf_offset,
        0,
        data_offset,
        data.len() as u64,
        order.len() as u64,
        order.len() as u64,
        order.len() as u64,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&[1, Compression::None as u8, tile_compression as u8, tile_type as u8, min_zoom, max_zoom]);
    for v in [-1_800_000_000i32, -850_511_287, 1_800_000_000, 850_511_287] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.push(min_zoom);
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    debug_assert_eq!(out.len(), HEADER_LENGTH);
    out.extend_from_slice(&directory);
    out.extend_from_slice(&metadata);
    out.extend_from_slice(&data);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_follow_the_hilbert_curve() {
        assert_eq!(tile_id(0, 0, 0), Some(0));
        assert_eq!(tile_id(1, 0, 0), Some(1));
        assert_eq!(tile_id(1, 0, 1), Some(2));
        assert_eq!(tile_id(1, 1, 1), Some(3));
        assert_eq!(tile_id(1, 1, 0), Some(4));
        assert_eq!(tile_id(2, 0, 0), Some(5));
        assert_eq!(tile_id(1, 2, 0), None);
    }

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    #[test]
    fn directory_round_trip_and_lookup() {
        // Entries: tile 5 run 2 at 0 len 10, tile 7 run 1 contiguous len 4, tile 20 leaf at 100 len 50.
        let mut bytes = Vec::new();
        for v in [3, 5, 2, 13] { varint(v, &mut bytes); }
        for v in [2, 1, 0] { varint(v, &mut bytes); }
        for v in [10, 4, 50] { varint(v, &mut bytes); }
        for v in [1, 0, 101] { varint(v, &mut bytes); }
        let dir = Directory::decode(&bytes).unwrap();
        assert_eq!(dir.entries[1].offset, 10);
        assert_eq!(dir.entries[2].offset, 100);
        assert!(matches!(dir.lookup(4), Lookup::Missing));
        assert!(matches!(dir.lookup(6), Lookup::Tile(e) if e.tile_id == 5));
        assert!(matches!(dir.lookup(7), Lookup::Tile(e) if e.tile_id == 7));
        assert!(matches!(dir.lookup(8), Lookup::Missing));
        assert!(matches!(dir.lookup(25), Lookup::Leaf(e) if e.offset == 100));
    }

    #[test]
    fn a_written_archive_reads_back() {
        let tiles = vec![
            WriterTile { z: 1, x: 1, y: 0, data: vec![4, 4, 4] },
            WriterTile { z: 0, x: 0, y: 0, data: vec![0] },
            WriterTile { z: 1, x: 0, y: 1, data: vec![2, 2] },
        ];
        let bytes = write_archive(tiles, Compression::Gzip, TileType::Mvt).unwrap();
        let header = Header::decode(&bytes).unwrap();
        assert_eq!((header.min_zoom, header.max_zoom), (0, 1));
        assert_eq!(header.tile_compression, Compression::Gzip);
        let root = &bytes[header.root_directory_offset as usize..][..header.root_directory_length as usize];
        let directory = Directory::decode(root).unwrap();
        let read = |z, x, y| match directory.lookup(tile_id(z, x, y).unwrap()) {
            Lookup::Tile(e) => bytes[(header.tile_data_offset + e.offset) as usize..][..e.length as usize].to_vec(),
            _ => vec![],
        };
        assert_eq!(read(0, 0, 0), vec![0]);
        assert_eq!(read(1, 0, 1), vec![2, 2]);
        assert_eq!(read(1, 1, 0), vec![4, 4, 4]);
        assert!(read(1, 1, 1).is_empty());
    }

    #[test]
    fn the_directory_cache_evicts_the_least_recently_used() {
        let mut cache = DirectoryCache::new(100);
        cache.insert(1, Directory::default(), 40);
        cache.insert(2, Directory::default(), 40);
        assert!(cache.get(1).is_some());
        cache.insert(3, Directory::default(), 40);
        assert!(cache.get(2).is_none(), "2 was the least recently used");
        assert!(cache.get(1).is_some() && cache.get(3).is_some());
        cache.insert(4, Directory::default(), 101);
        assert!(cache.get(4).is_none(), "costlier than the limit");
        assert_eq!(cache.total_cost(), 80);
    }
}
