# Godot baseline

Behaviour metrics of the Godot build (4.7.2-stable (official)), recorded by `migration/run_baseline.gd` on 2026-11-11. The Rust port must land inside the **band** column (plan §3). The RNG is bit-exact, so the Rust tests should run the same scenarios with the same seeds; where a metric is outside its band, find the first tick where the two runs diverge.

## Town, one day from 06:00

`Scenarios.build_town`, 5760 ticks, the safety checks on every tick (as in `test_town_lives_through_a_whole_day`).

| Metric | Godot | Band |
|---|---|---|
| Citizens (cars / no car) | 176 (136 / 40) | exact |
| Flow % | 60.3 | ± 5 points |
| Failed vehicle trips | 2 | ≤ Godot + 3 |
| Vehicle arrivals | 573 | ± 10% |
| Truck trips | 261 | ± 15% |
| Highways used for exports | 2 | exact |
| Peak vehicles | 165 | ± 15% |
| 95% of drivers at work (hour) | 12.37 | ± 1 h |
| Commuters who reached work | 175 | ± 5 |
| Median commute (× ideal) | 1.77 | ± 0.3 |
| Mode share: car | 69.6% | ± 5 points |
| Mode share: bus | 5.4% | ± 5 points |
| Mode share: walk | 25.0% | ± 5 points |
| Conflict-zone violations | 0 | 0 |
| Smallest queue gap (px) | 4.00 | ≥ 3.99 |
| Most vehicles stuck 30 s+ | 0 | 0 if Godot is 0 |

## Bottleneck mornings

`test_bottleneck.gd`'s `_morning_for_seed`: the citizens' RNG seeded with the given seed before `build_bottleneck` (origin (30, 30)), run until every commuter has reached work (max 300 s). *Driving* = total time commuters spent driving to work; *took bypass* = commuters who drove on a bypass road.

| Seed | Bypass | Everyone at work (s) | Driving (s) | Took bypass | Band |
|---|---|---|---|---|---|
| 1 | no | 80.4 | 1169 | 0 | time ± 15%, driving ± 15% |
| 1 | yes | 82.1 | 1187 | 2 | time ± 15%, driving ± 15% |
| 2 | no | 88.7 | 814 | 0 | time ± 15%, driving ± 15% |
| 2 | yes | 87.9 | 883 | 2 | time ± 15%, driving ± 15% |
| 3 | no | 89.8 | 977 | 0 | time ± 15%, driving ± 15% |
| 3 | yes | 89.8 | 979 | 1 | time ± 15%, driving ± 15% |

## Bottleneck, Flow after the morning

Default seeds, 2400 ticks (120 s, 10 game hours) from the start of the scenario, by which time every commuter has reached work.

| Setup | Flow % | Vehicle arrivals | Mode shares | Band |
|---|---|---|---|---|
| plain | 50.2 | 39 | car 70%, walk 30% | Flow ± 5 points, arrivals ± 10% |
| bypass | 50.5 | 39 | car 70%, walk 30% | Flow ± 5 points, arrivals ± 10% |
| bus line (curbside) | 52.8 | 44 | car 70%, bus 25%, walk 5% | Flow ± 5 points, arrivals ± 10% |
| bus lanes | 54.3 | 49 | car 70%, bus 25%, walk 5% | Flow ± 5 points, arrivals ± 10% |

## Stress grids

Random trips (`StressTraffic`, seed 0) on a 5 × 5 grid of roads 3 cells apart, safety checks on every tick. The first two rows are `test_traffic.gd`'s `_stress_grid` (local streets, 250 vehicles, 120 s, every junction of 3+ roads under the control); the others `test_road_types.gd`'s `_stress_grid` (even / odd rows and columns of the two road types, one-way ones alternating direction, 200 vehicles, 90 s).

| Grid | Vehicles at end | Arrivals | Stuck 30 s+ | Smallest gap | Violations | Band |
|---|---|---|---|---|---|---|
| local, none, 250 | 250 | 507 | 0 | 4.00 | 0 | see below |
| local, traffic_light, 250 | 249 | 622 | 0 | 4.00 | 0 | see below |
| avenue_4lane / local_2lane, 200 | 200 | 491 | 0 | 4.00 | 0 | see below |
| local_2lane / one_way_2lane / one_way_1lane, 200 | 199 | 283 | 0 | 4.00 | 0 | see below |
| avenue_4lane / one_way_2lane / one_way_1lane, 200 | 199 | 419 | 0 | 4.00 | 0 | see below |

Bands: arrivals ± 15%, vehicles at end ± 15%, stuck 0 where Godot has 0, smallest gap ≥ 3.99, violations 0.

Recorded in 32 s.

## Traffic benchmark

`benchmarks/traffic_benchmark.gd` on this Mac (Apple M1, headless, GDScript). M4.8 runs the Rust port of it and compares: the go / no-go point is at least 5× faster at 2 000 vehicles.

Grid: 20 x 20 roads, 3 cells apart
| target | vehicles | avg tick ms | worst tick ms | render fill ms | stuck >30s | arrivals/s |
|---|---|---|---|---|---|---|
| 500 | 500 | 4.93 | 13.62 | 0.22 | 0 | 0.9 |
| 1000 | 1000 | 10.53 | 16.80 | 0.41 | 0 | 1.2 |
| 2000 | 2000 | 26.50 | 57.40 | 1.03 | 0 | 1.8 |
| 5000 | 5000 | 48.91 | 65.40 | 2.31 | 0 | 2.6 |
