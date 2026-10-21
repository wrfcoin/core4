// LEGACY — see README.md in this directory.
//
// The Rust files here (discovery_tests.rs, node_tests.rs, sync_tests.rs,
// hsm_integration_tests.rs) reference module paths from the old
// blockchain-core architecture (crate::node::discovery, crate::wallet::hsm,
// etc.) that do not exist in the current core4 workspace.  They cannot be
// wired into cargo without a substantial rewrite.
//
// Disposition: deferred to a follow-up issue.  Do NOT add a [lib] or [[test]]
// entry for this directory until the module paths are updated.

#[cfg(test)]
mod integration;
