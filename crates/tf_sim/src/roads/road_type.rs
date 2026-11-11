//! `RoadType` (`sim/roads/road_type.gd`): static description of a kind of road. One
//! file per type in `crates/tf_sim/data/road_types/`, so new road types are data, not
//! code. Saves reference a type by its `id`.

use std::sync::OnceLock;

use serde::Deserialize;

/// The road type files, embedded so the sim needs no file system.
const FILES: [&str; 6] = [
    include_str!("../../data/road_types/local_2lane.ron"),
    include_str!("../../data/road_types/avenue_4lane.ron"),
    include_str!("../../data/road_types/avenue_4lane_bus.ron"),
    include_str!("../../data/road_types/one_way_1lane.ron"),
    include_str!("../../data/road_types/one_way_2lane.ron"),
    include_str!("../../data/road_types/one_way_3lane.ron"),
];

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RoadType {
    pub id: String,
    pub display_name: String,
    /// Position in the build palette (lowest first).
    pub palette_order: i32,
    /// Lanes each way; on a one-way road, its number of lanes.
    pub lanes_per_direction: i32,
    /// One-way roads only have lanes running from the segment's start node to its end
    /// node (the direction the player dragged), centered on the road.
    pub one_way: bool,
    /// At junctions without lights, drivers coming from a road of higher priority go
    /// first and don't slow down to look; between equal roads, other rules decide.
    pub priority: i32,
    /// The curb lane each way is a bus lane: only buses drive along it (cars may cross
    /// it to reach the curb). Needs two lanes or more each way.
    pub bus_lanes: bool,
    /// World pixels.
    pub lane_width: f64,
    /// World pixels per sim second.
    pub speed_limit: f64,
    /// RGBA, 0 to 1.
    pub surface_color: [f32; 4],
    pub edge_color: [f32; 4],
}

/// A `RoadType.new()` in GDScript: the exported defaults (tests build custom types from it).
impl Default for RoadType {
    fn default() -> Self {
        Self {
            id: String::new(),
            display_name: String::new(),
            palette_order: 0,
            lanes_per_direction: 1,
            one_way: false,
            priority: 0,
            bus_lanes: false,
            lane_width: 14.0,
            speed_limit: 90.0,
            // Color("4a5562") and Color("2b323b").
            surface_color: [74.0 / 255.0, 85.0 / 255.0, 98.0 / 255.0, 1.0],
            edge_color: [43.0 / 255.0, 50.0 / 255.0, 59.0 / 255.0, 1.0],
        }
    }
}

impl RoadType {
    pub fn total_width(&self) -> f64 {
        self.lane_width * f64::from(self.lane_count())
    }

    /// Lanes across the whole road, both directions.
    pub fn lane_count(&self) -> i32 {
        if self.one_way {
            self.lanes_per_direction
        } else {
            self.lanes_per_direction * 2
        }
    }

    /// Every road type, in palette order (then by id, like Godot's `list_all`).
    pub fn list_all() -> &'static [RoadType] {
        static TYPES: OnceLock<Vec<RoadType>> = OnceLock::new();
        TYPES.get_or_init(|| {
            let mut types: Vec<RoadType> = FILES
                .iter()
                .map(|text| ron::from_str(text).expect("road type file"))
                .collect();
            types.sort_by(|a, b| {
                a.palette_order
                    .cmp(&b.palette_order)
                    .then_with(|| a.id.cmp(&b.id))
            });
            types
        })
    }

    pub fn load_by_id(type_id: &str) -> Option<&'static RoadType> {
        Self::list_all()
            .iter()
            .find(|road_type| road_type.id == type_id)
    }

    /// Is the lane `index` lanes out from the center (`Lane::index_from_center`) a bus
    /// lane?
    pub fn is_bus_lane(&self, index: i32) -> bool {
        self.bus_lanes && index == self.lanes_per_direction - 1
    }

    /// Lanes each way that all traffic may drive along.
    pub fn general_lane_count(&self) -> i32 {
        self.lanes_per_direction - i32::from(self.bus_lanes)
    }
}
