//! All the `tf_game` integration tests in one test executable: every test file is a module
//! here, so Bevy is linked once instead of once per file. Run one group with
//! `cargo test -p tf_game --test app <module>::` (for example `tools::`).
//!
//! The tests are threads of one process: anything process-global (the save folder in
//! `TRANSIT_FLOW_DATA_DIR`) comes from `common`.

mod common;

mod camera_input;
mod debug_keys;
mod map_views;
mod moving_views;
mod play;
mod playtest_parity;
mod runner;
mod status_line;
mod street_runner;
mod street_view;
mod tools;
mod transit_views;
mod view_snapshot;
