// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! A crate that contains foundational building blocks for the IOTA Tangle.

#![deny(missing_docs)]

/// Types used for tangle configuration.
pub mod config;
/// Types that represent tangle events.
pub mod event;
/// Message flags.
pub mod flags;
/// Message data, including message flags.
pub mod metadata;
/// Types used to represent SEPs (Solid Entry Points).
pub mod solid_entry_point;
/// Types used for interoperation with a node's storage layer.
pub mod storage;
/// Milestone-enabled tangle type.
pub mod tangle;
/// The overall `TangleWorker` type. Used as part of the bee runtime in a node.
pub mod tangle_worker;
/// A worker that periodically cleans the tip pool.
pub mod tip_pool_cleaner_worker;
/// Common tangle traversal functionality.
pub mod traversal;
/// Types used to represent unreferenced messages.
pub mod unreferenced_message;
/// The URTS tips pool.
pub mod urts;

mod conflict;

use bee_runtime::node::{Node, NodeBuilder};

use self::tip_pool_cleaner_worker::TipPoolCleanerWorker;
pub use self::{conflict::ConflictReason, tangle::Tangle, tangle_worker::TangleWorker};

/// Initiate the tangle on top of the given node builder.
pub fn init<N: Node>(tangle_config: &config::TangleConfig, node_builder: N::Builder) -> N::Builder
where
    N::Backend: storage::StorageBackend,
{
    node_builder
        .with_worker_cfg::<TangleWorker>(tangle_config.clone())
        .with_worker::<TipPoolCleanerWorker>()
}
