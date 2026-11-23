// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! What a host's worker runs off the main thread, free of the host's types:
//! the tile parse as the settings configure it (`tile_parser`) and the
//! decode of a model tile into what the GPU takes (`model_tile`). The host
//! (a Web Worker over Wasm, a thread of the Android engine) owns the IO and
//! the hand-over to its main thread.

pub mod model_tile;
pub mod tile_parser;
