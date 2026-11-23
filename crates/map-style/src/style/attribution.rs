// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Map data source attribution: what a product using third-party data is
/// obligated to display. It belongs to the data, not the engine: the tile
/// source brings the tiles, so it knows whose data they are and under which
/// license they are served, and the engine adds the credit of the data it
/// ships itself (the overview of the globe).
///
/// The badge is one line of credits along the map's edge, in the order
/// given, each one shown at the camera zooms its data is on screen at. A
/// data set that the map draws through a few zooms only, as the overview of
/// the globe, is credited at those zooms and not past them.
///
/// The data's credits are what the licenses require. The engine's own,
/// `engine_credit`, opens the line by default as a courtesy the MIT license
/// does not ask for.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attribution {
    /// The credits, in the order they are shown on the line.
    pub credits: Vec<AttributionCredit>,
    /// Where a tap on the badge leads. Usually the data source's license
    /// page.
    pub link_url: Option<String>,
}

/// One credit on the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionCredit {
    /// The words, as the data's license asks for them.
    pub text: String,
    /// The camera zooms the credited data is on screen at, a half-open
    /// range of camera zoom (`0.0..8.0` is zoom 0 up to, not including, 8).
    /// `None` shows the credit at every zoom. The badge drops the credit
    /// outside the range, so a credit is shown where its data is and not
    /// where the map draws none of it.
    #[serde(default)]
    pub zoom_range: Option<Range<f64>>,
}

impl AttributionCredit {
    /// A credit at every zoom.
    pub fn new(text: impl Into<String>) -> Self {
        AttributionCredit { text: text.into(), zoom_range: None }
    }

    /// A credit at the camera zooms of a half-open range.
    pub fn in_zooms(text: impl Into<String>, zoom_range: Range<f64>) -> Self {
        AttributionCredit { text: text.into(), zoom_range: Some(zoom_range) }
    }

    /// Whether the credit shows at a camera zoom. An unknown zoom (`None`,
    /// before the camera is placed) shows every credit: the safe side of a
    /// license.
    pub fn is_shown(&self, zoom: Option<f64>) -> bool {
        match (zoom, &self.zoom_range) {
            (Some(zoom), Some(range)) => range.contains(&zoom),
            _ => true,
        }
    }
}

impl Attribution {
    pub fn new(credits: Vec<AttributionCredit>, link_url: Option<String>) -> Self {
        Attribution { credits, link_url }
    }

    /// One credit for every zoom, the common case of one data source.
    pub fn single(text: impl Into<String>, link_url: Option<String>) -> Self {
        Attribution { credits: vec![AttributionCredit::new(text)], link_url }
    }

    /// Empty attribution: the badge draws nothing. The default for sources
    /// whose data source is unknown to the engine. If such a source serves
    /// OSM or other data that requires attribution, its author must set it.
    pub fn none() -> Self {
        Attribution::default()
    }

    /// The engine's own credit, the first on the default badge's line. A
    /// courtesy, not a license term.
    pub fn engine_credit() -> AttributionCredit {
        AttributionCredit::new("Powered by ImmersiveMap")
    }

    /// Minimal attribution for OpenStreetMap data (ODbL).
    pub fn open_street_map() -> Self {
        Attribution::single(
            "© OpenStreetMap contributors",
            Some("https://www.openstreetmap.org/copyright".to_owned()),
        )
    }

    /// No words to show at any zoom.
    pub fn is_empty(&self) -> bool {
        self.credits.iter().all(|credit| credit.text.is_empty())
    }

    /// The credits with words that show at a camera zoom, in order.
    pub fn credits_at_zoom(&self, zoom: Option<f64>) -> Vec<&AttributionCredit> {
        self.credits.iter().filter(|credit| !credit.text.is_empty() && credit.is_shown(zoom)).collect()
    }

    /// The attribution with a credit added at the end of the line.
    pub fn appending(&self, credit: AttributionCredit) -> Attribution {
        let mut attribution = self.clone();
        attribution.credits.push(credit);
        attribution
    }
}
