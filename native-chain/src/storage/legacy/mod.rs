//! Frozen historical persistence layouts (#3091).
//!
//! Every module here is a byte-exact copy of the serialized `State` layout
//! as it was at a named commit, kept so [`super::migrations`] can decode a
//! checkpoint written by that era's binary. **Never edit a frozen module.**
//! When `CHECKPOINT_FORMAT_VERSION` is bumped, add `state_v<N>.rs` for the
//! layout being retired (N = the version just superseded) and register the
//! N → N+1 migration; the previous modules stay exactly as they are.
//!
//! Nested types a frozen layout shares with the current `State` are named by
//! their current path rather than copied, because bincode is positional and
//! a type's encoding is defined by its field list, not its name. The golden
//! fixture for that version (`tests/fixtures/checkpoint_state_v<N>_golden.bin`)
//! is the guard: the moment a shared nested type changes shape, the frozen
//! decode of the golden fails and the author must freeze that type here too.
//!
//! Zero non-test callers outside `migrations.rs`, by design.

pub mod state_v3;
pub mod state_v4;
pub mod state_v5;
pub mod state_v6;
pub mod state_v7;
pub mod state_v8;
pub mod state_v9;

pub mod state_v10;

pub mod state_v11;
pub mod state_v12;

pub mod state_v13;
pub mod state_v14;
pub mod state_v15;
