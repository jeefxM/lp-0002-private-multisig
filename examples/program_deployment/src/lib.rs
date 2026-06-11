//! Shared demo fixtures + helpers for the LP-0002 multisig runner bins.
//!
//! The five `run_*` bins (see `src/bin/`) all consume [`msig_demo`] so their inputs compose
//! into one valid chain. This lib target exists only to share that fixture across the bins.

pub mod msig_demo;
