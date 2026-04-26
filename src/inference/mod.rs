//! Scenario-driven inference passes that run during `reen prepare`.
//!
//! The compile-parity sprint added two passes — signature derivation
//! (bug 5) and multiplicity inference (bug 6) — that read scenarios on
//! a prepared artefact and annotate operations / functionalities /
//! attributes with the resulting Rust shape. Both used to live in
//! `src/prepare.rs`; this module collects them so prepare.rs can stick
//! to vocabulary resolution, document promotion, and tracker
//! integration.
//!
//! ## Design decision: explicit allow-lists, no regex magic
//!
//! Every classifier in this module — return-shape phrases, count words,
//! query-parameter shapes, plurality cues — is an explicit string
//! allow-list. New shapes are added by extending the allow-list, not
//! by widening a pattern. The motivation: BA prose drifts in small
//! ways across drafts, and a regex that "almost" matches is hard to
//! audit when it produces a wrong inference. An explicit list makes
//! the supported shapes legible at review time and forces a
//! deliberate decision when the vocabulary expands.
//!
//! When no shape matches, we leave the field unset (`None` for
//! signatures, `Single` for multiplicity, empty for query parameters)
//! and let the codegen path fall back to its existing default. The BA
//! sees no error; the implementer sees a `// TODO` comment in the
//! scaffolded code.
//!
//! ## Layout
//!
//! - [`signature`] — `derive_signatures`, the per-kind inferrers, and
//!   `aggregate_signatures`. Reads scenarios; writes
//!   `OperationSpec.derived_signature` /
//!   `FunctionalitySpec.derived_signature`.
//! - [`multiplicity`] — `derive_multiplicity` and the three cue
//!   helpers (name, prose, scenario). Writes
//!   `RolePlayerSpec.multiplicity` and `AttributeSpec.multiplicity`
//!   (including per-variant attributes).
//! - [`phrases`] — return-shape classifiers and rejection-detection
//!   helpers shared by signature inference.
//! - [`query_params`] — projection-query parameter parsing
//!   (`with minimum N and maximum M`, `with "alice"`).

pub mod multiplicity;
pub mod phrases;
pub mod query_params;
pub mod signature;

pub use multiplicity::derive_multiplicity;
pub use signature::derive_signatures;
