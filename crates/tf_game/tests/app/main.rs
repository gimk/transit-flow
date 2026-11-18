//! All the `tf_game` integration tests in one test executable: every test file is a module
//! here, so Bevy is linked once instead of once per file. Run one group with
//! `cargo test -p tf_game --test app <module>::` (for example `play::`).
//!
//! The tests are threads of one process: anything process-global (the data folder in
//! `TRANSIT_FLOW_DATA_DIR`) comes from `common`.

mod common;

mod bicycle_tools;
mod camera_input;
mod play;
mod street_runner;
mod street_view;
