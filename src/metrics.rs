use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use opentelemetry::global;
use opentelemetry::metrics::{Counter as OtelCounter, Histogram as OtelHistogram};

/// Metrics recorded by the event bus. Includes lightweight atomics for tests and
/// OpenTelemetry handles for production.
#[derive(Clone)]
pub struct EventMetrics {
    pub events_in_total: Arc<AtomicU64>,
    pub events_out_total: Arc<AtomicU64>,
    pub events_failed_total: Arc<AtomicU64>,
    pub events_dlq_total: Arc<AtomicU64>,
    latency_hist: Option<OtelHistogram<f64>>,
    events_in_counter: Option<OtelCounter<f64>>,
    events_out_counter: Option<OtelCounter<f64>>,
    events_failed_counter: Option<OtelCounter<f64>>,
    events_dlq_counter: Option<OtelCounter<f64>>,
}

impl EventMetrics {
    pub fn new() -> Self {
        let meter = global::meter("greentic-events");
        Self {
            events_in_total: Arc::new(AtomicU64::new(0)),
            events_out_total: Arc::new(AtomicU64::new(0)),
            events_failed_total: Arc::new(AtomicU64::new(0)),
            events_dlq_total: Arc::new(AtomicU64::new(0)),
            latency_hist: Some(meter.f64_histogram("event_latency_ms").build()),
            events_in_counter: Some(meter.f64_counter("events_in_total").build()),
            events_out_counter: Some(meter.f64_counter("events_out_total").build()),
            events_failed_counter: Some(meter.f64_counter("events_failed_total").build()),
            events_dlq_counter: Some(meter.f64_counter("events_dlq_total").build()),
        }
    }

    pub fn record_in(&self) {
        self.events_in_total.fetch_add(1, Ordering::Relaxed);
        if let Some(counter) = &self.events_in_counter {
            counter.add(1.0, &[]);
        }
    }

    pub fn record_out(&self, latency: Duration) {
        self.events_out_total.fetch_add(1, Ordering::Relaxed);
        if let Some(counter) = &self.events_out_counter {
            counter.add(1.0, &[]);
        }
        if let Some(hist) = &self.latency_hist {
            hist.record(latency.as_secs_f64() * 1000.0, &[]);
        }
    }

    pub fn record_failed(&self) {
        self.events_failed_total.fetch_add(1, Ordering::Relaxed);
        if let Some(counter) = &self.events_failed_counter {
            counter.add(1.0, &[]);
        }
    }

    pub fn record_dlq(&self) {
        self.events_dlq_total.fetch_add(1, Ordering::Relaxed);
        if let Some(counter) = &self.events_dlq_counter {
            counter.add(1.0, &[]);
        }
    }
}

impl Default for EventMetrics {
    fn default() -> Self {
        Self::new()
    }
}
