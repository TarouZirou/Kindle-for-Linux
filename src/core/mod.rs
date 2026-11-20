//! Pure, side-effect free logic. Every function here has a formal model with
//! proofs in `proof/KindleCore/*.lean` and is differentially tested against
//! vectors produced by those Lean definitions (`tests/lean_vectors.rs`).

pub mod layout;
pub mod progress;
pub mod scroll;
pub mod zoom;
