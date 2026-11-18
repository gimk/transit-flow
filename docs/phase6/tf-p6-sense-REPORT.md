# W3 p6-sense report

## API (tf_sim::street::run::perceive)
- `Looker { mode: Mode, eye, dir, table, seed, fov, alternate }`, `.notices(who, target, tick, place) -> Option<Noticed>`.
  `mode` is car, bicycle or pedestrian. No W1 code is used. A cyclist gets its own `driver_table`-style table and a `Looker`.
- `in_blind_spot(mode, dir, to)`, `useful_fov(attention, risk)`, `reaction_s(base, surprise)`.
- Hearing: `sound_level(mode, speed, electric)`, `background_noise(moving)`, `hears(level, distance, noise)`, `is_deaf(seed, id, hurried)`. A bicycle is almost silent.
- Constants: `FULL_FOV`, `SURE_RANGE_M` (40 m), `GLANCE_TICKS`, `MEMORY_GLANCES`.
- No random stream is used: glances and chances come from a hash of ids and tick.

## Rules (user decision 2026-11-18)
- A fully attentive agent always notices a visible thing within 40 m, except blind spots and things behind.
- A shoulder check shows what is in a blind spot.
- Realistic misses: blind spots, hidden things, far periphery, agents that are not fully attentive.
- Walker: a standing vehicle in plain view is known; hearing counts moving vehicles only.

## Files
run/{perceive,agent,drive,events,kerb,meet,mod,spawn,walker,yielding}.rs, street/score.rs,
tests/{street_perceive,street_cross,street_meet,street_run,street_walk_choice}.rs.

## Tests
`cargo test -p tf_sim`: all pass (last full run before two tiny clippy fixes, then street_walk_choice and clippy re-run: pass). Clippy and fmt pass.
Replay hashes changed on purpose (seed 1 Mixed did not move).

## Test changes
- street_cross: careful walkers may rarely start with a car in the way (rate <= 10%).
- street_walk_choice (cut corner): a contact is allowed only with a walker-side cause (hurried walkers are deaf).
- a_death_ends_the_run: 3600 walkers/h (no death in 40 seeds at 1800/h: deaths are rare at street speed).

## Open points
- Rate tests for hidden cases are in street_meet (hidden kerb) and street_perceive (blind spot); a rate test of the van blind spot against a cyclist waits for W1.
- Cyclists should use `Mode::Bicycle` Looker, `sound_level` for hearing.
- Values are start values for Phase 8.

## Text for the plan
Phase 6 WP3 done: perception (additions 2, 3, 7) in `street/run/perceive.rs`: gaze by task, zones of the visual field, pillar and van blind spots with shoulder check, surprise (+0.8 s), hearing with street noise and deaf walkers. Fully attentive agents never miss a visible thing within 40 m. Replay hashes changed on purpose.
