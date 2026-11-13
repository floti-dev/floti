//! The protocol crate's integration tests, one module per feature, built as one test binary (`it`).
//!
//! Every integration test file is a binary of its own unless it is a
//! module of one: each would link the whole crate again, so one change
//! under `src` would relink every one of them. A module's tests run with
//! `--test it -E 'test(<module>::)'`.

mod agent_plugin;
mod agents_channels;
mod backends;
mod capabilities;
mod capability_delivery;
mod carry;
mod channels;
mod flow_control;
mod frame_limits;
mod framing;
mod generated_files;
mod ipc_catalogue;
mod major_3_0;
mod major_4_0;
mod minor_2_2;
mod minor_2_3;
mod noise;
mod previous_minor;
mod version_negotiation;
mod version_record;
