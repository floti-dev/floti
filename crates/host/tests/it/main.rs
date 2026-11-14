//! The host's integration tests, one module per feature, built as one test binary (`it`).
//!
//! Every integration test file is a binary of its own unless it is a
//! module of one: each would link the whole crate again, so one change
//! under `src` would relink every one of them. A module's tests run with
//! `--test it -E 'test(<module>::)'`.

mod common;

mod add;
mod agent_config;
mod agent_definitions;
mod agent_tools;
mod agents;
mod attached_devices;
mod bridge;
mod carry;
mod content;
mod discovery;
mod fake_agent;
mod fake_agent_input;
mod files;
mod forwards;
mod hearing;
mod highlighting;
mod history;
mod host;
mod host_plugin_tools;
mod install_self;
mod keeper;
mod noise_sessions;
mod notices;
mod os_conditionals;
mod plugin_background;
mod plugin_exec;
mod plugin_fetch;
mod plugin_pages;
mod plugin_runtime;
mod plugin_secrets;
mod plugin_services;
mod plugins;
mod profiles;
mod protocol_versions;
mod remote;
mod resources;
mod session_records;
mod sessions;
mod settings;
mod setup;
mod templates;
mod typing_paths;
mod uninstall;
mod update;
mod update_stop;
mod usage;
mod watch_attach;
mod worktrees;
mod wsl;
