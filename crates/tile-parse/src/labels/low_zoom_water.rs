// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The ocean and sea names the parser adds to the coarse zooms itself, with
//! their anchors and their spellings per language, and the projection that
//! puts an anchor into a tile (the Swift `LowZoomWaterLabels`). Data only:
//! `LabelFeatureReader` decides which of them a tile shows.

use std::collections::HashSet;

use map_style::style::WaterNameKind;

use crate::tile::Tile;

/// The layer name the labels' identities are minted under, so they never
/// collide with a tile's own labels.
pub const IDENTITY_NAMESPACE: &str = "natural_label";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Label {
    /// Spellings keyed by language code, the keys
    /// `TileLabelDecisions::localized_name` walks.
    pub names: &'static [(&'static str, &'static str)],
    pub latitude: f64,
    pub longitude: f64,
    pub sort_key: i32,
    pub kind: WaterNameKind,
}

impl Label {
    pub fn aliases(&self) -> impl Iterator<Item = &'static str> {
        self.names.iter().map(|(_, name)| *name).filter(|name| !name.is_empty())
    }

    /// Whether any spelling of the name is already on the tile as a water
    /// label, so the tile's own label wins.
    pub fn is_duplicate(&self, existing_water_text: &HashSet<String>) -> bool {
        self.aliases().any(|alias| existing_water_text.contains(alias))
    }
}

const PACIFIC: Label = Label {
    names: &[
        ("en", "Pacific Ocean"),
        ("ru", "Тихий океан"),
        ("fr", "Océan Pacifique"),
        ("de", "Pazifischer Ozean"),
        ("es", "Océano Pacífico"),
        ("it", "Oceano Pacifico"),
        ("pt", "Oceano Pacífico"),
        ("tr", "Pasifik Okyanusu"),
    ],
    latitude: 0.0,
    longitude: -150.0,
    sort_key: 20,
    kind: WaterNameKind::Ocean,
};

const ATLANTIC: Label = Label {
    names: &[
        ("en", "Atlantic Ocean"),
        ("ru", "Атлантический океан"),
        ("fr", "Océan Atlantique"),
        ("de", "Atlantischer Ozean"),
        ("es", "Océano Atlántico"),
        ("it", "Oceano Atlantico"),
        ("pt", "Oceano Atlântico"),
        ("tr", "Atlas Okyanusu"),
    ],
    latitude: 8.0,
    longitude: -32.0,
    sort_key: 18,
    kind: WaterNameKind::Ocean,
};

const INDIAN: Label = Label {
    names: &[
        ("en", "Indian Ocean"),
        ("ru", "Индийский океан"),
        ("fr", "Océan Indien"),
        ("de", "Indischer Ozean"),
        ("es", "Océano Índico"),
        ("it", "Oceano Indiano"),
        ("pt", "Oceano Índico"),
        ("tr", "Hint Okyanusu"),
    ],
    latitude: -18.0,
    longitude: 80.0,
    sort_key: 22,
    kind: WaterNameKind::Ocean,
};

const ARCTIC: Label = Label {
    names: &[
        ("en", "Arctic Ocean"),
        ("ru", "Северный Ледовитый океан"),
        ("fr", "Océan Arctique"),
        ("de", "Arktischer Ozean"),
        ("es", "Océano Ártico"),
        ("it", "Mar Glaciale Artico"),
        ("pt", "Oceano Ártico"),
        ("tr", "Arktik Okyanusu"),
    ],
    latitude: 76.0,
    longitude: 15.0,
    sort_key: 16,
    kind: WaterNameKind::Ocean,
};

const SOUTHERN: Label = Label {
    names: &[
        ("en", "Southern Ocean"),
        ("ru", "Южный океан"),
        ("fr", "Océan Austral"),
        ("de", "Südlicher Ozean"),
        ("es", "Océano Austral"),
        ("it", "Oceano Australe"),
        ("pt", "Oceano Antártico"),
        ("tr", "Güney Okyanusu"),
    ],
    latitude: -56.0,
    longitude: 25.0,
    sort_key: 24,
    kind: WaterNameKind::Ocean,
};

const MEDITERRANEAN: Label = Label {
    names: &[
        ("en", "Mediterranean Sea"),
        ("ru", "Средиземное море"),
        ("fr", "Mer Méditerranée"),
        ("de", "Mittelmeer"),
        ("es", "Mar Mediterráneo"),
        ("it", "Mar Mediterraneo"),
        ("pt", "Mar Mediterrâneo"),
        ("tr", "Akdeniz"),
    ],
    latitude: 35.0,
    longitude: 18.0,
    sort_key: 30,
    kind: WaterNameKind::Sea,
};

const CARIBBEAN: Label = Label {
    names: &[
        ("en", "Caribbean Sea"),
        ("ru", "Карибское море"),
        ("fr", "Mer des Caraïbes"),
        ("de", "Karibisches Meer"),
        ("es", "Mar Caribe"),
        ("it", "Mar dei Caraibi"),
        ("pt", "Mar do Caribe"),
        ("tr", "Karayip Denizi"),
    ],
    latitude: 15.0,
    longitude: -74.0,
    sort_key: 32,
    kind: WaterNameKind::Sea,
};

const ARABIAN: Label = Label {
    names: &[
        ("en", "Arabian Sea"),
        ("ru", "Аравийское море"),
        ("fr", "Mer d'Arabie"),
        ("de", "Arabisches Meer"),
        ("es", "Mar Arábigo"),
        ("it", "Mar Arabico"),
        ("pt", "Mar Arábico"),
        ("tr", "Umman Denizi"),
    ],
    latitude: 15.0,
    longitude: 64.0,
    sort_key: 34,
    kind: WaterNameKind::Sea,
};

const BERING: Label = Label {
    names: &[
        ("en", "Bering Sea"),
        ("ru", "Берингово море"),
        ("fr", "Mer de Béring"),
        ("de", "Beringmeer"),
        ("es", "Mar de Bering"),
        ("it", "Mare di Bering"),
        ("pt", "Mar de Bering"),
        ("tr", "Bering Denizi"),
    ],
    latitude: 57.0,
    longitude: -178.0,
    sort_key: 36,
    kind: WaterNameKind::Sea,
};

const OCEANS: [Label; 5] = [PACIFIC, ATLANTIC, INDIAN, ARCTIC, SOUTHERN];
const OCEANS_AND_SEAS: [Label; 9] = [PACIFIC, ATLANTIC, INDIAN, ARCTIC, SOUTHERN, MEDITERRANEAN, CARIBBEAN, ARABIAN, BERING];

/// The five oceans at every zoom up to 2, plus four seas at zoom 2.
pub fn labels(tile: Tile) -> &'static [Label] {
    if tile.z == 2 { &OCEANS_AND_SEAS } else { &OCEANS }
}

/// The tile-space anchor of a geographic position, `None` when it falls
/// outside the tile.
pub fn tile_point(latitude: f64, longitude: f64, tile: Tile) -> Option<[i16; 2]> {
    let n = 2f64.powi(tile.z);
    if n <= 0.0 {
        return None;
    }

    // Swift's truncatingRemainder is Rust's `%` on floats.
    let wrapped_longitude = (((longitude + 180.0) % 360.0) + 360.0) % 360.0 - 180.0;
    let x = (wrapped_longitude + 180.0) / 360.0 * n;

    let clamped_latitude = latitude.clamp(-85.05112878, 85.05112878);
    let latitude_radians = clamped_latitude * std::f64::consts::PI / 180.0;
    let y = (1.0 - (latitude_radians.tan() + 1.0 / latitude_radians.cos()).ln() / std::f64::consts::PI) * 0.5 * n;

    let local_x = (x - tile.x as f64) * 4096.0;
    let local_y = (y - tile.y as f64) * 4096.0;
    if !(0.0..=4096.0).contains(&local_x) || !(0.0..=4096.0).contains(&local_y) {
        return None;
    }

    let rounded_x = (local_x.round() as i64).clamp(0, 4096) as i16;
    let rounded_y = (local_y.round() as i64).clamp(0, 4096) as i16;
    Some([rounded_x, rounded_y])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seas_join_the_oceans_at_zoom_two_only() {
        assert_eq!(labels(Tile::new(0, 0, 0)).len(), 5);
        assert_eq!(labels(Tile::new(1, 0, 0)).len(), 5);
        assert_eq!(labels(Tile::new(2, 0, 0)).len(), 9);
    }

    #[test]
    fn the_equator_on_the_meridian_is_the_centre_of_the_world_tile() {
        assert_eq!(tile_point(0.0, 0.0, Tile::new(0, 0, 0)), Some([2048, 2048]));
        assert_eq!(tile_point(0.0, 180.0, Tile::new(0, 0, 0)), Some([0, 2048]), "180 wraps to -180");
    }

    #[test]
    fn a_position_outside_the_tile_has_no_anchor() {
        assert_eq!(tile_point(0.0, 10.0, Tile::new(1, 0, 0)), None);
        assert!(tile_point(0.0, 10.0, Tile::new(1, 1, 0)).is_some());
    }

    #[test]
    fn the_atlantic_falls_in_the_western_half_at_zoom_one() {
        let tile = Tile::new(1, 0, 0);
        let point = tile_point(ATLANTIC.latitude, ATLANTIC.longitude, tile).expect("in the tile");
        // x = (148 / 360) * 2 * 4096, y just above the equator.
        assert_eq!(point[0], ((148.0 / 360.0) * 2.0 * 4096.0f64).round() as i16);
        assert!(point[1] > 3900 && point[1] < 4096);
    }

    #[test]
    fn any_spelling_already_on_the_tile_is_a_duplicate() {
        let mut existing = HashSet::new();
        assert!(!ATLANTIC.is_duplicate(&existing));
        existing.insert("Oceano Atlântico".to_owned());
        assert!(ATLANTIC.is_duplicate(&existing));
        assert!(!PACIFIC.is_duplicate(&existing));
    }
}
