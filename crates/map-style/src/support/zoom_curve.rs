// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One stop of a [`ZoomCurve`]: a value at a zoom.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ZoomCurveStop {
    pub zoom: f64,
    pub value: f32,
}

/// How a curve runs between two stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomCurveInterpolation {
    /// A smoothstep of the zoom from one value to the next: it starts and
    /// arrives at rest.
    #[default]
    Smooth,
    /// The same ratio for every step of zoom, the way the map's scale
    /// changes (twice per zoom level): for a length on the ground, so two
    /// stops a few zooms apart follow the map in between. Falls back to
    /// `Smooth` where a stop is not above zero.
    Geometric,
}

/// A value that follows the camera zoom: stops of a value at a zoom, and a
/// smooth blend between neighbouring stops. Below the first stop the value
/// is the first stop's, above the last the last's. A single value is a
/// curve with one stop, the same at every zoom.
///
/// Between two stops the value runs on a smoothstep of the zoom, the curve
/// `ZoomFade` runs on, so it starts and arrives at rest: a zoom gesture
/// crossing a stop sees no kink, and a setting reached at a stop holds
/// there instead of overshooting it. Evaluated against the camera zoom each
/// frame, so a zoom gesture plays the change.
///
/// Serialized as the list of its stops. Deserialized from a number (the
/// same value at every zoom), a list of stops, or a map of zoom to value
/// (`{"13": 4, "14": 1}`).
///
/// Stand-in for the engine's `ImmersiveMapZoomCurve`
/// (`Configuration/ImmersiveMapZoomCurve.swift`).
#[derive(Debug, Clone, PartialEq)]
pub struct ZoomCurve {
    /// The stops, in increasing zoom, never empty.
    stops: Vec<ZoomCurveStop>,
}

impl ZoomCurve {
    /// The same value at every zoom.
    pub fn constant(value: f32) -> Self {
        ZoomCurve { stops: vec![ZoomCurveStop { zoom: 0.0, value }] }
    }

    /// The stops in any order, as `(zoom, value)` pairs. Two stops at one
    /// zoom would be a jump, which the curve does not make.
    ///
    /// # Panics
    ///
    /// When there is no stop, or two stops lie at one zoom.
    pub fn new(stops: impl IntoIterator<Item = (f64, f32)>) -> Self {
        match Self::try_new(stops) {
            Ok(curve) => curve,
            Err(message) => panic!("{message}"),
        }
    }

    /// The stops in any order, or why they make no curve.
    pub fn try_new(stops: impl IntoIterator<Item = (f64, f32)>) -> Result<Self, &'static str> {
        let mut stops: Vec<ZoomCurveStop> =
            stops.into_iter().map(|(zoom, value)| ZoomCurveStop { zoom, value }).collect();
        if stops.is_empty() {
            return Err("A zoom curve needs at least one stop");
        }
        stops.sort_by(|a, b| a.zoom.total_cmp(&b.zoom));
        if stops.windows(2).any(|pair| pair[0].zoom >= pair[1].zoom) {
            return Err("A zoom curve's stops lie at distinct zooms");
        }
        Ok(ZoomCurve { stops })
    }

    /// The stops, in increasing zoom.
    pub fn stops(&self) -> &[ZoomCurveStop] {
        &self.stops
    }

    /// The value at a camera zoom, on a smoothstep between the stops.
    pub fn value(&self, zoom: f64) -> f32 {
        self.value_with(zoom, ZoomCurveInterpolation::Smooth)
    }

    /// The value at a camera zoom.
    pub fn value_with(&self, zoom: f64, interpolation: ZoomCurveInterpolation) -> f32 {
        let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else {
            return 0.0;
        };
        if zoom <= first.zoom {
            return first.value;
        }
        if zoom >= last.zoom {
            return last.value;
        }
        // The segment the zoom lies in: the first stop past it and the one
        // before. A curve holds a handful of stops, so a linear scan.
        let upper = self.stops.iter().position(|stop| stop.zoom > zoom).unwrap_or(self.stops.len() - 1);
        let from = self.stops[upper - 1];
        let to = self.stops[upper];
        let t = (((zoom - from.zoom) / (to.zoom - from.zoom)) as f32).clamp(0.0, 1.0);
        if interpolation == ZoomCurveInterpolation::Geometric && from.value > 0.0 && to.value > 0.0 {
            return from.value * (to.value / from.value).powf(t);
        }
        let eased = t * t * (3.0 - 2.0 * t);
        from.value + (to.value - from.value) * eased
    }

    /// The value, when the curve is the same at every zoom.
    pub fn constant_value(&self) -> Option<f32> {
        if self.stops.len() == 1 { Some(self.stops[0].value) } else { None }
    }
}

impl From<f32> for ZoomCurve {
    fn from(value: f32) -> Self {
        ZoomCurve::constant(value)
    }
}

impl Serialize for ZoomCurve {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.stops.serialize(serializer)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ZoomCurveRepr {
    Constant(f32),
    Stops(Vec<ZoomCurveStop>),
    Pairs(Vec<(f64, f32)>),
    Map(std::collections::BTreeMap<String, f32>),
}

impl<'de> Deserialize<'de> for ZoomCurve {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stops: Vec<(f64, f32)> = match ZoomCurveRepr::deserialize(deserializer)? {
            ZoomCurveRepr::Constant(value) => return Ok(ZoomCurve::constant(value)),
            ZoomCurveRepr::Stops(stops) => stops.into_iter().map(|stop| (stop.zoom, stop.value)).collect(),
            ZoomCurveRepr::Pairs(pairs) => pairs,
            ZoomCurveRepr::Map(map) => {
                let mut stops = Vec::with_capacity(map.len());
                for (zoom, value) in map {
                    let zoom: f64 = zoom.trim().parse().map_err(serde::de::Error::custom)?;
                    stops.push((zoom, value));
                }
                stops
            }
        };
        ZoomCurve::try_new(stops).map_err(serde::de::Error::custom)
    }
}
