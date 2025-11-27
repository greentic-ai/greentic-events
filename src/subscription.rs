use std::pin::Pin;

use futures::stream::Stream;
use tokio::sync::mpsc;

use greentic_types::EventEnvelope;

/// Acknowledgement mode for subscriptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckMode {
    Manual,
    Auto,
}

/// Options controlling subscription behaviour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionOptions {
    pub durable: bool,
    pub deliver_existing: bool,
    pub ack_mode: AckMode,
}

impl Default for SubscriptionOptions {
    fn default() -> Self {
        Self {
            durable: true,
            deliver_existing: true,
            ack_mode: AckMode::Auto,
        }
    }
}

/// Handle returned to subscribers for receiving events.
#[derive(Debug)]
pub struct SubscriptionHandle {
    receiver: mpsc::Receiver<EventEnvelope>,
}

impl SubscriptionHandle {
    pub(crate) fn new(receiver: mpsc::Receiver<EventEnvelope>) -> Self {
        Self { receiver }
    }

    /// Receives the next event from the subscription.
    pub async fn next(&mut self) -> Option<EventEnvelope> {
        self.receiver.recv().await
    }

    /// Converts the subscription into a `Stream` for ergonomic use in async flows.
    pub fn into_stream(self) -> impl Stream<Item = EventEnvelope> + Send + 'static {
        tokio_stream::wrappers::ReceiverStream::new(self.receiver)
    }
}

impl Stream for SubscriptionHandle {
    type Item = EventEnvelope;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_recv(cx)
    }
}

pub fn subscription_channel() -> (mpsc::Sender<EventEnvelope>, SubscriptionHandle) {
    let (tx, rx) = mpsc::channel(64);
    (tx, SubscriptionHandle::new(rx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use greentic_types::{EventEnvelope, EventId, TenantCtx};
    use serde_json::json;

    #[tokio::test]
    async fn subscription_stream_works() {
        let (tx, mut handle) = subscription_channel();
        let tenant = TenantCtx::new("dev".try_into().unwrap(), "tenant-1".try_into().unwrap());
        let event = EventEnvelope {
            id: EventId::new("evt-1").unwrap(),
            topic: "foo".into(),
            r#type: "t".into(),
            source: "s".into(),
            tenant,
            subject: None,
            time: Utc::now(),
            correlation_id: None,
            payload: json!({"ok": true}),
            metadata: Default::default(),
        };
        tx.send(event.clone()).await.unwrap();

        let received = handle.next().await.unwrap();
        assert_eq!(received.id, event.id);
    }
}
