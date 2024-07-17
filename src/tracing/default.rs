use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{filter, fmt, registry};

use super::*;

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub log_level: Option<String>,
    pub span_events: Option<HashSet<config::SpanEvent>>,
    pub color: Option<config::Color>,
}

impl Config {
    pub fn merge(&mut self, other: Self) {
        self.log_level = other.log_level.or(self.log_level.take());
        self.span_events = other.span_events.or(self.span_events.take());
        self.color = other.color.or(self.color.take());
    }
}

#[rustfmt::skip] // `rustfmt` formats this into unreadable single line
pub type Logger<S> = filter::Filtered<
    Option<fmt::Layer<S>>,
    filter::EnvFilter,
    S,
>;

pub fn new_logger<S>(config: &Config) -> Logger<S>
where
    S: tracing::Subscriber + for<'span> registry::LookupSpan<'span>,
{
    let layer = new_layer(config);
    let filter = new_filter(config);
    Some(layer).with_filter(filter)
}

pub fn new_layer<S>(config: &Config) -> fmt::Layer<S>
where
    S: tracing::Subscriber + for<'span> registry::LookupSpan<'span>,
{
    fmt::Layer::default()
        .with_span_events(config::SpanEvent::unwrap_or_default_config(
            &config.span_events,
        ))
        .with_ansi(config.color.unwrap_or_default().to_bool())
}

pub fn new_filter(config: &Config) -> filter::EnvFilter {
    filter(config.log_level.as_deref().unwrap_or(""))
}
