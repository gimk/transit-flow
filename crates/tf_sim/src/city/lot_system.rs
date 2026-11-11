//! `LotSystem` (`sim/city/lot_system.gd`): owns all lots and resolves each lot's road
//! access: the nearest road-lane position within reach of the lot's center (the lane on
//! the lot's side of the road), plus the matching positions on the road's lanes in the
//! other direction. Access is cached and recomputed lazily after any road change.
//! Lots stand on what the roads around their cell leave free (`compute_plot`): a road
//! built so close that too little is left (e.g. an avenue one cell from another road)
//! removes the lot, and no lot is placed there.
//!
//! The road network is passed into the calls that need it (the GDScript class holds it).

use glam::{IVec2, Vec2};
use indexmap::IndexMap;
use serde_json::{Value, json};

use super::lot::{Lot, LotType};
use crate::events::{Outbox, SimEvent};
use crate::geom::segment::closest_point_to_segment;
use crate::geom::{GdVec2, Rect2};
use crate::grid;
use crate::ids::{LotId, SegmentId};
use crate::roads::{ChangeSet, Lane, RoadNetwork};
use crate::routing::LanePoint;
use crate::traffic::vehicle;
use crate::walking::walk_network::SIDEWALK_OFFSET;

/// From a cell center, the near lane of a road on the cell's edge is ~25 px away and the
/// far lane ~39 px; anything beyond a cell's reach doesn't count.
pub const ACCESS_DISTANCE: f64 = 48.0;
/// Expected extra time (sim seconds) to turn across the road into or out of a driveway,
/// counted when choosing between the two sides (`Router::find_best_route`).
pub const FAR_SIDE_PENALTY: f64 = 4.0;
/// Driveways keep this far (px) from either end of their lane: a driver stopping to turn
/// in, or waiting to pull out, stands clear of the junction instead of blocking its exit
/// (corner lots would otherwise get one right at the junction).
pub const DRIVEWAY_SETBACK: f64 = vehicle::MAX_LENGTH + 4.0;
/// Room a road takes beyond its half width: the sidewalk, and a little verge.
pub const ROAD_CLEARANCE: f64 = SIDEWALK_OFFSET + 2.0;
/// A plot narrower than this (px) holds no building.
pub const MIN_PLOT: f64 = 16.0;
/// How far around a changed road lots may be affected by the edit (px): the widest
/// road's half width and clearance plus a cell, which also covers ACCESS_DISTANCE.
pub const EDIT_REACH: f64 = 2.0 * grid::CELL_SIZE as f64;

#[derive(Clone, Debug)]
pub struct LotSystem {
    pub lots: IndexMap<LotId, Lot>,
    lot_by_cell: IndexMap<IVec2, LotId>,
    access_cache: IndexMap<LotId, Vec<LanePoint>>,
    /// Each road segment's box (`RoadSegment::bounds`), kept so that after a road edit
    /// the area the changed roads covered before and after it is known: only lots there
    /// need their plot and access worked out again.
    segment_bounds: IndexMap<SegmentId, Rect2>,
    next_id: i32,
}

impl Default for LotSystem {
    fn default() -> Self {
        Self {
            lots: IndexMap::new(),
            lot_by_cell: IndexMap::new(),
            access_cache: IndexMap::new(),
            segment_bounds: IndexMap::new(),
            next_id: 1,
        }
    }
}

impl LotSystem {
    pub fn new(roads: &RoadNetwork) -> Self {
        let mut lots = Self::default();
        for segment in roads.segments.values() {
            lots.segment_bounds.insert(segment.id, segment.bounds());
        }
        lots
    }

    /// The new lot's id, or `None` if the cell is taken, outside the map, or the roads
    /// leave no room for a building there.
    pub fn add_lot(
        &mut self,
        roads: &RoadNetwork,
        cell: IVec2,
        lot_type: LotType,
        out: &mut Outbox,
    ) -> Option<LotId> {
        if !self.can_place(cell) {
            return None;
        }
        let plot = Self::compute_plot(roads, cell);
        if !plot.has_area() {
            return None;
        }
        let mut lot = Lot::new(LotId(self.next_id), cell, lot_type);
        lot.plot = plot;
        self.next_id += 1;
        let id = lot.id;
        self.insert(lot);
        out.push(SimEvent::LotsChanged);
        Some(id)
    }

    /// The part of `cell` that roads leave free for a building (world px), or an empty
    /// `Rect2` if too little is left (MIN_PLOT). Each road along or through the cell
    /// takes its half width plus ROAD_CLEARANCE from the side it runs on (diagonal and
    /// curved pieces, roughly, from the side facing them).
    pub fn compute_plot(roads: &RoadNetwork, cell: IVec2) -> Rect2 {
        let size = grid::CELL_SIZE as f32;
        let cell_rect = Rect2::new(cell.as_vec2() * size, Vec2::splat(size));
        let mut top_left = cell_rect.position;
        let mut bottom_right = cell_rect.end();
        let center = cell_rect.center();
        let reach = f64::from(grid::CELL_SIZE);
        for segment in roads.segments.values() {
            if segment.elevation_layer != 0 {
                continue;
            }
            let half = segment.road_type.total_width() * 0.5 + ROAD_CLEARANCE;
            if !segment
                .bounds()
                .grow((half + reach) as f32)
                .intersects(&cell_rect)
            {
                continue; // Too far to matter (most roads, for most cells).
            }
            for pair in segment.polyline().windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if (a.y - b.y).abs() < 0.5 && f64::from((a.y - center.y).abs()) < reach {
                    // Along the cell's top or bottom (or through it).
                    if a.x.min(b.x) < cell_rect.end().x - 1.0
                        && a.x.max(b.x) > cell_rect.position.x + 1.0
                    {
                        if a.y <= center.y {
                            top_left.y = f64::from(top_left.y).max(f64::from(a.y) + half) as f32;
                        } else {
                            bottom_right.y =
                                f64::from(bottom_right.y).min(f64::from(a.y) - half) as f32;
                        }
                    }
                } else if (a.x - b.x).abs() < 0.5 && f64::from((a.x - center.x).abs()) < reach {
                    if a.y.min(b.y) < cell_rect.end().y - 1.0
                        && a.y.max(b.y) > cell_rect.position.y + 1.0
                    {
                        if a.x <= center.x {
                            top_left.x = f64::from(top_left.x).max(f64::from(a.x) + half) as f32;
                        } else {
                            bottom_right.x =
                                f64::from(bottom_right.x).min(f64::from(a.x) - half) as f32;
                        }
                    }
                } else if (a.x - b.x).abs() >= 0.5 && (a.y - b.y).abs() >= 0.5 {
                    // A diagonal or curved piece: the plot keeps clear of it on the side
                    // facing it (roughly: by the room left between the cell's center and
                    // the road).
                    let closest = closest_point_to_segment(center, a, b);
                    let room = f64::from(closest.gd_distance_to(center)) - half;
                    if room >= f64::from(grid::CELL_SIZE) * 0.71 {
                        continue; // Clear of the whole cell.
                    }
                    if room < MIN_PLOT * 0.5 {
                        return Rect2::default();
                    }
                    let toward = closest - center;
                    if toward.x.abs() >= toward.y.abs() {
                        if toward.x > 0.0 {
                            bottom_right.x =
                                f64::from(bottom_right.x).min(f64::from(center.x) + room) as f32;
                        } else {
                            top_left.x =
                                f64::from(top_left.x).max(f64::from(center.x) - room) as f32;
                        }
                    } else if toward.y > 0.0 {
                        bottom_right.y =
                            f64::from(bottom_right.y).min(f64::from(center.y) + room) as f32;
                    } else {
                        top_left.y = f64::from(top_left.y).max(f64::from(center.y) - room) as f32;
                    }
                }
            }
        }
        let size = bottom_right - top_left;
        if f64::from(size.x) < MIN_PLOT || f64::from(size.y) < MIN_PLOT {
            return Rect2::default();
        }
        Rect2::new(top_left, size)
    }

    pub fn remove_lot(&mut self, lot_id: LotId, out: &mut Outbox) -> bool {
        let Some(lot) = self.lots.shift_remove(&lot_id) else {
            return false;
        };
        self.lot_by_cell.shift_remove(&lot.cell);
        self.access_cache.shift_remove(&lot_id);
        out.push(SimEvent::LotsChanged);
        true
    }

    pub fn can_place(&self, cell: IVec2) -> bool {
        !self.lot_by_cell.contains_key(&cell) && grid::map_rect().has_point(grid::cell_center(cell))
    }

    pub fn lot_at_cell(&self, cell: IVec2) -> Option<LotId> {
        self.lot_by_cell.get(&cell).copied()
    }

    pub fn lots_of_type(&self, lot_type: LotType) -> Vec<&Lot> {
        self.lots
            .values()
            .filter(|lot| lot.lot_type == lot_type)
            .collect()
    }

    /// The near lane of the lot's road, or `None` if it has no road access.
    pub fn access(&mut self, roads: &RoadNetwork, lot_id: LotId) -> Option<LanePoint> {
        self.access_points(roads, lot_id).first().copied()
    }

    /// Every lane position in front of the lot: the near lane first, then the inner lane
    /// running the other way on the same road. Drivers may turn out of (and into) a
    /// driveway in either direction, so trips pick whichever is quickest
    /// (`Router::find_best_route`). Empty if the lot has no road access.
    pub fn access_points(&mut self, roads: &RoadNetwork, lot_id: LotId) -> Vec<LanePoint> {
        let Some(lot) = self.lots.get(&lot_id) else {
            return Vec::new();
        };
        // Road edits only clear the lots near them (on_roads_changed); a lane gone anyway
        // means this one was missed: work it out again rather than hand out a stale lane.
        let stale = match self.access_cache.get(&lot_id) {
            None => true,
            Some(cached) => cached
                .iter()
                .any(|point| !roads.lanes.contains_key(&point.lane_id)),
        };
        if stale {
            let points = Self::find_access_points(roads, lot.center());
            self.access_cache.insert(lot_id, points);
        }
        self.access_cache[&lot_id].clone()
    }

    pub fn to_dict(&self) -> Value {
        let lots: Vec<Value> = self
            .lots
            .values()
            .map(|lot| json!({ "id": lot.id.0, "cell": [lot.cell.x, lot.cell.y], "type": lot.lot_type as i32 }))
            .collect();
        json!({ "lots": lots, "next_id": self.next_id })
    }

    /// Replaces all lots with saved ones.
    pub fn restore(&mut self, roads: &RoadNetwork, data: &Value, out: &mut Outbox) {
        let int = |value: &Value| value.as_f64().unwrap_or(0.0) as i32;
        self.lots.clear();
        self.lot_by_cell.clear();
        self.access_cache.clear();
        for entry in data["lots"].as_array().into_iter().flatten() {
            let cell = IVec2::new(int(&entry["cell"][0]), int(&entry["cell"][1]));
            let mut lot = Lot::new(
                LotId(int(&entry["id"])),
                cell,
                LotType::from_index(int(&entry["type"])),
            );
            lot.plot = Self::compute_plot(roads, cell);
            self.insert(lot);
        }
        self.next_id = data.get("next_id").map_or(1, int);
        out.push(SimEvent::LotsChanged);
    }

    fn insert(&mut self, lot: Lot) {
        self.lot_by_cell.insert(lot.cell, lot.id);
        self.lots.insert(lot.id, lot);
    }

    fn find_access_points(roads: &RoadNetwork, center: Vec2) -> Vec<LanePoint> {
        let mut points = Vec::new();
        let Some(near) = roads.find_lane_position(center, ACCESS_DISTANCE, 0) else {
            return points;
        };
        let near_lane = &roads.lanes[&near.lane_id];
        points.push(LanePoint::new(
            near.lane_id,
            Self::set_back(near_lane, near.distance),
        ));
        // Across the road, only the lane next to the center line: turning left into or
        // out of the driveway, as drivers do.
        let segment = &roads.segments[&near_lane.segment_id.expect("a road lane")];
        for lane_id in &segment.lane_ids {
            let lane = &roads.lanes[lane_id];
            if lane.from_node_id != near_lane.from_node_id && lane.index_from_center == 0 {
                // Turning left across the road takes waiting for a gap: drivers avoid it
                // when the near side isn't much further.
                let distance = Self::set_back(lane, f64::from(lane.curve().closest_offset(center)));
                points.push(LanePoint {
                    lane_id: *lane_id,
                    distance,
                    penalty: FAR_SIDE_PENALTY,
                });
            }
        }
        points
    }

    /// `distance` along `lane`, kept DRIVEWAY_SETBACK from its ends (the middle, on a lane
    /// too short for that).
    fn set_back(lane: &Lane, distance: f64) -> f64 {
        if lane.length < DRIVEWAY_SETBACK * 2.0 {
            return lane.length * 0.5;
        }
        distance.clamp(DRIVEWAY_SETBACK, lane.length - DRIVEWAY_SETBACK)
    }

    pub fn on_roads_changed(&mut self, roads: &RoadNetwork, change: &ChangeSet, out: &mut Outbox) {
        // Where the changed roads were and are now, widened by as far as a road can reach
        // into a plot (half its width, the clearance and a cell) or a lot reach a road
        // (ACCESS_DISTANCE): lots outside that are untouched by the edit.
        let mut areas: Vec<Rect2> = Vec::new();
        for segment_id in &change.segment_ids {
            if let Some(bounds) = self.segment_bounds.shift_remove(segment_id) {
                areas.push(bounds.grow(EDIT_REACH as f32));
            }
            if let Some(segment) = roads.segments.get(segment_id) {
                self.segment_bounds.insert(*segment_id, segment.bounds());
                areas.push(segment.bounds().grow(EDIT_REACH as f32));
            }
        }
        // Roads may have taken land: re-fit those plots, and drop lots left without room.
        let mut removed = false;
        let size = grid::CELL_SIZE as f32;
        let ids: Vec<LotId> = self.lots.keys().copied().collect();
        for id in ids {
            let cell = self.lots[&id].cell;
            let cell_rect = Rect2::new(cell.as_vec2() * size, Vec2::splat(size));
            if !areas.iter().any(|area| area.intersects(&cell_rect)) {
                continue;
            }
            self.access_cache.shift_remove(&id);
            let plot = Self::compute_plot(roads, cell);
            self.lots[&id].plot = plot;
            if !plot.has_area() {
                self.lots.shift_remove(&id);
                self.lot_by_cell.shift_remove(&cell);
                removed = true;
            }
        }
        if removed {
            out.push(SimEvent::LotsChanged);
        }
    }
}
