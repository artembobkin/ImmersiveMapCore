// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The layers of the app's own data, the CPU side the layers' worker runs:
//! a GeoJSON document read into the flat shapes of a source (`geojson`,
//! decision B11), a layer's shapes tessellated into its GPU form (`mesh`, the
//! Swift `LayerMeshBuilder`) and that form as one buffer (`mesh_file`, the
//! Swift `LayerMeshFile`). Shapes built in code reach the same `mesh::build`
//! through the same flat arrays (`content`).

pub mod content;
pub mod geojson;
pub mod mesh;
pub mod mesh_file;
