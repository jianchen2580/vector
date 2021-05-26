use crate::event::{self, BTreeMap};
use chrono::TimeZone;

include!(concat!(env!("OUT_DIR"), "/event.rs"));
pub use event_wrapper::Event;
pub use metric::Value as MetricValue;

impl From<Event> for EventWrapper {
    fn from(event: Event) -> Self {
        Self { event: Some(event) }
    }
}

impl From<Chunk> for Event {
    fn from(chunk: Chunk) -> Self {
        Self::Chunk(chunk)
    }
}

impl From<Frame> for Event {
    fn from(frame: Frame) -> Self {
        Self::Frame(frame)
    }
}

impl From<Log> for Event {
    fn from(log: Log) -> Self {
        Self::Log(log)
    }
}

impl From<Metric> for Event {
    fn from(metric: Metric) -> Self {
        Self::Metric(metric)
    }
}

impl From<Log> for event::LogEvent {
    fn from(log: Log) -> Self {
        let fields = log
            .fields
            .into_iter()
            .filter_map(|(k, v)| decode_value(v).map(|value| (k, value)))
            .collect::<BTreeMap<_, _>>();

        Self::from(fields)
    }
}

impl From<Metric> for event::Metric {
    fn from(metric: Metric) -> Self {
        let kind = match metric.kind() {
            metric::Kind::Incremental => event::MetricKind::Incremental,
            metric::Kind::Absolute => event::MetricKind::Absolute,
        };

        let name = metric.name;

        let namespace = if metric.namespace.is_empty() {
            None
        } else {
            Some(metric.namespace)
        };

        let timestamp = metric
            .timestamp
            .map(|ts| chrono::Utc.timestamp(ts.seconds, ts.nanos as u32));

        let tags = if metric.tags.is_empty() {
            None
        } else {
            Some(metric.tags)
        };

        let value = match metric.value.unwrap() {
            MetricValue::Counter(counter) => event::MetricValue::Counter {
                value: counter.value,
            },
            MetricValue::Gauge(gauge) => event::MetricValue::Gauge { value: gauge.value },
            MetricValue::Set(set) => event::MetricValue::Set {
                values: set.values.into_iter().collect(),
            },
            MetricValue::Distribution1(dist) => event::MetricValue::Distribution {
                statistic: dist.statistic().into(),
                samples: event::metric::zip_samples(dist.values, dist.sample_rates),
            },
            MetricValue::Distribution2(dist) => event::MetricValue::Distribution {
                statistic: dist.statistic().into(),
                samples: dist.samples.into_iter().map(Into::into).collect(),
            },
            MetricValue::AggregatedHistogram1(hist) => event::MetricValue::AggregatedHistogram {
                buckets: event::metric::zip_buckets(hist.buckets, hist.counts),
                count: hist.count,
                sum: hist.sum,
            },
            MetricValue::AggregatedHistogram2(hist) => event::MetricValue::AggregatedHistogram {
                buckets: hist.buckets.into_iter().map(Into::into).collect(),
                count: hist.count,
                sum: hist.sum,
            },
            MetricValue::AggregatedSummary1(summary) => event::MetricValue::AggregatedSummary {
                quantiles: event::metric::zip_quantiles(summary.quantiles, summary.values),
                count: summary.count,
                sum: summary.sum,
            },
            MetricValue::AggregatedSummary2(summary) => event::MetricValue::AggregatedSummary {
                quantiles: summary.quantiles.into_iter().map(Into::into).collect(),
                count: summary.count,
                sum: summary.sum,
            },
        };

        Self::new(name, kind, value)
            .with_namespace(namespace)
            .with_tags(tags)
            .with_timestamp(timestamp)
    }
}

impl From<EventWrapper> for event::Event {
    fn from(proto: EventWrapper) -> Self {
        let event = proto.event.unwrap();

        match event {
            Event::Chunk(proto) => Self::Chunk(proto.bytes, Default::default()),
            Event::Frame(proto) => Self::Frame(proto.bytes, Default::default()),
            Event::Log(proto) => Self::Log(proto.into()),
            Event::Metric(proto) => Self::Metric(proto.into()),
        }
    }
}

impl From<Vec<u8>> for Chunk {
    fn from(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl From<Vec<u8>> for Frame {
    fn from(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl From<event::LogEvent> for Log {
    fn from(log_event: event::LogEvent) -> Self {
        let (fields, _metadata) = log_event.into_parts();
        let fields = fields
            .into_iter()
            .map(|(k, v)| (k, encode_value(v)))
            .collect::<BTreeMap<_, _>>();

        Self { fields }
    }
}

impl From<event::Metric> for Metric {
    fn from(metric: event::Metric) -> Self {
        let name = metric.series.name.name;
        let namespace = metric.series.name.namespace.unwrap_or_default();

        let timestamp = metric.data.timestamp.map(|ts| prost_types::Timestamp {
            seconds: ts.timestamp(),
            nanos: ts.timestamp_subsec_nanos() as i32,
        });

        let tags = metric.series.tags.unwrap_or_default();

        let kind = match metric.data.kind {
            event::MetricKind::Incremental => metric::Kind::Incremental,
            event::MetricKind::Absolute => metric::Kind::Absolute,
        }
        .into();

        let metric = match metric.data.value {
            event::MetricValue::Counter { value } => MetricValue::Counter(Counter { value }),
            event::MetricValue::Gauge { value } => MetricValue::Gauge(Gauge { value }),
            event::MetricValue::Set { values } => MetricValue::Set(Set {
                values: values.into_iter().collect(),
            }),
            event::MetricValue::Distribution { samples, statistic } => {
                MetricValue::Distribution2(Distribution2 {
                    samples: samples.into_iter().map(Into::into).collect(),
                    statistic: match statistic {
                        event::StatisticKind::Histogram => StatisticKind::Histogram,
                        event::StatisticKind::Summary => StatisticKind::Summary,
                    }
                    .into(),
                })
            }
            event::MetricValue::AggregatedHistogram {
                buckets,
                count,
                sum,
            } => MetricValue::AggregatedHistogram2(AggregatedHistogram2 {
                buckets: buckets.into_iter().map(Into::into).collect(),
                count,
                sum,
            }),
            event::MetricValue::AggregatedSummary {
                quantiles,
                count,
                sum,
            } => MetricValue::AggregatedSummary2(AggregatedSummary2 {
                quantiles: quantiles.into_iter().map(Into::into).collect(),
                count,
                sum,
            }),
        };

        Self {
            name,
            namespace,
            timestamp,
            tags,
            kind,
            value: Some(metric),
        }
    }
}

impl From<event::Event> for Event {
    fn from(event: event::Event) -> Self {
        match event {
            event::Event::Chunk(chunk, _) => Chunk::from(chunk).into(),
            event::Event::Frame(frame, _) => Frame::from(frame).into(),
            event::Event::Log(log_event) => Log::from(log_event).into(),
            event::Event::Metric(metric) => Metric::from(metric).into(),
        }
    }
}

impl From<event::Event> for EventWrapper {
    fn from(event: event::Event) -> Self {
        Event::from(event).into()
    }
}

fn decode_value(input: Value) -> Option<event::Value> {
    match input.kind {
        Some(value::Kind::RawBytes(data)) => Some(event::Value::Bytes(data.into())),
        Some(value::Kind::Timestamp(ts)) => Some(event::Value::Timestamp(
            chrono::Utc.timestamp(ts.seconds, ts.nanos as u32),
        )),
        Some(value::Kind::Integer(value)) => Some(event::Value::Integer(value)),
        Some(value::Kind::Float(value)) => Some(event::Value::Float(value)),
        Some(value::Kind::Boolean(value)) => Some(event::Value::Boolean(value)),
        Some(value::Kind::Map(map)) => decode_map(map.fields),
        Some(value::Kind::Array(array)) => decode_array(array.items),
        Some(value::Kind::Null(_)) => Some(event::Value::Null),
        None => {
            error!("Encoded event contains unknown value kind.");
            None
        }
    }
}

fn decode_map(fields: BTreeMap<String, Value>) -> Option<event::Value> {
    let mut accum: BTreeMap<String, event::Value> = BTreeMap::new();
    for (key, value) in fields {
        match decode_value(value) {
            Some(value) => {
                accum.insert(key, value);
            }
            None => return None,
        }
    }
    Some(event::Value::Map(accum))
}

fn decode_array(items: Vec<Value>) -> Option<event::Value> {
    let mut accum = Vec::with_capacity(items.len());
    for value in items {
        match decode_value(value) {
            Some(value) => accum.push(value),
            None => return None,
        }
    }
    Some(event::Value::Array(accum))
}

fn encode_value(value: event::Value) -> Value {
    Value {
        kind: match value {
            event::Value::Bytes(b) => Some(value::Kind::RawBytes(b.to_vec())),
            event::Value::Timestamp(ts) => Some(value::Kind::Timestamp(prost_types::Timestamp {
                seconds: ts.timestamp(),
                nanos: ts.timestamp_subsec_nanos() as i32,
            })),
            event::Value::Integer(value) => Some(value::Kind::Integer(value)),
            event::Value::Float(value) => Some(value::Kind::Float(value)),
            event::Value::Boolean(value) => Some(value::Kind::Boolean(value)),
            event::Value::Map(fields) => Some(value::Kind::Map(encode_map(fields))),
            event::Value::Array(items) => Some(value::Kind::Array(encode_array(items))),
            event::Value::Null => Some(value::Kind::Null(ValueNull::NullValue as i32)),
        },
    }
}

fn encode_map(fields: BTreeMap<String, event::Value>) -> ValueMap {
    ValueMap {
        fields: fields
            .into_iter()
            .map(|(key, value)| (key, encode_value(value)))
            .collect(),
    }
}

fn encode_array(items: Vec<event::Value>) -> ValueArray {
    ValueArray {
        items: items.into_iter().map(encode_value).collect(),
    }
}
