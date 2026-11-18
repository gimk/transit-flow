//! Street cross-sections (pivot plan §3.1): a profile is the ordered list of strips from
//! the left building line to the right one, looking along the segment's centerline from
//! its first point to its last. The total width is fixed by the level; reallocating the
//! space is editing this list. Metres throughout.

use serde::{Deserialize, Serialize};

/// Agent classes, as bits, so a strip or a movement can allow several.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Access(pub u8);

impl Access {
    pub const NONE: Access = Access(0);
    pub const PEDESTRIAN: Access = Access(1);
    pub const BICYCLE: Access = Access(2);
    pub const CAR: Access = Access(4);
    pub const BUS: Access = Access(8);
    /// Delivery vans and trucks.
    pub const DELIVERY: Access = Access(16);
    /// Everything with an engine.
    pub const MOTOR: Access = Access(4 | 8 | 16);

    pub fn contains(self, other: Access) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Access) -> bool {
        self.0 & other.0 != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn without(self, other: Access) -> Access {
        Access(self.0 & !other.0)
    }
}

impl std::ops::BitOr for Access {
    type Output = Access;
    fn bitor(self, other: Access) -> Access {
        Access(self.0 | other.0)
    }
}

impl std::ops::BitAnd for Access {
    type Output = Access;
    fn bitand(self, other: Access) -> Access {
        Access(self.0 & other.0)
    }
}

/// What a strip is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StripUse {
    Sidewalk,
    CarLane,
    BusLane,
    /// A painted bicycle lane: a careless driver can cross the paint.
    BikeLane,
    /// A bicycle track: like a lane, but its separators are meant to be physical.
    BikeTrack,
    Parking,
    DeliveryBay,
    Median,
    Planting,
    Terrace,
}

impl StripUse {
    /// Vehicles travel along it, so it needs a direction and gets a lane.
    pub fn is_travel(self) -> bool {
        matches!(
            self,
            StripUse::CarLane | StripUse::BusLane | StripUse::BikeLane | StripUse::BikeTrack
        )
    }

    /// Who may use it by the rules. In France cyclists may ride in car lanes, and Paris
    /// bus lanes are open to cyclists.
    pub fn default_access(self) -> Access {
        match self {
            StripUse::Sidewalk => Access::PEDESTRIAN,
            StripUse::CarLane => Access::MOTOR | Access::BICYCLE,
            StripUse::BusLane => Access::BUS | Access::BICYCLE,
            StripUse::BikeLane | StripUse::BikeTrack => Access::BICYCLE,
            StripUse::Terrace => Access::PEDESTRIAN,
            StripUse::Parking | StripUse::DeliveryBay | StripUse::Median | StripUse::Planting => {
                Access::NONE
            }
        }
    }
}

/// The direction of travel of a travel strip, relative to its segment's centerline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// Not a travel strip.
    #[default]
    None,
    /// From the centerline's first point to its last.
    Forward,
    Backward,
}

/// The edge of a strip on one side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Separator {
    #[default]
    None,
    Paint,
    Kerb,
    Bollards,
    Planter,
}

/// What it takes to cross a separator for one class of agent. The drive code reads it: a
/// careful agent never crosses `Paint`, a careless one may (to park, stop or pass);
/// nobody crosses `Blocked`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Crossability {
    /// Nothing in the way and no marking.
    Free,
    /// A painted line: physically possible, against the rules for vehicles.
    Paint,
    /// A physical barrier: the class cannot cross.
    Blocked,
}

impl Separator {
    /// How hard this separator is to cross for `mode` (one or more `Access` classes; with
    /// several, the hardest one counts, so `Free` means every class crosses freely).
    ///
    /// - `None`: free for all.
    /// - `Paint`: vehicles (bicycle, car, bus, delivery) cross it only by breaking the
    ///   rule; a pedestrian walks over it.
    /// - `Kerb`: blocks every vehicle, bicycles too (a cyclist does not hop a kerb);
    ///   pedestrians step over it.
    /// - `Bollards`: block motor vehicles only. The gaps (a row of thin posts) let
    ///   bicycles and pedestrians through, so a cyclist can leave a track at a gap.
    /// - `Planter`: a continuous box that blocks everyone.
    pub fn crossability(self, mode: Access) -> Crossability {
        let mut hardest = Crossability::Free;
        for class in [
            Access::PEDESTRIAN,
            Access::BICYCLE,
            Access::CAR,
            Access::BUS,
            Access::DELIVERY,
        ] {
            if !mode.intersects(class) {
                continue;
            }
            let wheeled = class != Access::PEDESTRIAN;
            let motor = Access::MOTOR.intersects(class);
            let here = match self {
                Separator::None => Crossability::Free,
                Separator::Paint if wheeled => Crossability::Paint,
                Separator::Paint => Crossability::Free,
                Separator::Kerb if wheeled => Crossability::Blocked,
                Separator::Kerb => Crossability::Free,
                Separator::Bollards if motor => Crossability::Blocked,
                Separator::Bollards => Crossability::Free,
                Separator::Planter => Crossability::Blocked,
            };
            hardest = hardest.max(here);
        }
        hardest
    }

    /// Kerb, bollards and planter: what makes a bicycle track a track.
    pub fn is_physical(self) -> bool {
        matches!(
            self,
            Separator::Kerb | Separator::Bollards | Separator::Planter
        )
    }
}

impl StripUse {
    /// A strip that wheeled traffic cannot drive over to reach the strips behind it (see
    /// `Street::crossable_between`). Free parking spots and delivery bays can be driven
    /// over; sidewalks, terraces, planting and medians cannot.
    pub fn blocks_wheels(self) -> bool {
        matches!(
            self,
            StripUse::Sidewalk | StripUse::Terrace | StripUse::Planting | StripUse::Median
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Strip {
    #[serde(rename = "use")]
    pub use_: StripUse,
    pub width: f32,
    #[serde(default)]
    pub direction: Direction,
    /// Overrides `use_.default_access()`.
    #[serde(default)]
    pub access: Option<Access>,
    #[serde(default)]
    pub left: Separator,
    #[serde(default)]
    pub right: Separator,
    /// The level forbids the player to change it (for example a bus lane that must stay).
    #[serde(default)]
    pub locked: bool,
}

impl Strip {
    pub fn access(&self) -> Access {
        self.access.unwrap_or_else(|| self.use_.default_access())
    }
}

/// The strips of a segment, from the left building line to the right one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Profile {
    pub strips: Vec<Strip>,
}

impl Profile {
    pub fn total_width(&self) -> f32 {
        self.strips.iter().map(|strip| strip.width).sum()
    }

    /// The span of strip `index` as offsets from the centerline (positive = right of the
    /// centerline's direction), assuming the centerline runs down the middle of the
    /// profile.
    pub fn strip_span(&self, index: usize) -> (f32, f32) {
        let before: f32 = self.strips[..index].iter().map(|strip| strip.width).sum();
        let left = before - self.total_width() * 0.5;
        (left, left + self.strips[index].width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(use_: StripUse, width: f32) -> Strip {
        Strip {
            use_,
            width,
            direction: Direction::None,
            access: None,
            left: Separator::None,
            right: Separator::None,
            locked: false,
        }
    }

    #[test]
    fn spans_are_centered_on_the_centerline() {
        let profile = Profile {
            strips: vec![
                strip(StripUse::Sidewalk, 3.0),
                strip(StripUse::CarLane, 3.0),
                strip(StripUse::CarLane, 3.0),
                strip(StripUse::Sidewalk, 2.0),
            ],
        };
        assert_eq!(profile.total_width(), 11.0);
        assert_eq!(profile.strip_span(0), (-5.5, -2.5));
        assert_eq!(profile.strip_span(3), (3.5, 5.5));
    }

    #[test]
    fn separators_decide_who_can_cross() {
        use Crossability::*;
        let cases = [
            (Separator::None, [Free, Free, Free]),
            (Separator::Paint, [Free, Paint, Paint]),
            (Separator::Kerb, [Free, Blocked, Blocked]),
            (Separator::Bollards, [Free, Free, Blocked]),
            (Separator::Planter, [Blocked, Blocked, Blocked]),
        ];
        for (separator, [walk, bike, car]) in cases {
            assert_eq!(separator.crossability(Access::PEDESTRIAN), walk);
            assert_eq!(separator.crossability(Access::BICYCLE), bike);
            assert_eq!(separator.crossability(Access::CAR), car);
        }
        assert_eq!(
            Separator::Bollards.crossability(Access::BICYCLE | Access::CAR),
            Blocked
        );
        assert_eq!(Separator::Paint.crossability(Access::NONE), Free);
        assert!(Separator::Kerb.is_physical() && !Separator::Paint.is_physical());
    }

    #[test]
    fn access_bits() {
        let car_lane = StripUse::CarLane.default_access();
        assert!(car_lane.contains(Access::CAR | Access::BICYCLE));
        assert!(!car_lane.intersects(Access::PEDESTRIAN));
        assert_eq!(car_lane.without(Access::MOTOR), Access::BICYCLE);
        let mut bus_lane = strip(StripUse::BusLane, 3.5);
        assert!(!bus_lane.access().intersects(Access::CAR));
        bus_lane.access = Some(Access::BUS);
        assert_eq!(bus_lane.access(), Access::BUS);
    }
}
