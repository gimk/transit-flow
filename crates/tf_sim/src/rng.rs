//! Seeded random numbers for the sim, bit-for-bit the same as Godot's
//! `RandomNumberGenerator` (PCG32 with Godot's seeding and float conversion), so a
//! system seeded like its GDScript original draws the same numbers.
//!
//! Only what the sim uses is ported: `seed`, `randi`, `randf`, `randf_range`. The
//! expected values in the tests were printed by Godot 4.7.2.

/// Godot's `PCG_DEFAULT_INC_64`: the stream every `RandomNumberGenerator` uses.
const DEFAULT_INC: u64 = 1_442_695_040_888_963_407;
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// A PCG32 generator seeded like Godot's `RandomNumberGenerator.seed = n`.
#[derive(Clone, Debug)]
pub struct SimRng {
    state: u64,
    inc: u64,
}

impl SimRng {
    /// Same as `RandomNumberGenerator.new()` followed by `rng.seed = seed`.
    pub fn new(seed: u64) -> Self {
        // pcg32_srandom_r(seed, DEFAULT_INC)
        let mut rng = Self {
            state: 0,
            inc: (DEFAULT_INC << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// GDScript `randi()`: a uniform 32-bit integer (an `int` in GDScript, so callers
    /// doing `randi() % n` get a non-negative result).
    pub fn randi(&mut self) -> i64 {
        i64::from(self.next_u32())
    }

    /// GDScript `randf()`: a float in [0, 1]. Godot treats one draw as the exponent
    /// (by its leading zeros) and the next as the significand, for finer values near 0.
    pub fn randf(&mut self) -> f32 {
        let proto_exp_offset = self.next_u32();
        if proto_exp_offset == 0 {
            return 0.0;
        }
        let significand = self.next_u32() | 0x8000_0001;
        let exponent = -32 - proto_exp_offset.leading_zeros() as i32;
        significand as f32 * 2f32.powi(exponent)
    }

    /// GDScript `randf_range(from, to)`.
    pub fn randf_range(&mut self, from: f32, to: f32) -> f32 {
        self.randf() * (to - from) + from
    }
}

#[cfg(test)]
// The expected values are copied as Godot printed them (9 digits).
#[allow(clippy::excessive_precision)]
mod tests {
    use super::SimRng;

    /// Godot 4.7.2: four `randi()`, four `randf()`, then `randf_range(2, 10)`.
    fn check(seed: u64, ints: [i64; 4], floats: [f32; 4], ranged: f32) {
        let mut rng = SimRng::new(seed);
        for expected in ints {
            assert_eq!(rng.randi(), expected, "seed {seed}");
        }
        for expected in floats {
            assert!(
                (rng.randf() - expected).abs() < 1e-8,
                "seed {seed}: expected {expected}"
            );
        }
        assert!((rng.randf_range(2.0, 10.0) - ranged).abs() < 1e-6);
    }

    #[test]
    fn matches_godot_sequences() {
        check(
            0,
            [881477183, 1327520283, 692503688, 2153658078],
            [0.358580858, 0.168866158, 0.859284401, 0.472052306],
            7.669811249,
        );
        check(
            7,
            [1352667803, 3706616521, 2188350268, 3036842621],
            [0.900402844, 0.670574307, 0.800319552, 0.755683362],
            2.384271145,
        );
        check(
            11,
            [3351202525, 1402821896, 1385530632, 1371404858],
            [0.295134485, 0.020639274, 0.094243959, 0.477473915],
            8.654043198,
        );
        check(
            23,
            [2759913527, 1681500467, 55096380, 4252518940],
            [0.828000546, 0.653066754, 0.807484806, 0.902796388],
            9.023698807,
        );
        check(
            123456789,
            [1712221112, 557107306, 1131688667, 13764607],
            [0.830873191, 0.886955142, 0.241405994, 0.803440750],
            7.185671806,
        );
    }
}
