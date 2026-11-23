// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The tile's screen labels laid out (the Swift `TileTextLabelsBuilder`):
//! each label's text wrapped into its box, a POI's icon beside its name,
//! a road's route signs side by side with their numbers on their plates,
//! then every label's quads grouped into runs by the style they draw with.

use std::cmp::Ordering;
use std::collections::HashMap;

use map_style::style::{LabelFontWeight, LabelTextStyle, PoiSpriteIcon, RouteShieldShape, RouteShieldStyle};
use text_layout::{LabelLayoutRequest, LabelTextAlignment, LabelVertex, LabelWrapOptions, TextLayout, TextMetrics};

use super::poi_sprite_atlas::{PoiSpriteAtlasLayout, disc_scale};
use super::{LabelRun, RouteShieldRunStyle, TextLabelPlacementInput, TextLabelSet};
use crate::coordinate_space::TILE_EXTENT_F64;
use crate::labels::ParsedTextLabel;

/// A label wraps at the base width to at most this many lines.
const BASE_LABEL_WRAP_LINE_COUNT: usize = 3;
/// A label whose text does not fit the base lines re-wraps wider and
/// taller instead of running its tail onto one long last line: the box
/// grows by this factor and may run to six lines.
const EXTENDED_LABEL_WRAP_LINE_COUNT: usize = 6;
const EXTENDED_LABEL_WRAP_WIDTH_FACTOR: f32 = 1.5;
/// The label box's width in ems at the base wrap.
const BASE_LABEL_WRAP_WIDTH_EMS: f32 = 10.0;
/// About what a lowercase letter advances, in ems: a tracked name's box
/// widens by its tracking over it.
const AVERAGE_ADVANCE_EMS: f32 = 0.55;
/// How large a POI label draws next to the plain text labels: one factor
/// scales its type, its icon disc and the gap between them together.
const POI_COMBINED_LABEL_SCALE: f32 = 1.05;

/// One label laid out, before the set groups the runs (the Swift `BuiltBaseLabel`).
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltBaseLabel {
    pub placement_input: TextLabelPlacementInput,
    pub style: LabelTextStyle,
    pub text_vertices: Vec<LabelVertex>,
    pub icon_vertices: Vec<LabelVertex>,
    /// A route sign label's numbers, each in its sign's colour, beside
    /// `text_vertices`, which such a label leaves empty.
    pub route_shield_glyphs: Vec<(LabelTextStyle, Vec<LabelVertex>)>,
    /// A route sign label's plates, one quad per sign.
    pub route_shield_plates: Vec<(RouteShieldRunStyle, Vec<LabelVertex>)>,
}

/// Wraps a label at the base box, and again at the extended box when its
/// text overflows the base lines (the last line then takes the remainder,
/// so the laid-out width exceeds the box). A tracked name wraps at a box
/// widened by its tracking, so it keeps the lines its letters alone make.
pub fn wrapped_label_metrics(layout: &TextLayout, text: &str, label_index: i32, text_scale: f32, weight: LabelFontWeight, letter_spacing_em: f32) -> TextMetrics {
    let spacing_widening = 1.0 + letter_spacing_em.max(0.0) / AVERAGE_ADVANCE_EMS;
    let base_width = text_scale * BASE_LABEL_WRAP_WIDTH_EMS * spacing_widening;
    let request = LabelLayoutRequest::new(label_index, text_scale, weight).letter_spacing(letter_spacing_em);
    let base = layout.label_vertices(text, &request.wrap(LabelWrapOptions::new(base_width, BASE_LABEL_WRAP_LINE_COUNT, LabelTextAlignment::Left)));
    // A line a hair over the box is the wrapper's own rounding, not an overflow worth a second pass.
    if base.width <= base_width * 1.02 {
        return base;
    }
    let extended = LabelWrapOptions::new(base_width * EXTENDED_LABEL_WRAP_WIDTH_FACTOR, EXTENDED_LABEL_WRAP_LINE_COUNT, LabelTextAlignment::Left);
    layout.label_vertices(text, &request.wrap(extended))
}

/// The screen labels of a tile, sorted by the zoom they show from, then
/// collision priority, then sort key, so the labels a camera zoom shows
/// are a run at the head of the tile's.
pub fn build(layout: &TextLayout, labels: &[&ParsedTextLabel]) -> TextLabelSet {
    let atlas = PoiSpriteAtlasLayout::default();
    let mut sorted: Vec<(usize, &ParsedTextLabel)> = labels.iter().copied().enumerate().collect();
    sorted.sort_by(|(ia, a), (ib, b)| {
        a.min_camera_zoom
            .partial_cmp(&b.min_camera_zoom)
            .unwrap_or(Ordering::Equal)
            .then(a.collision_priority.cmp(&b.collision_priority))
            .then(a.sort_key.cmp(&b.sort_key))
            .then(ia.cmp(ib))
    });
    let mut built = Vec::with_capacity(sorted.len());
    for (sorted_index, (_, label)) in sorted.into_iter().enumerate() {
        let label_index = sorted_index as i32;
        let placement = |size: [f32; 2]| TextLabelPlacementInput {
            uv: [(label.position[0] as f64 / TILE_EXTENT_F64) as f32, (label.position[1] as f64 / TILE_EXTENT_F64) as f32],
            roof_height: label.roof_height,
            lifts_to_roof: label.lifts_to_roof,
            key: label.key,
            sort_key: label.sort_key,
            collision_priority: label.collision_priority,
            label_size_points: size,
            min_camera_zoom: label.min_camera_zoom,
            max_camera_zoom: label.max_camera_zoom,
            is_local: label.is_local,
        };
        if let Some(shields) = &label.route_shields {
            let geometry = route_shield_geometry(layout, shields, label.text_style.key, label_index);
            if geometry.size[0] <= 0.0 {
                continue;
            }
            built.push(BuiltBaseLabel {
                placement_input: placement(geometry.size),
                style: label.text_style,
                text_vertices: Vec::new(),
                icon_vertices: Vec::new(),
                route_shield_glyphs: geometry.glyphs,
                route_shield_plates: geometry.plates,
            });
            continue;
        }
        let style = label.text_style;
        let content_scale = if label.poi_icon.is_none() { 1.0 } else { POI_COMBINED_LABEL_SCALE };
        // Baked in layout points: the shaders scale by the frame's pixels per point.
        let text_scale = style.size_points * content_scale;
        let metrics = wrapped_label_metrics(layout, &label.text, label_index, text_scale, style.weight, style.letter_spacing_em);
        let (text_vertices, size, icon_vertices) = combined_geometry(&atlas, metrics, label.poi_icon, &style, label_index, content_scale);
        built.push(BuiltBaseLabel { placement_input: placement(size), style, text_vertices, icon_vertices, route_shield_glyphs: Vec::new(), route_shield_plates: Vec::new() });
    }
    make_text_labels(built)
}

/// The identity of a homogeneous glyph or icon run: everything the drawing
/// applies per run (the atlas by the weight, the colours, the halo width in
/// points, the growth, the opacity). The size is baked into the vertices,
/// so labels differing only in size share a run. Two weights sharing a
/// style key never share a run: a run binds one atlas.
/// A run identity bit for bit, what runs are grouped by (the Swift `Hashable`).
type RunIdentityBits = (i32, u8, [u32; 3], [u32; 3], [u32; 3]);

#[derive(Debug, Clone, Copy)]
struct RunIdentity {
    key: i32,
    weight: LabelFontWeight,
    fill: [f32; 3],
    stroke: [f32; 3],
    halo_width_points: f32,
    embolden_points: f32,
    opacity: f32,
}

impl RunIdentity {
    fn new(style: &LabelTextStyle) -> RunIdentity {
        RunIdentity {
            key: style.key,
            weight: style.weight,
            fill: style.fill_color,
            stroke: style.stroke_color,
            halo_width_points: style.halo_em * style.size_points,
            embolden_points: style.embolden_points,
            opacity: style.opacity,
        }
    }

    fn bits(&self) -> RunIdentityBits {
        (
            self.key,
            self.weight.raw_value(),
            self.fill.map(f32::to_bits),
            self.stroke.map(f32::to_bits),
            [self.halo_width_points.to_bits(), self.embolden_points.to_bits(), self.opacity.to_bits()],
        )
    }

    /// The draw order: by key, then by the other fields, so equal keys break into a stable sequence.
    fn order(a: &RunIdentity, b: &RunIdentity) -> Ordering {
        let float = |x: f32, y: f32| x.partial_cmp(&y).unwrap_or(Ordering::Equal);
        let mut ordering = a.key.cmp(&b.key).then(a.weight.raw_value().cmp(&b.weight.raw_value()));
        ordering = ordering.then(float(a.halo_width_points, b.halo_width_points)).then(float(a.embolden_points, b.embolden_points)).then(float(a.opacity, b.opacity));
        for i in 0..3 {
            ordering = ordering.then(float(a.fill[i], b.fill[i]));
        }
        for i in 0..3 {
            ordering = ordering.then(float(a.stroke[i], b.stroke[i]));
        }
        ordering
    }
}

struct RunGroup {
    identity: RunIdentity,
    style: LabelTextStyle,
    glyphs: Vec<LabelVertex>,
    icons: Vec<LabelVertex>,
}

fn remapped(vertices: &[LabelVertex], label_index: i32) -> impl Iterator<Item = LabelVertex> + '_ {
    vertices.iter().map(move |v| LabelVertex { label_index, ..*v })
}

/// Every built label in one set with compact label indices, the glyph and
/// icon runs in the identities' order, the plate runs in the order their
/// looks first appear. No budget and no distance tier: the frame's
/// collisions alone decide what shows.
pub fn make_text_labels(built: Vec<BuiltBaseLabel>) -> TextLabelSet {
    let mut groups: Vec<RunGroup> = Vec::new();
    let mut group_of: HashMap<RunIdentityBits, usize> = HashMap::new();
    let mut plates: Vec<LabelRun<RouteShieldRunStyle>> = Vec::new();
    let mut placement_inputs = Vec::with_capacity(built.len());
    let mut group_for = |style: &LabelTextStyle, groups: &mut Vec<RunGroup>| -> usize {
        let identity = RunIdentity::new(style);
        let index = *group_of.entry(identity.bits()).or_insert_with(|| {
            groups.push(RunGroup { identity, style: *style, glyphs: Vec::new(), icons: Vec::new() });
            groups.len() - 1
        });
        // The last style of an identity is the run's, as the Swift dictionary keeps it.
        groups[index].style = *style;
        index
    };
    for (compact, label) in built.into_iter().enumerate() {
        let label_index = compact as i32;
        let g = group_for(&label.style, &mut groups);
        placement_inputs.push(label.placement_input);
        groups[g].glyphs.extend(remapped(&label.text_vertices, label_index));
        groups[g].icons.extend(remapped(&label.icon_vertices, label_index));
        for (style, vertices) in &label.route_shield_glyphs {
            let g = group_for(style, &mut groups);
            groups[g].glyphs.extend(remapped(vertices, label_index));
        }
        for (style, vertices) in &label.route_shield_plates {
            match plates.iter_mut().find(|run| run.style.same_run(style)) {
                Some(run) => run.vertices.extend(remapped(vertices, label_index)),
                None => plates.push(LabelRun { style: *style, vertices: remapped(vertices, label_index).collect() }),
            }
        }
    }
    groups.sort_by(|a, b| RunIdentity::order(&a.identity, &b.identity));
    let mut glyph_runs = Vec::new();
    let mut poi_icon_runs = Vec::new();
    for group in groups {
        if !group.glyphs.is_empty() {
            glyph_runs.push(LabelRun { style: group.style, vertices: group.glyphs });
        }
        if !group.icons.is_empty() {
            poi_icon_runs.push(LabelRun { style: group.style, vertices: group.icons });
        }
    }
    TextLabelSet { placement_inputs, glyph_runs, poi_icon_runs, route_shield_runs: plates }
}

/// A label's text and, for a POI, the icon disc left of it, the two
/// centred on each other's height: the text's vertices, the label's box,
/// the icon's quad.
fn combined_geometry(
    atlas: &PoiSpriteAtlasLayout,
    metrics: TextMetrics,
    icon: Option<PoiSpriteIcon>,
    style: &LabelTextStyle,
    label_index: i32,
    content_scale: f32,
) -> (Vec<LabelVertex>, [f32; 2], Vec<LabelVertex>) {
    let Some((icon, region)) = icon.and_then(|icon| atlas.region(icon).map(|r| (icon, r))) else {
        return (metrics.vertices, [metrics.width, metrics.height], Vec::new());
    };
    // An icon's side in layout points follows the text over a narrow band of sizes.
    let icon_size = style.size_points.clamp(9.0, 12.0) * 2.6 * content_scale * disc_scale(icon);
    let icon_gap = (style.size_points * 0.2).max(3.0) * content_scale;
    let width = icon_size + icon_gap + metrics.width;
    let height = icon_size.max(metrics.height);
    let text_y = ((height - metrics.height) * 0.5).max(0.0);
    let icon_y = ((height - icon_size) * 0.5).max(0.0);
    let mut text = metrics.vertices;
    if icon_size > 0.0 {
        for v in &mut text {
            v.position[0] += icon_size + icon_gap;
            v.position[1] += text_y;
        }
    }
    let [u0, v0, u1, v1] = region.uv_rect;
    let corner = |x: f32, y: f32, uv: [f32; 2], sprite: [f32; 2]| LabelVertex { position: [x, y], uv, label_index, sprite_uv: sprite };
    let quad = vec![
        corner(0.0, icon_y, [u1, v1], [0.0, 0.0]),
        corner(icon_size, icon_y, [u0, v1], [1.0, 0.0]),
        corner(0.0, icon_y + icon_size, [u1, v0], [0.0, 1.0]),
        corner(icon_size, icon_y, [u0, v1], [1.0, 0.0]),
        corner(icon_size, icon_y + icon_size, [u0, v0], [1.0, 1.0]),
        corner(0.0, icon_y + icon_size, [u1, v0], [0.0, 1.0]),
    ];
    (text, [width, height], quad)
}

struct RouteShieldGeometry {
    size: [f32; 2],
    glyphs: Vec<(LabelTextStyle, Vec<LabelVertex>)>,
    plates: Vec<(RouteShieldRunStyle, Vec<LabelVertex>)>,
}

/// A route sign label in layout points (y up): the signs side by side,
/// each centred on the label's height, each number centred on its plate.
/// A plate's quad carries its own size in `uv` and its corner in
/// `sprite_uv`, which the plate's distance field is drawn from. On an
/// escutcheon the number sits in the body under the band, a little above
/// the middle, where the sign is still wide before it narrows to its point.
fn route_shield_geometry(layout: &TextLayout, style: &RouteShieldStyle, text_key: i32, label_index: i32) -> RouteShieldGeometry {
    let em = style.size_points;
    let plate_height = em * style.plate_height_em;
    let gap = em * style.gap_em;
    struct Sign {
        metrics: TextMetrics,
        appearance: map_style::style::RouteShieldAppearance,
        width: f32,
        height: f32,
    }
    let mut signs = Vec::new();
    for shield in &style.shields {
        let metrics = layout.label_vertices(&shield.text, &LabelLayoutRequest::new(label_index, em, style.weight));
        if metrics.vertices.is_empty() {
            continue;
        }
        let appearance = shield.appearance;
        let padded_width = metrics.width + 2.0 * em * appearance.padding_em;
        let (width, height) = match appearance.shape {
            RouteShieldShape::Rectangle => (padded_width.max(plate_height), plate_height),
            // The round ends take room the padding does not count.
            RouteShieldShape::Capsule => ((padded_width + plate_height * 0.4).max(plate_height * 1.2), plate_height),
            RouteShieldShape::Escutcheon => {
                let width = padded_width.max(plate_height);
                (width, (plate_height * 1.2).max(width * 1.05))
            }
        };
        signs.push(Sign { metrics, appearance, width, height });
    }
    if signs.is_empty() {
        return RouteShieldGeometry { size: [0.0, 0.0], glyphs: Vec::new(), plates: Vec::new() };
    }
    let label_height = signs.iter().map(|s| s.height).fold(f32::MIN, f32::max);
    let mut glyphs = Vec::with_capacity(signs.len());
    let mut plates = Vec::with_capacity(signs.len());
    let mut x = 0.0f32;
    for sign in signs {
        let appearance = sign.appearance;
        let bottom = (label_height - sign.height) * 0.5;
        let header_fraction = if appearance.header_color.is_none() { 0.0 } else { appearance.header_fraction };
        let text_centre = if appearance.shape == RouteShieldShape::Escutcheon { (1.0 - header_fraction) * 0.56 } else { 0.5 };
        let offset = [x + (sign.width - sign.metrics.width) * 0.5, bottom + sign.height * text_centre - sign.metrics.height * 0.5];
        let vertices = sign
            .metrics
            .vertices
            .iter()
            .map(|v| LabelVertex { position: [v.position[0] + offset[0], v.position[1] + offset[1]], ..*v })
            .collect();
        let glyph_style = LabelTextStyle { key: text_key, ..LabelTextStyle::new(appearance.text_color, appearance.text_color, 0.0, em, style.weight) };
        glyphs.push((glyph_style, vertices));
        let plate_style = RouteShieldRunStyle {
            shape: appearance.shape,
            fill_color: appearance.fill_color,
            border_color: appearance.border_color,
            header_color: appearance.header_color,
            header_fraction,
            border_width_points: em * appearance.border_em,
            corner_radius_points: em * appearance.corner_radius_em,
        };
        plates.push((plate_style, plate_quad([x, bottom], [sign.width, sign.height], label_index)));
        x += sign.width + gap;
    }
    RouteShieldGeometry { size: [x - gap, label_height], glyphs, plates }
}

fn plate_quad(origin: [f32; 2], size: [f32; 2], label_index: i32) -> Vec<LabelVertex> {
    let corner = |u: f32, v: f32| LabelVertex { position: [origin[0] + size[0] * u, origin[1] + size[1] * v], uv: size, label_index, sprite_uv: [u, v] };
    vec![corner(0.0, 0.0), corner(1.0, 0.0), corner(0.0, 1.0), corner(1.0, 0.0), corner(1.0, 1.0), corner(0.0, 1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use map_style::style::{RouteShield, RouteShieldAppearance};
    use std::collections::BTreeSet;

    fn text_style(key: i32, weight: LabelFontWeight, fill: [f32; 3], size_points: f32) -> LabelTextStyle {
        LabelTextStyle { key, ..LabelTextStyle::new(fill, [1.0, 1.0, 1.0], 0.15, size_points, weight) }
    }

    fn placement(index: usize) -> TextLabelPlacementInput {
        TextLabelPlacementInput {
            uv: [index as f32, index as f32],
            roof_height: 0.0,
            lifts_to_roof: false,
            key: index as u64 + 1,
            sort_key: index as i32,
            collision_priority: index as i32,
            label_size_points: [10.0, 4.0],
            min_camera_zoom: 0.0,
            max_camera_zoom: f32::INFINITY,
            is_local: false,
        }
    }

    fn built(index: usize, key: i32, weight: LabelFontWeight, fill: [f32; 3], uv: [f32; 2], with_icon: bool) -> BuiltBaseLabel {
        let icon = LabelVertex { position: [0.0, 0.0], uv: [0.5, 0.5], label_index: index as i32, sprite_uv: [0.0, 0.0] };
        BuiltBaseLabel {
            placement_input: placement(index),
            style: text_style(key, weight, fill, 20.0),
            text_vertices: vec![LabelVertex::new([0.0, 0.0], uv, index as i32)],
            icon_vertices: if with_icon { vec![icon] } else { Vec::new() },
            route_shield_glyphs: Vec::new(),
            route_shield_plates: Vec::new(),
        }
    }

    fn parsed(text: &str, style: LabelTextStyle) -> ParsedTextLabel {
        ParsedTextLabel::new(text.to_string(), [2048, 2048], 1, 0, 0, style)
    }

    fn size_of(text: &str, size_points: f32) -> [f32; 2] {
        let layout = text_layout::bundled();
        let label = parsed(text, text_style(7, LabelFontWeight::Thin, [0.1, 0.1, 0.1], size_points));
        build(&layout, &[&label]).placement_inputs[0].label_size_points
    }

    /// TileTextLabelsBuilderTests.testEveryLabelIsKeptInOrder
    #[test]
    fn every_label_is_kept_in_order() {
        let labels = (0..40).map(|i| built(i, (i % 3 + 1) as i32, LabelFontWeight::Thin, [1.0, 1.0, 1.0], [0.0, 0.0], i % 2 == 0)).collect();
        let set = make_text_labels(labels);
        assert_eq!(set.placement_inputs.len(), 40, "no budget and no tier: the tile carries all its labels");
        assert_eq!(set.placement_inputs.iter().map(|p| p.key).collect::<Vec<_>>(), (1..=40).collect::<Vec<u64>>());
        let glyphs: BTreeSet<i32> = set.glyph_runs.iter().flat_map(|r| r.vertices.iter().map(|v| v.label_index)).collect();
        assert_eq!(glyphs, (0..40).collect(), "every label has its text vertices, indexed compactly");
        let icons: BTreeSet<i32> = set.poi_icon_runs.iter().flat_map(|r| r.vertices.iter().map(|v| v.label_index)).collect();
        assert_eq!(icons, (0..40).step_by(2).collect(), "icons ride with the labels that have them");
        assert_eq!(set.glyph_runs.iter().map(|r| r.style.key).collect::<Vec<_>>(), vec![1, 2, 3], "runs by key");
    }

    /// TileTextLabelsBuilderTests.testLabelsSharingKeyButDifferingWeightSplitIntoSeparateRuns
    #[test]
    fn labels_sharing_a_key_but_not_a_weight_split_into_separate_runs() {
        let bold = built(0, 70, LabelFontWeight::Bold, [0.2, 0.2, 0.2], [0.1, 0.1], false);
        let thin = built(1, 70, LabelFontWeight::Thin, [0.3, 0.3, 0.3], [0.9, 0.9], false);
        let runs = make_text_labels(vec![bold, thin]).glyph_runs;
        assert_eq!(runs.len(), 2, "same key with mixed weights must not collapse into one run");
        let bold_run = runs.iter().find(|r| r.style.weight == LabelFontWeight::Bold).unwrap();
        let thin_run = runs.iter().find(|r| r.style.weight == LabelFontWeight::Thin).unwrap();
        assert_eq!(bold_run.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(), vec![[0.1, 0.1]]);
        assert_eq!(thin_run.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(), vec![[0.9, 0.9]]);
        assert_eq!(bold_run.style.fill_color, [0.2, 0.2, 0.2]);
        assert_eq!(thin_run.style.fill_color, [0.3, 0.3, 0.3]);
    }

    /// LabelScreenUnitTests.testGlyphGeometryIsBakedInPointsAndIsDisplayIndependent
    #[test]
    fn glyph_geometry_is_baked_in_points() {
        let size = size_of("Point", 12.0);
        // A 12 point label is roughly a line high, never the 24 or 36 pixels a 2x or 3x display would give it.
        assert!(size[1] > 8.0 && size[1] < 20.0, "{size:?}");
    }

    /// LabelScreenUnitTests.testGlyphGeometryScalesWithTheStyleSize
    #[test]
    fn glyph_geometry_scales_with_the_style_size() {
        let (small, large) = (size_of("Point", 11.0), size_of("Point", 22.0));
        assert!((large[0] / small[0] - 2.0).abs() < 0.01);
        assert!((large[1] / small[1] - 2.0).abs() < 0.01);
    }

    fn wrapped(text: &str) -> TextMetrics {
        wrapped_label_metrics(&text_layout::bundled(), text, 0, 14.0, LabelFontWeight::Bold, 0.0)
    }

    /// LabelWrapExtensionTests.testAShortNameStaysInTheBaseBox
    #[test]
    fn a_short_name_stays_in_the_base_box() {
        assert!(wrapped("Кафедры анатомии").width <= 14.0 * 10.0 * 1.02);
    }

    /// LabelWrapExtensionTests.testALongNameGetsTheWiderTallerBoxInsteadOfATrailingLine
    #[test]
    fn a_long_name_gets_the_wider_taller_box_instead_of_a_trailing_line() {
        let long = wrapped("Кафедры анатомии, гистологии, микробиологии академии им. И. М. Сеченова");
        let base = 14.0 * 10.0;
        assert!(long.width <= base * 1.5 * 1.02, "every line fits the extended box");
        assert!(long.width > base, "and the box really did widen");
        let three_lines = wrapped("Кафедры\nанатомии\nгистологии").height;
        assert!(long.height > three_lines, "a long name runs taller than three lines");
    }

    #[test]
    fn labels_sort_by_their_zoom_then_priority_and_index_their_vertices_by_that_order() {
        let layout = text_layout::bundled();
        let style = text_style(1, LabelFontWeight::Bold, [0.0, 0.0, 0.0], 12.0);
        let mut late = parsed("Late", style);
        late.min_camera_zoom = 15.0;
        late.key = 10;
        let mut urgent = parsed("Urgent", style);
        urgent.collision_priority = -1;
        urgent.key = 20;
        let plain = ParsedTextLabel { key: 30, ..parsed("Plain", style) };
        let set = build(&layout, &[&late, &plain, &urgent]);
        assert_eq!(set.placement_inputs.iter().map(|p| p.key).collect::<Vec<_>>(), vec![20, 30, 10]);
        assert_eq!(set.placement_inputs[0].uv, [0.5, 0.5]);
        assert_eq!(set.glyph_runs.len(), 1);
        let first_quad_of = |index: i32| set.glyph_runs[0].vertices.iter().position(|v| v.label_index == index).unwrap();
        assert!(first_quad_of(0) < first_quad_of(1) && first_quad_of(1) < first_quad_of(2));
    }

    #[test]
    fn a_poi_draws_its_disc_left_of_its_name_both_centred_on_the_label() {
        let layout = text_layout::bundled();
        let style = text_style(2, LabelFontWeight::Bold, [0.0, 0.0, 0.0], 11.0);
        let mut museum = parsed("Museum", style);
        museum.poi_icon = Some(PoiSpriteIcon::Museum);
        let mut dot = parsed("Museum", style);
        dot.poi_icon = Some(PoiSpriteIcon::Marker);
        let set = build(&layout, &[&museum]);
        let icon = &set.poi_icon_runs[0].vertices;
        assert_eq!(icon.len(), 6);
        // 11 points within 9...12, times 2.6, times the POI scale: the disc's side.
        let side = 11.0 * 2.6 * POI_COMBINED_LABEL_SCALE;
        assert!((icon[4].position[0] - side).abs() < 1e-4);
        assert_eq!(icon[0].sprite_uv, [0.0, 0.0]);
        assert_eq!(icon[4].sprite_uv, [1.0, 1.0]);
        let gap = 3.0f32.max(11.0 * 0.2) * POI_COMBINED_LABEL_SCALE;
        let text_left = set.glyph_runs[0].vertices.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
        assert!((text_left - (side + gap)).abs() < 1e-3, "the name starts after the disc and the gap");
        let size = set.placement_inputs[0].label_size_points;
        assert!((size[1] - side).abs() < 1e-4, "the disc is taller than the name");
        let small = build(&layout, &[&dot]).placement_inputs[0].label_size_points;
        assert!(small[0] < size[0], "the plain marker is a smaller dot");
    }

    #[test]
    fn route_signs_lay_out_their_numbers_on_plates_side_by_side() {
        let layout = text_layout::bundled();
        let white = [1.0, 1.0, 1.0];
        let green = RouteShieldAppearance::new([0.0, 0.5, 0.0], white);
        let blue = RouteShieldAppearance::new([0.0, 0.0, 0.6], white).shape(RouteShieldShape::Escutcheon).header(Some([0.8, 0.0, 0.0]));
        let shields = RouteShieldStyle::new(vec![RouteShield { text: "E30".into(), appearance: green }, RouteShield { text: "M1".into(), appearance: blue }]);
        let mut label = parsed("E30 M1", text_style(5, LabelFontWeight::Bold, white, 11.0));
        label.route_shields = Some(shields.clone());
        let set = build(&layout, &[&label]);
        assert_eq!(set.placement_inputs.len(), 1);
        assert_eq!(set.route_shield_runs.len(), 2, "two looks, two plate runs");
        let first = &set.route_shield_runs[0];
        assert_eq!(first.style.shape, RouteShieldShape::Rectangle);
        assert_eq!(first.style.header_fraction, 0.0, "no band");
        assert!((first.style.corner_radius_points - 11.0 * 0.2).abs() < 1e-5);
        let plate_height = 11.0 * shields.plate_height_em;
        assert!((first.vertices[0].uv[1] - plate_height).abs() < 1e-4, "a plate carries its size in uv");
        let second = &set.route_shield_runs[1];
        assert_eq!(second.style.header_color, Some([0.8, 0.0, 0.0]));
        assert!(second.vertices[0].position[0] > first.vertices[4].position[0], "the second sign right of the first");
        // The escutcheon is the taller sign, so it sets the label's height.
        let size = set.placement_inputs[0].label_size_points;
        assert!((size[1] - second.vertices[0].uv[1]).abs() < 1e-4);
        // The numbers draw in one run of their colour, under the label's key.
        assert_eq!(set.glyph_runs.len(), 1);
        assert_eq!(set.glyph_runs[0].style.key, 5);
        assert_eq!(set.glyph_runs[0].style.halo_em, 0.0);
        assert_eq!(set.glyph_runs[0].vertices.len(), 5 * 6);
        assert!(set.glyph_runs[0].vertices.iter().all(|v| v.label_index == 0));
    }

    #[test]
    fn a_route_sign_label_without_a_number_the_atlas_draws_is_dropped() {
        let layout = text_layout::bundled();
        let shields = RouteShieldStyle::new(vec![RouteShield { text: " ".into(), appearance: RouteShieldAppearance::new([0.0; 3], [1.0; 3]) }]);
        let mut label = parsed(" ", text_style(5, LabelFontWeight::Bold, [1.0; 3], 11.0));
        label.route_shields = Some(shields);
        assert!(build(&layout, &[&label]).placement_inputs.is_empty());
    }
}
