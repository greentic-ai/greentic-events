use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use greentic_types::{ChannelMessageEnvelope, EventEnvelope};

use crate::error::EventBusError;

/// Bridge converting channel messages into events.
#[async_trait]
pub trait MessageToEventBridge: Send + Sync {
    async fn handle_message(&self, message: ChannelMessageEnvelope) -> Result<Vec<EventEnvelope>>;
}

/// Bridge converting events into channel messages.
#[async_trait]
pub trait EventToMessageBridge: Send + Sync {
    async fn handle_event(&self, event: EventEnvelope) -> Result<Vec<ChannelMessageEnvelope>>;
}

/// Factory used to instantiate bridge components.
#[async_trait]
pub trait BridgeFactory: Send + Sync {
    async fn build_message_to_event(&self, name: &str) -> Result<Arc<dyn MessageToEventBridge>>;

    async fn build_event_to_message(&self, name: &str) -> Result<Arc<dyn EventToMessageBridge>>;
}

/// Registry tracking available bridges.
#[derive(Default)]
pub struct BridgeRegistry {
    message_to_event: HashMap<String, Arc<dyn MessageToEventBridge>>,
    event_to_message: HashMap<String, Arc<dyn EventToMessageBridge>>,
}

impl BridgeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_message_to_event(
        &mut self,
        name: impl Into<String>,
        bridge: Arc<dyn MessageToEventBridge>,
    ) {
        self.message_to_event.insert(name.into(), bridge);
    }

    pub fn register_event_to_message(
        &mut self,
        name: impl Into<String>,
        bridge: Arc<dyn EventToMessageBridge>,
    ) {
        self.event_to_message.insert(name.into(), bridge);
    }

    pub fn message_to_event(&self, name: &str) -> Option<Arc<dyn MessageToEventBridge>> {
        self.message_to_event.get(name).cloned()
    }

    pub fn event_to_message(&self, name: &str) -> Option<Arc<dyn EventToMessageBridge>> {
        self.event_to_message.get(name).cloned()
    }
}

/// High-level API for invoking bridges.
#[derive(Default)]
pub struct BridgeService {
    registry: BridgeRegistry,
}

impl BridgeService {
    pub fn new(registry: BridgeRegistry) -> Self {
        Self { registry }
    }

    pub async fn handle_message(
        &self,
        name: &str,
        message: ChannelMessageEnvelope,
    ) -> Result<Vec<EventEnvelope>, EventBusError> {
        let bridge =
            self.registry
                .message_to_event(name)
                .ok_or_else(|| EventBusError::BridgeNotFound {
                    bridge: name.to_string(),
                })?;
        bridge
            .handle_message(message)
            .await
            .map_err(|err| EventBusError::BridgeError(err.to_string()))
    }

    pub async fn handle_event(
        &self,
        name: &str,
        event: EventEnvelope,
    ) -> Result<Vec<ChannelMessageEnvelope>, EventBusError> {
        let bridge =
            self.registry
                .event_to_message(name)
                .ok_or_else(|| EventBusError::BridgeNotFound {
                    bridge: name.to_string(),
                })?;
        bridge
            .handle_event(event)
            .await
            .map_err(|err| EventBusError::BridgeError(err.to_string()))
    }
}
