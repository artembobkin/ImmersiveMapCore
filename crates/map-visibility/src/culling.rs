// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The frame's coverage on either surface, with what a host keeps between
//! frames: the version that moves when the coverage changes, and the
//! sphere's tile bounds.

use crate::flat::{self, FlatCoverageInput};
use crate::globe::{self, GlobeBounds, GlobeCoverageInput, GlobeMetrics};
use crate::rules::FlatRingBand;
use crate::{TARGET_STRIDE, Target};

/// What a frame draws: the targets, finest first, and the bands that placed them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VisibleContent {
    pub targets: Vec<Target>,
    /// One band per rule, nearest first.
    pub bands: Vec<FlatRingBand>,
    /// Moves when the targets or their rasters differ from the last frame's,
    /// not when the walk merely ran again: a host's working set gates on it.
    pub coverage_version: u32,
    /// How many tiles the walk looked at, for the diagnostics.
    pub visited: usize,
    /// The sphere's walk in detail, all zero on the plane.
    pub globe_metrics: GlobeMetrics,
}

impl VisibleContent {
    /// The targets as flat records of `TARGET_STRIDE` ints.
    pub fn write_targets(&self, out: &mut Vec<i32>) {
        out.clear();
        out.reserve(self.targets.len() * TARGET_STRIDE);
        for target in &self.targets {
            target.append_to(out);
        }
    }
}

/// The coverage of frame after frame.
#[derive(Debug, Default)]
pub struct TileCulling {
    coverage_version: u32,
    previous: Vec<Target>,
    bounds: GlobeBounds,
}

impl TileCulling {
    pub fn new() -> TileCulling {
        TileCulling::default()
    }

    /// The frame's coverage on the plane. A camera with no footprint draws nothing.
    pub fn resolve_flat(&mut self, input: &FlatCoverageInput<'_>) -> VisibleContent {
        let resolved = flat::coverage(input).unwrap_or_default();
        self.content(resolved.targets, resolved.bands, resolved.visited, GlobeMetrics::default())
    }

    /// The frame's coverage on the sphere.
    pub fn resolve_globe(&mut self, input: &GlobeCoverageInput<'_>) -> VisibleContent {
        let resolved = globe::coverage(input, &mut self.bounds);
        self.content(resolved.targets, resolved.bands, resolved.metrics.visited_nodes, resolved.metrics)
    }

    fn content(&mut self, targets: Vec<Target>, bands: Vec<FlatRingBand>, visited: usize, globe_metrics: GlobeMetrics) -> VisibleContent {
        // Which targets are rasters, and at what size, is part of the
        // coverage: a change of it is a new coverage even over the same
        // targets. The lines and the labels are not.
        let same = targets.len() == self.previous.len() && targets.iter().zip(&self.previous).all(|(a, b)| a.tile == b.tile && a.raster_size == b.raster_size);
        if !same {
            self.coverage_version += 1;
            self.previous.clone_from(&targets);
        }
        VisibleContent { targets, bands, coverage_version: self.coverage_version, visited, globe_metrics }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FlatRenderState;
    use crate::rules::default_rules;

    /// A camera looking straight down whose frustum cuts the plane in the square of ±100 about the origin.
    const DOWN: [f64; 16] = [0.01, 0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 1.0];

    fn input<'a>(rules: &'a [crate::rules::FlatRingRule], target_zoom: i32) -> FlatCoverageInput<'a> {
        FlatCoverageInput { camera_matrix: &DOWN, relief_heights: None, eye: [0.0, 0.0], raster_band_margin_share: 0.0, flat: FlatRenderState { pan: [0.0, 0.0], render_map_size: 1024.0 }, target_zoom, backdrop_zoom: Some(0), rules }
    }

    #[test]
    fn the_coverage_version_moves_only_with_the_coverage() {
        let rules = default_rules();
        let mut culling = TileCulling::new();
        let first = culling.resolve_flat(&input(&rules, 3));
        let again = culling.resolve_flat(&input(&rules, 3));
        assert!(!first.targets.is_empty());
        assert_eq!(again.targets, first.targets);
        assert_eq!(again.coverage_version, first.coverage_version, "the same coverage keeps its version");
        let coarser = culling.resolve_flat(&input(&rules, 2));
        assert_ne!(coarser.targets, first.targets);
        assert_ne!(coarser.coverage_version, first.coverage_version, "a different coverage moves it");
    }

    #[test]
    fn a_camera_with_no_footprint_draws_nothing() {
        let rules = default_rules();
        let mut culling = TileCulling::new();
        culling.resolve_flat(&input(&rules, 3));
        let nothing = culling.resolve_flat(&FlatCoverageInput { camera_matrix: &[0.0; 16], ..input(&rules, 3) });
        assert!(nothing.targets.is_empty() && nothing.bands.is_empty());
    }
}
