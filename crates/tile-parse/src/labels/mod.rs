// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The labels of a parse: the contract types a tile hands to the label
//! runtime (`ParsedTextLabel`, `ParsedRoadTextLabel` and their identity,
//! `parsed_label_key`), the point reader, and the ocean and sea names the
//! parser adds to the coarse zooms itself. Ported from the Swift
//! `Tile/Parse/Labels` folder and the label types of `Tile/Parse/Contract`.
//!
//! The contract types serialize to JSON (camelCase) for the TypeScript
//! side. A `u64` key or feature id serializes as a decimal string, since a
//! JavaScript number cannot hold every 64-bit value.

pub(crate) mod contract;
pub mod label_reader;
pub mod low_zoom_water;
pub mod parsed_label_key;

pub use contract::{ParsedRoadTextLabel, ParsedTextLabel};
pub use label_reader::LabelFeatureReader;
