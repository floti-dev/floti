//! What composition reads of an agent's config: `config:scopes`,
//! `config:tree` and `config:effective`.
//!
//! The reader is built for the first backend whose config source says where
//! the agent keeps its config
//! ([`ConfigSource::agent_config`](crate::agent::ConfigSource::agent_config)),
//! the way history is indexed from the first backend that declares history.
//! Every channel is served whether or not such a backend exists; with none,
//! each is answered with a sentence saying so. None of them writes anything.

use std::sync::Arc;

use floti_protocol::channel::Router;
use floti_protocol::channels::workspace::{ListConfigScopes, ReadConfigTree, ReadEffectiveView};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::{Caller, Feature, FeatureContext};
use crate::agent_config::{ConfigReader, NO_AGENT_CONFIG};

pub(crate) struct Config {
    reader: Option<ConfigReader>,
}

impl Config {
    pub fn new(context: &FeatureContext) -> crate::Result<Self> {
        let reader = context
            .hosted
            .backends()
            .iter()
            .find_map(|backend| Some((Arc::clone(backend), backend.config()?.agent_config()?)))
            .map(|(backend, agent)| {
                ConfigReader::new(backend, agent, context.hosted.ledger().shared_store())
            });
        Ok(Self { reader })
    }

    fn reader(&self) -> Result<&ConfigReader, String> {
        self.reader
            .as_ref()
            .ok_or_else(|| NO_AGENT_CONFIG.to_owned())
    }
}

impl Feature for Config {
    fn requests<'a>(&'a self, _: Caller, router: &mut Router<'a>) {
        router
            .handle::<ListConfigScopes>(|()| self.reader()?.scopes())
            .handle::<ReadConfigTree>(|scope| self.reader()?.tree(&scope.scope_path))
            .handle::<ReadEffectiveView>(|request| self.reader()?.effective(&request));
    }
}

/// Runs `work`, which may wait on a child process for a long time, without
/// holding up a runtime worker: on a multi-threaded runtime the worker's
/// other tasks move to another thread while it runs.
pub(super) fn off_the_workers<T>(work: impl FnOnce() -> T) -> T {
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(work)
        }
        _ => work(),
    }
}
