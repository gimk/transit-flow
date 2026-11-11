//! Saves: the same JSON format as the Godot build (`Simulation.to_dict` / `from_dict`),
//! so a save made by either build loads in the other.

use serde_json::{Value, json};

use crate::Simulation;
use crate::roads::RoadNetwork;

impl Simulation {
    /// Citizens, vehicles and buses are not saved yet: on load everyone starts at home
    /// (bus stops and lines are saved).
    pub fn to_dict(&self) -> Value {
        json!({
            "version": Self::SAVE_VERSION,
            "tick_count": self.tick_count,
            "clock_minutes": self.clock.total_minutes,
            "roads": self.roads.to_dict(),
            "lots": self.lots.to_dict(),
            "zones": self.zones.to_dict(),
            "externals": self.externals.to_dict(),
            "transit": self.transit.to_dict(),
        })
    }

    /// `None` if the data is from an unsupported save version.
    pub fn from_dict(data: &Value) -> Option<Self> {
        let number = |value: &Value| value.as_f64().unwrap_or(0.0);
        let version = data.get("version").map_or(-1.0, number) as i32;
        if version != Self::SAVE_VERSION {
            return None;
        }
        let mut sim = Self::with_roads(RoadNetwork::from_dict(&data["roads"]));
        sim.tick_count = number(&data["tick_count"]) as u64;
        sim.clock.total_minutes = number(&data["clock_minutes"]);
        let empty = json!({});
        let part = |key: &str| data.get(key).unwrap_or(&empty).clone();
        // Restoring the lots tells the citizens, who move into the homes (in Godot the
        // same signal runs `_sync_with_lots`).
        let lots = part("lots");
        sim.call(|sim, out| sim.lots.restore(&sim.roads, &lots, out));
        let zones = part("zones");
        sim.call(|sim, out| sim.zones.restore(&sim.lots, &zones, out));
        let externals = part("externals");
        sim.call(|sim, out| sim.externals.restore(&externals, out));
        let transit = part("transit");
        let (system, mut ctx) = sim.transit_ctx();
        system.restore(&mut ctx, &transit);
        Some(sim)
    }
}
