//! spor's frontend-agnostic core: git plumbing, lane layout, branch colors and
//! remote-host (PR) integration. The terminal UI (`spor`) and the native app
//! (`spor-app`, behind the `gui` feature) are both thin shells over this.

pub mod color;
pub mod diff;
pub mod git;
pub mod graph;
pub mod remote;
