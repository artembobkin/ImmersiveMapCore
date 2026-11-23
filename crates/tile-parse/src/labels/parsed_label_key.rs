// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The identity of a parsed label, an FNV-1a style hash (the Swift
//! `ParsedLabelKey`): the layer and the feature id where the tile ships
//! one, otherwise the layer, the text and the geometry. The same feature
//! keys the same across parses, which is what the runtime label cache
//! fades by.
//!
//! Unlike the label policy's `StableHasher`, a string is mixed byte by byte
//! with no length in front, as the Swift engine does: the keys must match.

const LABEL_KEY_SEED: u64 = 1469598103934665603;
const LABEL_KEY_PRIME: u64 = 1099511628211;

pub fn make_point_label_key(text: &str, anchor: [i16; 2], feature_id: u64, has_feature_id: bool, layer_name: &str) -> u64 {
    if has_feature_id {
        return make_layer_feature_label_key(feature_id, layer_name);
    }
    make_fallback_label_key(text, make_point_anchor_hash(anchor), layer_name)
}

pub fn make_road_label_key(text: &str, path: &[[i16; 2]], feature_id: u64, has_feature_id: bool, layer_name: &str) -> u64 {
    if has_feature_id {
        return make_layer_feature_label_key(feature_id, layer_name);
    }
    make_fallback_label_key(text, make_road_path_hash(path), layer_name)
}

fn make_layer_feature_label_key(feature_id: u64, layer_name: &str) -> u64 {
    let mut hash = LABEL_KEY_SEED;
    mix_utf8(&mut hash, layer_name);
    mix(&mut hash, feature_id);
    hash
}

fn make_fallback_label_key(text: &str, geometry_hash: u64, layer_name: &str) -> u64 {
    let mut hash = LABEL_KEY_SEED;
    mix_utf8(&mut hash, layer_name);
    mix_utf8(&mut hash, text);
    mix(&mut hash, geometry_hash);
    hash
}

fn make_point_anchor_hash(anchor: [i16; 2]) -> u64 {
    let mut hash = LABEL_KEY_SEED;
    mix(&mut hash, packed_i16_pair(anchor));
    hash
}

fn make_road_path_hash(path: &[[i16; 2]]) -> u64 {
    let mut hash = LABEL_KEY_SEED;
    mix(&mut hash, path.len() as u64);
    for &point in path {
        mix(&mut hash, packed_i16_pair(point));
    }
    hash
}

fn mix_utf8(hash: &mut u64, string: &str) {
    for byte in string.bytes() {
        mix(hash, byte as u64);
    }
}

#[inline(always)]
fn mix(hash: &mut u64, value: u64) {
    *hash ^= value;
    *hash = hash.wrapping_mul(LABEL_KEY_PRIME);
}

fn packed_i16_pair(point: [i16; 2]) -> u64 {
    let x = point[0] as u16 as u32;
    let y = point[1] as u16 as u32;
    ((x << 16) | y) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv(values: &[u64]) -> u64 {
        let mut hash = LABEL_KEY_SEED;
        for &v in values {
            mix(&mut hash, v);
        }
        hash
    }

    #[test]
    fn a_feature_id_keys_by_layer_and_id_alone() {
        let a = make_point_label_key("A", [1, 2], 42, true, "pois");
        let b = make_point_label_key("B", [3, 4], 42, true, "pois");
        assert_eq!(a, b);
        let expected = fnv(&[b'p' as u64, b'o' as u64, b'i' as u64, b's' as u64, 42]);
        assert_eq!(a, expected);
        assert_ne!(a, make_point_label_key("A", [1, 2], 42, true, "places"));
        assert_eq!(make_road_label_key("x", &[[0, 0], [1, 1]], 42, true, "pois"), a);
    }

    #[test]
    fn without_an_id_the_text_and_the_geometry_key() {
        let a = make_point_label_key("A", [1, 2], 0, false, "pois");
        assert_ne!(a, make_point_label_key("B", [1, 2], 0, false, "pois"));
        assert_ne!(a, make_point_label_key("A", [2, 1], 0, false, "pois"));
        let anchor = fnv(&[(1u64 << 16) | 2]);
        assert_eq!(a, fnv(&[b'p' as u64, b'o' as u64, b'i' as u64, b's' as u64, b'A' as u64, anchor]));
    }

    #[test]
    fn a_negative_coordinate_packs_by_its_bit_pattern() {
        assert_eq!(packed_i16_pair([-1, 0]), 0xFFFF_0000);
        assert_eq!(packed_i16_pair([0, -1]), 0x0000_FFFF);
    }

    #[test]
    fn a_road_path_hash_counts_its_points() {
        let path = [[0, 0], [10, 0]];
        let expected_geometry = fnv(&[2, 0, 10 << 16]);
        assert_eq!(make_road_label_key("R", &path, 0, false, "r"), fnv(&[b'r' as u64, b'R' as u64, expected_geometry]));
        assert_ne!(make_road_label_key("R", &path, 0, false, "r"), make_road_label_key("R", &[[0, 0], [10, 0], [10, 0]], 0, false, "r"));
    }
}
