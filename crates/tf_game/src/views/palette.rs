//! The colours of the street game, in one place. The target look is the street style of
//! Google Maps (Paris): a light map, a blue-grey carriageway, white marks, beige building
//! blocks, mint for bike lanes and planting. Values are chosen by eye from that reference;
//! change them here and nowhere else. Agents, marks and signals are darker and more
//! saturated than the map, so they stay readable on it.

use bevy::color::Alpha;
use bevy::prelude::Color;

/// A colour from its 8-bit sRGB parts (`#RRGGBB`).
pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// The same colour with another opacity (ghosts, rings).
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    color.with_alpha(alpha)
}

/// Outside the level (also the camera's clear colour in play mode).
pub const OUTSIDE: Color = rgb(0xF2, 0xEF, 0xE9);
pub const BLOCK: Color = rgb(0xF8, 0xF1, 0xE3);
pub const BLOCK_OUTLINE: Color = rgb(0xE8, 0xDC, 0xC5);
/// Carriageway, and the junction area.
pub const CARRIAGEWAY: Color = rgb(0xAA, 0xB9, 0xC9);
pub const PARKING: Color = rgb(0xBF, 0xCA, 0xD6);
pub const SIDEWALK: Color = rgb(0xE9, 0xEC, 0xEF);
pub const KERB: Color = rgb(0xC9, 0xD0, 0xD8);
/// Lane marks and zebra bars.
pub const PAINT: Color = rgb(0xFF, 0xFF, 0xFF);
pub const BUS_LANE: Color = rgb(0xD8, 0xA9, 0xA3);
pub const BIKE_LANE: Color = rgb(0xBF, 0xE6, 0xCF);
pub const BIKE_TRACK: Color = rgb(0xA6, 0xDC, 0xBC);
/// The bicycle box (*sas vélo*): a stronger green than the bicycle lane, so it reads as one
/// flat area across the motor lanes.
pub const BIKE_BOX: Color = rgb(0x5C, 0xC2, 0x86);
pub const DELIVERY_BAY: Color = rgb(0xE3, 0xD3, 0xA4);
pub const MEDIAN: Color = rgb(0xD5, 0xD9, 0xDE);
pub const PLANTING: Color = rgb(0xC8, 0xEB, 0xD5);
pub const TERRACE: Color = rgb(0xEA, 0xD7, 0xBE);
pub const REST_AREA: Color = rgb(0xE8, 0xE2, 0xC4);
pub const TREE: Color = rgb(0x7F, 0xBF, 0x92);
pub const BENCH: Color = rgb(0xB5, 0x8F, 0x6A);
pub const BOLLARD: Color = rgb(0x4A, 0x55, 0x60);

/// The speed limit painted on the carriageway. Worn white by default, as real markings are
/// never new. To paint it in the tint of the carriageway instead, set `SPEED_MARK` to
/// `SPEED_MARK_TINT`.
pub const SPEED_MARK_WORN: Color = rgb(0xDD, 0xE3, 0xEA);
/// A darker tint of the carriageway colour: the other option for the speed marking.
pub const SPEED_MARK_TINT: Color = rgb(0x8E, 0x9F, 0xB2);
/// The colour of the speed marking in use.
pub const SPEED_MARK: Color = SPEED_MARK_WORN;

/// Preview ghosts: a click that works, a click the street refuses or that removes.
pub const GHOST_OK: Color = rgb(0x2E, 0x7D, 0xF0);
pub const GHOST_BAD: Color = rgb(0xE0, 0x30, 0x30);
/// Opacity of a ghost's fill and of its outline.
pub const GHOST_FILL_ALPHA: f32 = 0.6;
pub const GHOST_LINE_ALPHA: f32 = 0.9;

/// The outline of the agent under the pointer.
pub const HOVER_OUTLINE: Color = rgb(0x10, 0x10, 0x14);

/// Panels of the play screen: dark cards with light text, on the light map.
pub const PANEL: Color = Color::srgba(0.07, 0.08, 0.10, 0.9);
pub const PANEL_TEXT: Color = rgb(0xF0, 0xF0, 0xF0);
