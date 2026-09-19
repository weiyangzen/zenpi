//! Shared zenpi library. The binary exposes only TUI and headless modes.

pub mod approval;
pub mod b3;
pub mod backend;
pub mod config;
pub mod context;
pub mod core;
pub mod diagnostics;
pub mod directory_picker;
pub mod domain_execution;
pub mod domain_store;
pub mod domains;
pub mod error;
pub mod extension_runtime;
pub mod extensions;
pub mod folder_source;
pub mod governance;
pub mod headless;
pub mod input_queue;
pub mod layout;
pub mod net_probe;
pub mod persona;
pub mod project_workspace;
pub mod prompt_templates;
pub mod protocol;
pub mod providers;
pub mod render;
pub mod resource_loader;
pub mod resources;
pub mod runtime;
pub mod runtime_intent;
pub mod search;
pub mod security;
pub mod session;
pub mod session_tree;
pub mod skills;
pub mod slash;
pub mod slash_actions;
pub mod sync;
pub mod tool_output;
pub mod tool_runtime;
pub mod tools;
pub mod tui;
pub mod view_model;

#[cfg(unix)]
mod external_editor;
