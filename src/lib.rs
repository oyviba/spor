//! spor's frontend-agnostic core: git plumbing, lane layout, branch colors,
//! remote-host (PR) integration, GitHub, agent tasks, and the work items
//! that join them. The app in `desktop/` (Tauri) is a thin shell over this.

pub mod agent;
pub mod askpass;
pub mod checkpoint;
pub mod color;
pub mod conflict;
pub mod debrief;
pub mod env;
pub mod git;
pub mod github;
pub mod graph;
pub mod host;
pub mod json;
pub mod mcp;
pub mod patch;
pub mod remote;
pub mod tasks;
pub mod work;
