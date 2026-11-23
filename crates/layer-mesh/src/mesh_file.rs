// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A layer's mesh as one buffer, what the layers' worker hands back and what
//! the disk cache keeps (the Swift `LayerMeshFile`): a header, the table of
//! its chunks, and its four blocks exactly as the GPU takes them, so the
//! main thread uploads the blocks with nothing to parse. Little-endian.
//!
//! The layout, `VERSION` 1:
//!
//! ```text
//! magic "IMLW"            4 bytes
//! version                 u32
//! chunk count             u32
//! 4 x (offset, size)      u64 each: fill vertices, fill indices, line segments, discs
//! chunks                  CHUNK_STRIDE bytes each
//! blocks                  each at a multiple of 16 bytes
//! ```
//!
//! A chunk is x, y, z (i32), the fill index start and count, the line start
//! and count, the join start and count, the point start and count (u32), the
//! box's minimum and maximum and the reach in points (f32). A change to this
//! layout, to a GPU struct or to what the builder makes of a shape is a new
//! `VERSION`.

use std::ops::Range;

use crate::mesh::{Chunk, Disc, FillVertex, LineSegment, MeshData};

pub const MAGIC: [u8; 4] = *b"IMLW";
pub const VERSION: u32 = 1;
pub const BLOCK_COUNT: usize = 4;
pub const HEADER_SIZE: usize = 12 + BLOCK_COUNT * 16;
pub const CHUNK_STRIDE: usize = 3 * 4 + 8 * 4 + 5 * 4;
pub const FILL_VERTEX_STRIDE: usize = 12;
pub const FILL_INDEX_STRIDE: usize = 4;
pub const LINE_SEGMENT_STRIDE: usize = 24;
pub const DISC_STRIDE: usize = 24;

/// Where the blocks lie in a file, and the chunks that draw from them.
#[derive(Clone, Debug, PartialEq)]
pub struct Contents {
    pub chunks: Vec<Chunk>,
    /// Fill vertices, fill indices, line segments, discs.
    pub blocks: [Range<usize>; BLOCK_COUNT],
}

pub fn fill_vertex_bytes(values: &[FillVertex]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * FILL_VERTEX_STRIDE);
    for v in values {
        out.extend_from_slice(&v.position[0].to_le_bytes());
        out.extend_from_slice(&v.position[1].to_le_bytes());
        out.extend_from_slice(&v.color);
    }
    out
}

pub fn fill_index_bytes(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn line_segment_bytes(values: &[LineSegment]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * LINE_SEGMENT_STRIDE);
    for s in values {
        for f in [s.start[0], s.start[1], s.end[0], s.end[1], s.width_points] {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.extend_from_slice(&s.color);
    }
    out
}

pub fn disc_bytes(values: &[Disc]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * DISC_STRIDE);
    for d in values {
        for f in [d.position[0], d.position[1], d.radius_points, d.stroke_width_points] {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.extend_from_slice(&d.color);
        out.extend_from_slice(&d.stroke_color);
    }
    out
}

/// The mesh as one buffer.
pub fn encode(mesh: &MeshData) -> Vec<u8> {
    let blocks = [
        fill_vertex_bytes(&mesh.fill_vertices),
        fill_index_bytes(&mesh.fill_indices),
        line_segment_bytes(&mesh.line_segments),
        disc_bytes(&mesh.discs),
    ];
    let mut offset = HEADER_SIZE + mesh.chunks.len() * CHUNK_STRIDE;
    let mut ranges: Vec<Range<usize>> = Vec::with_capacity(BLOCK_COUNT);
    for block in &blocks {
        offset = offset.div_ceil(16) * 16;
        ranges.push(offset..offset + block.len());
        offset += block.len();
    }
    let mut data = Vec::with_capacity(offset);
    data.extend_from_slice(&MAGIC);
    data.extend_from_slice(&VERSION.to_le_bytes());
    data.extend_from_slice(&(mesh.chunks.len() as u32).to_le_bytes());
    for range in &ranges {
        data.extend_from_slice(&(range.start as u64).to_le_bytes());
        data.extend_from_slice(&(range.len() as u64).to_le_bytes());
    }
    for c in &mesh.chunks {
        for v in [c.x, c.y, c.z] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        for v in [c.fill_index_start, c.fill_index_count, c.line_start, c.line_count, c.join_start, c.join_count, c.point_start, c.point_count] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        for v in [c.bounds_minimum[0], c.bounds_minimum[1], c.bounds_maximum[0], c.bounds_maximum[1], c.maximum_reach_points] {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    for (block, range) in blocks.iter().zip(&ranges) {
        data.resize(range.start, 0);
        data.extend_from_slice(block);
    }
    data
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let slice = self.data.get(self.offset..self.offset + N)?;
        self.offset += N;
        slice.try_into().ok()
    }

    fn u32(&mut self) -> Option<u32> {
        self.bytes::<4>().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.bytes::<8>().map(u64::from_le_bytes)
    }
}

/// How many bytes from a file's start hold its header and its chunks,
/// given its first `HEADER_SIZE` bytes. `None` when they are not a header.
pub fn prefix_size(header: &[u8]) -> Option<usize> {
    let mut reader = Reader { data: header, offset: 0 };
    if reader.bytes::<4>()? != MAGIC || reader.u32()? != VERSION {
        return None;
    }
    let count = reader.u32()?;
    (count < 1 << 24).then(|| HEADER_SIZE + count as usize * CHUNK_STRIDE)
}

/// Reads the header and the chunks of a file of `file_size` bytes from its
/// first bytes. `None` for a file of another version, a cut one, or one
/// whose blocks do not hold what its chunks draw.
pub fn decode(header: &[u8], file_size: usize) -> Option<Contents> {
    let mut reader = Reader { data: header, offset: 0 };
    if reader.bytes::<4>()? != MAGIC || reader.u32()? != VERSION {
        return None;
    }
    let chunk_count = reader.u32()? as usize;
    let mut blocks: [Range<usize>; BLOCK_COUNT] = Default::default();
    for block in &mut blocks {
        let offset = reader.u64()?;
        let size = reader.u64()?;
        if offset > file_size as u64 || size > file_size as u64 - offset {
            return None;
        }
        *block = offset as usize..(offset + size) as usize;
    }
    let mut chunks = Vec::with_capacity(chunk_count.min(1 << 16));
    for _ in 0..chunk_count {
        let x = reader.u32()? as i32;
        let y = reader.u32()? as i32;
        let z = reader.u32()? as i32;
        let mut counts = [0u32; 8];
        for count in &mut counts {
            *count = reader.u32()?;
        }
        let mut floats = [0f32; 5];
        for float in &mut floats {
            *float = f32::from_bits(reader.u32()?);
        }
        chunks.push(Chunk {
            x,
            y,
            z,
            fill_index_start: counts[0],
            fill_index_count: counts[1],
            line_start: counts[2],
            line_count: counts[3],
            join_start: counts[4],
            join_count: counts[5],
            point_start: counts[6],
            point_count: counts[7],
            bounds_minimum: [floats[0], floats[1]],
            bounds_maximum: [floats[2], floats[3]],
            maximum_reach_points: floats[4],
        });
    }
    // A chunk never draws past the end of a block: a damaged file must not
    // send the GPU out of its buffers.
    let fill_indices = blocks[1].len() / FILL_INDEX_STRIDE;
    let lines = blocks[2].len() / LINE_SEGMENT_STRIDE;
    let discs = blocks[3].len() / DISC_STRIDE;
    let within = |start: u32, count: u32, limit: usize| (start as u64 + count as u64) <= limit as u64;
    for c in &chunks {
        if !within(c.fill_index_start, c.fill_index_count, fill_indices)
            || !within(c.line_start, c.line_count, lines)
            || !within(c.join_start, c.join_count, discs)
            || !within(c.point_start, c.point_count, discs)
        {
            return None;
        }
    }
    Some(Contents { chunks, blocks })
}
