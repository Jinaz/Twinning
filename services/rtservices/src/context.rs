//! RT and Non-RT contexts passed to services during execution.

use std::sync::Arc;
use crossbeam_channel::Sender;

use crate::engine_event::EngineEvent;
use crate::properties::Properties;

/// RT context passed by reference into tick(). Not Clone, lifetime-bound.
pub struct RtContext<'a> {
    pub props: &'a Properties,
    pub event_tx: &'a Sender<EngineEvent>,
}

/// Non-RT context. Clone-able, owns Arcs.
#[derive(Clone)]
pub struct NonRtContext {
    pub props: Arc<Properties>,
    pub event_tx: Sender<EngineEvent>,
}
