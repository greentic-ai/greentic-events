use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use greentic_events::EventProvider;
use greentic_events::SubscriptionOptions;
use greentic_events::bridge::BridgeFactory;
use greentic_events::runner::{
    RunnerBridgeClient, RunnerBrokerClient, RunnerBrokerProvider, RunnerWitBridgeFactory,
    WitAttachment, WitChannelMessageEnvelope, WitEventEnvelope, WitSubscriptionOptions,
};
use greentic_events::subscription::AckMode;
use greentic_types::{
    ChannelMessageEnvelope, EventEnvelope, EventId, EventProviderDescriptor, EventProviderKind,
    OrderingKind, ReliabilityKind, TransportKind,
};
use serde_json::json;
use tokio::time::timeout;

mod support;
use support::tenant_ctx;

#[derive(Default)]
struct FakeRunnerBrokerClient {
    published: Arc<Mutex<Vec<WitEventEnvelope>>>,
    queue: Arc<Mutex<VecDeque<WitEventEnvelope>>>,
    acks: Arc<Mutex<Vec<(String, String)>>>,
}

#[async_trait::async_trait]
impl RunnerBrokerClient for FakeRunnerBrokerClient {
    async fn publish(&self, event: WitEventEnvelope) -> anyhow::Result<()> {
        self.published.lock().unwrap().push(event);
        Ok(())
    }

    async fn subscribe(
        &self,
        topic: &str,
        _opts: WitSubscriptionOptions,
    ) -> anyhow::Result<String> {
        Ok(format!("sub-{topic}"))
    }

    async fn next_event(
        &self,
        _subscription: &str,
        _timeout: Duration,
    ) -> anyhow::Result<Option<WitEventEnvelope>> {
        Ok(self.queue.lock().unwrap().pop_front())
    }

    async fn ack_event(&self, subscription: &str, event_id: &str) -> anyhow::Result<()> {
        self.acks
            .lock()
            .unwrap()
            .push((subscription.to_string(), event_id.to_string()));
        Ok(())
    }
}

fn descriptor(name: &str) -> EventProviderDescriptor {
    EventProviderDescriptor {
        name: name.to_string(),
        kind: EventProviderKind::Broker,
        transport: TransportKind::Nats,
        reliability: ReliabilityKind::AtLeastOnce,
        ordering: OrderingKind::PerKey,
        notes: None,
        tags: vec![],
    }
}

#[tokio::test]
async fn runner_broker_provider_handles_publish_and_subscribe() {
    let client = Arc::new(FakeRunnerBrokerClient::default());
    let client_trait: Arc<dyn RunnerBrokerClient> = client.clone();
    let provider =
        RunnerBrokerProvider::new(descriptor("runner"), vec!["topic.*".into()], client_trait);

    let event = EventEnvelope {
        id: EventId::new("evt-wit").unwrap(),
        topic: "topic.demo".into(),
        r#type: "t".into(),
        source: "runner".into(),
        tenant: tenant_ctx(),
        subject: Some("subject".into()),
        time: Utc::now(),
        correlation_id: None,
        payload: json!({"hello": "world"}),
        metadata: [("k".into(), "v".into())].into_iter().collect(),
    };

    provider.publish(event.clone()).await.unwrap();
    assert_eq!(client.published.lock().unwrap().len(), 1);

    client.queue.lock().unwrap().push_back(WitEventEnvelope {
        id: event.id.to_string(),
        topic: event.topic.clone(),
        r#type: event.r#type.clone(),
        source: event.source.clone(),
        tenant: event.tenant.clone(),
        subject: event.subject.clone(),
        time: event.time.to_rfc3339(),
        correlation_id: None,
        payload_json: serde_json::to_string(&event.payload).unwrap(),
        metadata: event.metadata.clone(),
    });

    let mut handle = provider
        .subscribe(
            "topic.demo",
            tenant_ctx(),
            SubscriptionOptions {
                durable: true,
                deliver_existing: true,
                ack_mode: AckMode::Auto,
            },
        )
        .await
        .unwrap();

    let received = timeout(Duration::from_millis(200), handle.next())
        .await
        .expect("timeout waiting for event")
        .expect("event missing");
    assert_eq!(received.id, event.id);
    assert_eq!(client.acks.lock().unwrap().len(), 1);
}

#[derive(Default)]
struct FakeRunnerBridgeClient {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl RunnerBridgeClient for FakeRunnerBridgeClient {
    async fn handle_message(
        &self,
        name: &str,
        message: WitChannelMessageEnvelope,
    ) -> anyhow::Result<Vec<WitEventEnvelope>> {
        self.calls.lock().unwrap().push(format!("msg:{name}"));
        Ok(vec![WitEventEnvelope {
            id: format!("evt-{}", message.id),
            topic: format!("converted.{}", message.channel),
            r#type: "bridge".into(),
            source: "runner".into(),
            tenant: message.tenant,
            subject: Some(message.session_id),
            time: Utc::now().to_rfc3339(),
            correlation_id: None,
            payload_json: "\"ok\"".into(),
            metadata: message.metadata,
        }])
    }

    async fn handle_event(
        &self,
        name: &str,
        event: WitEventEnvelope,
    ) -> anyhow::Result<Vec<WitChannelMessageEnvelope>> {
        self.calls.lock().unwrap().push(format!("evt:{name}"));
        Ok(vec![WitChannelMessageEnvelope {
            id: format!("msg-{}", event.id),
            tenant: event.tenant,
            channel: event.topic,
            session_id: event.subject.unwrap_or_else(|| "sess".into()),
            user_id: None,
            text: Some(event.payload_json),
            attachments: vec![WitAttachment {
                mime_type: "text/plain".into(),
                url: "https://example.invalid".into(),
                name: Some("payload".into()),
                size_bytes: None,
            }],
            metadata: event.metadata,
        }])
    }
}

#[tokio::test]
async fn runner_bridge_factory_converts_both_directions() {
    let client = Arc::new(FakeRunnerBridgeClient::default());
    let client_trait: Arc<dyn RunnerBridgeClient> = client.clone();
    let factory = RunnerWitBridgeFactory::new(client_trait);

    let msg_bridge = factory
        .build_message_to_event("bridge")
        .await
        .expect("build msg bridge");
    let evt_bridge = factory
        .build_event_to_message("bridge")
        .await
        .expect("build evt bridge");

    let msg = ChannelMessageEnvelope {
        id: "m1".into(),
        tenant: tenant_ctx(),
        channel: "chat".into(),
        session_id: "sess".into(),
        user_id: None,
        text: Some("hello".into()),
        attachments: vec![],
        metadata: [("k".into(), "v".into())].into_iter().collect(),
    };
    let events = msg_bridge.handle_message(msg).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, "converted.chat");

    let ev = EventEnvelope {
        id: EventId::new("evt-1").unwrap(),
        topic: "chat".into(),
        r#type: "t".into(),
        source: "src".into(),
        tenant: tenant_ctx(),
        subject: Some("sess".into()),
        time: Utc::now(),
        correlation_id: None,
        payload: json!({"ok": true}),
        metadata: [("k2".into(), "v2".into())].into_iter().collect(),
    };
    let messages = evt_bridge.handle_event(ev).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].attachments.len(), 1);
    let calls = client.calls.lock().unwrap().clone();
    assert!(calls.contains(&"msg:bridge".to_string()));
    assert!(calls.contains(&"evt:bridge".to_string()));
}
