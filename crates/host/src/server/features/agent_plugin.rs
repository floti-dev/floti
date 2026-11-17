//! `agentPlugin:offer`, `agentPlugin:call`, `agentPlugin:answer` and
//! `agentPlugin:cancel`, answered with nothing.
//!
//! The protocol keeps the channels declared, so its version is unchanged,
//! and no client sends on them: every plugin's tools are served by the host
//! itself ([`super::super::plugin_tools::HostPluginTools`]). An offer or an
//! answer is taken and changes nothing, and the two event channels can be
//! subscribed to and never carry an event.

use std::sync::Arc;

use floti_protocol::channel::Router;
use floti_protocol::channels::agent_plugin::{
    AgentPluginAnswer, AgentPluginCalls, AgentPluginCancels, AgentPluginOffer,
};

use super::{Caller, Feature};
use crate::server::events::Events;

/// The `agentPlugin:*` channels, served and unused.
pub(crate) struct AgentPluginChannels;

impl AgentPluginChannels {
    /// Serves `agentPlugin:call` and `agentPlugin:cancel` on `events`, where
    /// nothing is ever emitted.
    pub fn new(events: &Arc<Events>) -> Self {
        events.serve_per_client::<AgentPluginCalls>();
        events.serve_per_client::<AgentPluginCancels>();
        Self
    }
}

impl Feature for AgentPluginChannels {
    fn requests<'a>(&'a self, _caller: Caller, router: &mut Router<'a>) {
        router
            .handle::<AgentPluginOffer>(|_offer| Ok(()))
            .handle::<AgentPluginAnswer>(|_answer| Ok(()));
    }
}
