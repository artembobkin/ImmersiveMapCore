// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::fmt;

/// What can be wrong with the bytes of a model tile. Every case names the
/// place it was found, so a tile baked by another version of the baker is
/// diagnosable from the message alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTileFormatError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u32),
    UnsupportedVertexStride(u32),
    Malformed(String),
}

impl fmt::Display for ModelTileFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelTileFormatError::Truncated => f.write_str("the model tile is truncated"),
            ModelTileFormatError::BadMagic => f.write_str("the bytes are not a model tile (bad magic)"),
            ModelTileFormatError::UnsupportedVersion(version) => {
                write!(f, "model tile version {version} is not supported")
            }
            ModelTileFormatError::UnsupportedVertexStride(stride) => {
                write!(f, "a model tile vertex stride of {stride} bytes is not supported")
            }
            ModelTileFormatError::Malformed(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for ModelTileFormatError {}

pub(crate) fn malformed(reason: impl Into<String>) -> ModelTileFormatError {
    ModelTileFormatError::Malformed(reason.into())
}
