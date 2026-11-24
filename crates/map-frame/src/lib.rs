// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The per-frame logic of the map, free of any host: the frame's view (its
//! camera, presentation and tile coverage, `view`), the tile placement
//! planner, the placement of road names along their roads, the label
//! collision solve and the pin collision layout. A host
//! (the browser's Wasm module, the Android engine) calls these with its own
//! values, or through the flat arrays each module documents, which cross a
//! binding without a copy per object.

pub mod collisions;
pub mod pins;
pub mod planner;
pub mod road_labels;
pub mod view;
