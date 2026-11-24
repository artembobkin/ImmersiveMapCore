// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The ring rules by camera zoom: any number of sets, each owning the zooms
//! from its first to the next set's, the last one to the deepest zoom. A
//! frame reads the set its zoom falls in, so the globe's zooms and the
//! street zooms take different rules.

use crate::rules::{FlatRingRule, close_street_default_rules, default_rules, globe_default_rules, near_street_default_rules, normalized};

/// The zooms a set may start at.
pub const ZOOM_RANGE: (i32, i32) = (0, 22);
/// The first zoom past the sphere and its unroll under the default presentation settings, which finish the unroll at zoom 7.
pub const DEFAULT_STREET_FIRST_ZOOM: i32 = 7;
/// The first zoom of the near street set, where the camera comes down among the buildings.
pub const DEFAULT_NEAR_STREET_FIRST_ZOOM: i32 = 15;
/// The first zoom of the closest street set.
pub const DEFAULT_CLOSE_STREET_FIRST_ZOOM: i32 = 17;

/// One stretch of zooms and the ring rules it is drawn by.
#[derive(Debug, Clone, PartialEq)]
pub struct RingRuleSet {
    pub first_zoom: i32,
    pub rules: Vec<FlatRingRule>,
}

/// The sets as a frame reads them: first zooms inside the range, sorted, one
/// set per first zoom, the first set starting at zoom 0, every set's rules
/// normalized, at least one set.
#[derive(Debug, Clone, PartialEq)]
pub struct RingRuleSets {
    sets: Vec<RingRuleSet>,
}

impl Default for RingRuleSets {
    /// Four sets. The globe's zooms step down fast and drop their lines
    /// early, since toward the limb a tile is seen edge on. The zooms past
    /// the unroll take the plane's rules. Among the buildings and closer
    /// still the near rings are tighter, and every set past the globe's
    /// draws its far rings from textures.
    fn default() -> RingRuleSets {
        RingRuleSets::new(vec![
            RingRuleSet { first_zoom: 0, rules: globe_default_rules() },
            RingRuleSet { first_zoom: DEFAULT_STREET_FIRST_ZOOM, rules: default_rules() },
            RingRuleSet { first_zoom: DEFAULT_NEAR_STREET_FIRST_ZOOM, rules: near_street_default_rules() },
            RingRuleSet { first_zoom: DEFAULT_CLOSE_STREET_FIRST_ZOOM, rules: close_street_default_rules() },
        ])
    }
}

impl RingRuleSets {
    /// The sets normalized. No set at all is the default sets.
    pub fn new(sets: Vec<RingRuleSet>) -> RingRuleSets {
        let mut cleaned: Vec<RingRuleSet> = sets.into_iter().map(|set| RingRuleSet { first_zoom: set.first_zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1), rules: normalized(&set.rules) }).collect();
        // A stable sort, as the first of two sets at one zoom is the one kept.
        cleaned.sort_by_key(|set| set.first_zoom);
        let mut unique: Vec<RingRuleSet> = Vec::with_capacity(cleaned.len());
        for set in cleaned {
            if unique.last().is_none_or(|last| last.first_zoom < set.first_zoom) {
                unique.push(set);
            }
        }
        if unique.is_empty() {
            return RingRuleSets::default();
        }
        unique[0].first_zoom = ZOOM_RANGE.0;
        RingRuleSets { sets: unique }
    }

    pub fn sets(&self) -> &[RingRuleSet] {
        &self.sets
    }

    /// The index of the set `zoom` falls in.
    pub fn set_index(&self, zoom: i32) -> usize {
        self.sets.iter().rposition(|set| set.first_zoom <= zoom).unwrap_or(0)
    }

    /// The rules a frame at camera zoom `zoom` is drawn by.
    pub fn rules_for_zoom(&self, zoom: i32) -> &[FlatRingRule] {
        &self.sets[self.set_index(zoom)].rules
    }

    /// The zoom a frame's rule set is chosen by: the camera zoom's whole part, within the sets' range.
    pub fn rule_set_zoom(camera_zoom: f64) -> i32 {
        if !camera_zoom.is_finite() {
            return 0;
        }
        (camera_zoom.floor() as i32).clamp(0, ZOOM_RANGE.1)
    }
}

/// The zoom a frame's coverage targets: the camera zoom's whole part, within
/// the tileset's zooms. One zoom on both surfaces: the ring rules step it
/// down by distance, nothing requests ahead of it.
pub fn target_zoom(camera_zoom: f64, maximum_zoom_level: i32) -> i32 {
    (camera_zoom.trunc() as i32).max(0).min(maximum_zoom_level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_reads_the_set_its_zoom_falls_in() {
        let sets = RingRuleSets::default();
        assert_eq!(sets.sets().iter().map(|set| set.first_zoom).collect::<Vec<_>>(), [0, 7, 15, 17]);
        assert_eq!(sets.rules_for_zoom(0), globe_default_rules());
        assert_eq!(sets.rules_for_zoom(6), globe_default_rules());
        assert_eq!(sets.rules_for_zoom(7), default_rules());
        assert_eq!(sets.rules_for_zoom(16), near_street_default_rules());
        assert_eq!(sets.rules_for_zoom(22), close_street_default_rules());
        assert_eq!(RingRuleSets::rule_set_zoom(15.9), 15);
        assert_eq!(RingRuleSets::rule_set_zoom(99.0), 22);
        assert_eq!(RingRuleSets::rule_set_zoom(f64::NAN), 0);
    }

    #[test]
    fn the_sets_are_normalized() {
        let messy = RingRuleSets::new(vec![
            RingRuleSet { first_zoom: 12, rules: default_rules() },
            RingRuleSet { first_zoom: 5, rules: vec![] },
            RingRuleSet { first_zoom: 12, rules: globe_default_rules() },
            RingRuleSet { first_zoom: 40, rules: globe_default_rules() },
        ]);
        assert_eq!(messy.sets().iter().map(|set| set.first_zoom).collect::<Vec<_>>(), [0, 12, 22], "sorted, one per zoom, the first from zoom 0");
        assert_eq!(messy.sets()[0].rules, default_rules(), "a set with no rule takes the default rules");
        assert_eq!(messy.sets()[1].rules, default_rules(), "the first of two sets at one zoom is kept");
        assert_eq!(RingRuleSets::new(vec![]), RingRuleSets::default());
    }

    #[test]
    fn the_target_zoom_is_the_camera_zooms_whole_part_within_the_tileset() {
        assert_eq!(target_zoom(13.9, 15), 13);
        assert_eq!(target_zoom(17.2, 15), 15);
        assert_eq!(target_zoom(-0.5, 15), 0);
    }
}
