// Core crate surface for reen.
//
// The generated project scaffold lives under the implementation pipeline, but
// the root crate itself exposes the real runtime modules used by the CLI and
// integration tests.

pub mod build_tracker;
mod contexts;
pub mod execution;
pub mod registries;
