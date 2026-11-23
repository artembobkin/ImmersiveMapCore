// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The model tile as bytes, read (`ModelTileContents`) and written
//! (`ModelTile`, `ModelTileVertex`): one place for the layout, its version
//! and its reference, ported from the Swift engine's `ModelTileFormat`
//! module. The format is a contract with the published archives and the
//! external baker, and the reference is the comment on `ModelTileContents`.
//! It knows nothing of the engine, of WebGPU or of where a model comes
//! from.
//!
//! `astc` decodes a tile's texture array to RGBA8, the fallback for a GPU
//! without ASTC.

pub mod astc;
mod byte_writer;
pub mod contents;
mod element;
mod error;
mod vertex;
pub mod writer;

pub use byte_writer::ByteWriter;
pub use contents::{ModelTileContents, ReplacedBuilding};
pub use element::ModelTileElement;
pub use error::ModelTileFormatError;
pub use vertex::ModelTileVertex;
pub use writer::ModelTile;
