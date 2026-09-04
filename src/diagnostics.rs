//! Quiet-by-default, redacted diagnostics and bounded metrics.

use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_DIAGNOSTIC_EVENTS: usize = 1024;
pub const MAX_DIAGNOSTIC_VALUE_BYTES: usize = 4096;
pub const MAX_METRIC_KEYS: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticEvent {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub fields: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub counters: BTreeMap<String, u64>,
    pub dropped_events: u64,
    pub dropped_metric_keys: u64,
}

#[derive(Debug, Default)]
struct DiagnosticsState {
    events: VecDeque<DiagnosticEvent>,
    metrics: MetricsSnapshot,
}

#[derive(Debug, Clone)]
pub struct Diagnostics {
    enabled: bool,
    state: Arc<Mutex<DiagnosticsState>>,
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::disabled()
    }
}

impl Diagnostics {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            state: Arc::new(Mutex::new(DiagnosticsState::default())),
        }
    }

    pub fn enabled() -> Self {
        Self {
            enabled: true,
            state: Arc::new(Mutex::new(DiagnosticsState::default())),
        }
    }

    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn record(&self, mut event: DiagnosticEvent, known_secrets: &[&str]) {
        if !self.enabled {
            return;
        }
        event.fields = crate::security::redact_json(&event.fields, known_secrets);
        bound_json_strings(&mut event.fields);
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.events.len() == MAX_DIAGNOSTIC_EVENTS {
            state.events.pop_front();
            state.metrics.dropped_events = state.metrics.dropped_events.saturating_add(1);
        }
        state.events.push_back(event);
    }

    pub fn increment(&self, name: &str) {
        if !self.enabled || !valid_metric_name(name) {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if !state.metrics.counters.contains_key(name)
            && state.metrics.counters.len() >= MAX_METRIC_KEYS
        {
            state.metrics.dropped_metric_keys = state.metrics.dropped_metric_keys.saturating_add(1);
            return;
        }
        let value = state.metrics.counters.entry(name.to_owned()).or_default();
        *value = value.saturating_add(1);
    }

    pub fn events(&self) -> Vec<DiagnosticEvent> {
        self.state
            .lock()
            .map(|state| state.events.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn metrics(&self) -> MetricsSnapshot {
        self.state
            .lock()
            .map(|state| state.metrics.clone())
            .unwrap_or_default()
    }

    pub fn write_jsonl(&self, mut output: impl Write) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        for event in self.events() {
            serde_json::to_writer(&mut output, &event).map_err(std::io::Error::other)?;
            output.write_all(b"\n")?;
        }
        Ok(())
    }
}

fn bound_json_strings(value: &mut Value) {
    match value {
        Value::String(text) => {
            if text.len() > MAX_DIAGNOSTIC_VALUE_BYTES {
                let mut end = MAX_DIAGNOSTIC_VALUE_BYTES;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
                text.push_str("<truncated>");
            }
        }
        Value::Array(values) => values.iter_mut().for_each(bound_json_strings),
        Value::Object(object) => object.values_mut().for_each(bound_json_strings),
        _ => {}
    }
}

fn valid_metric_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
}
