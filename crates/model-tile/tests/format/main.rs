// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The model tile format, read and written: the decoder against tiles of an
//! independent fixture writer (`fixture`), the writer against the decoder
//! and the reference. Ported from the Swift `ModelTileFormatTests`.

mod contents;
mod fixture;
mod writer;
