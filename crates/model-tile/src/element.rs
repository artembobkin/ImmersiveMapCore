// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::ModelTileContents;

/// The OSM element of a building the map draws on its own, as a model tile
/// names it: its kind and its id. In a tile it is one `u64`, the kind in
/// the top four bits (1 node, 2 way, 3 relation) and the id in the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTileElement {
    Node(u64),
    Way(u64),
    Relation(u64),
}

impl ModelTileElement {
    /// The ids a tile can hold: the bits under the kind.
    pub const ID_LIMIT: u64 = 1 << ModelTileContents::REPLACED_BUILDING_KIND_SHIFT;

    /// The element of a kind as the tile writes it, `None` for a kind the
    /// format has none of.
    pub fn from_kind(kind: u64, id: u64) -> Option<ModelTileElement> {
        match kind {
            1 => Some(ModelTileElement::Node(id)),
            2 => Some(ModelTileElement::Way(id)),
            3 => Some(ModelTileElement::Relation(id)),
            _ => None,
        }
    }

    /// The kind as the tile writes it.
    pub fn kind(&self) -> u64 {
        match self {
            ModelTileElement::Node(_) => 1,
            ModelTileElement::Way(_) => 2,
            ModelTileElement::Relation(_) => 3,
        }
    }

    pub fn id(&self) -> u64 {
        match *self {
            ModelTileElement::Node(id) | ModelTileElement::Way(id) | ModelTileElement::Relation(id) => id,
        }
    }

    /// The element as its entry in the replaced building block.
    pub fn entry(&self) -> u64 {
        self.kind() << ModelTileContents::REPLACED_BUILDING_KIND_SHIFT | self.id()
    }
}
