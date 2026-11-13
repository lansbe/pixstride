//! OTLP JSON payloads with bounded, non-identifying attributes.
use serde_json::{json, Value};
#[derive(Clone)]
pub(super) struct Parent {
    pub trace: String,
    pub span: String,
    pub sampled: bool,
}
pub(super) struct Event {
    pub start: u128,
    pub duration: u64,
    pub status: u16,
    pub parent: Option<Parent>,
}
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Traces,
    Metrics,
    Logs,
}
fn attributes(status: u16) -> Value {
    json!([{"key":"http.response.status_code","value":{"intValue":status.to_string()}}])
}
fn resource(service: &str) -> Value {
    json!({"attributes":[{"key":"service.name","value":{"stringValue":service}},
        {"key":"service.version","value":{"stringValue":env!("CARGO_PKG_VERSION")}}]})
}
pub(super) fn build(
    kind: Kind,
    event: &Event,
    seed: u128,
    sequence: u64,
    service: &str,
) -> Option<Value> {
    let time = (event.start + u128::from(event.duration)).to_string();
    Some(match kind {
        Kind::Traces => {
            if event.parent.as_ref().is_some_and(|p| !p.sampled) {
                return None;
            }
            let trace = event.parent.as_ref().map_or_else(
                || format!("{:032x}", seed.wrapping_add(u128::from(sequence))),
                |p| p.trace.clone(),
            );
            let mut span = json!({"traceId":trace,"spanId":format!("{:016x}",(seed as u64).wrapping_add(sequence)),
                "name":"image.request","kind":2,"startTimeUnixNano":event.start.to_string(),"endTimeUnixNano":time,
                "attributes":attributes(event.status),"status":{"code":if event.status>=500 {2} else {1}}});
            if let Some(parent) = &event.parent {
                span["parentSpanId"] = json!(parent.span);
            }
            json!({"resourceSpans":[{"resource":resource(service),"scopeSpans":[{"scope":{"name":"pixstride.http"},"spans":[span]}]}]})
        }
        Kind::Metrics => {
            json!({"resourceMetrics":[{"resource":resource(service),"scopeMetrics":[{"scope":{"name":"pixstride.http"},"metrics":[
                {"name":"http.server.request.duration","unit":"s","histogram":{"aggregationTemporality":1,"dataPoints":[{
                    "startTimeUnixNano":event.start.to_string(),"timeUnixNano":time,"attributes":attributes(event.status),
                    "count":"1","sum":event.duration as f64/1e9,"explicitBounds":[0.001,0.01,0.1,1.0],
                    "bucketCounts":([event.duration<=1_000_000,event.duration>1_000_000&&event.duration<=10_000_000,
                        event.duration>10_000_000&&event.duration<=100_000_000,event.duration>100_000_000&&event.duration<=1_000_000_000,
                        event.duration>1_000_000_000].map(|v|u8::from(v).to_string()))}]}}
            ]}]}]})
        }
        Kind::Logs => {
            if event.status < 400 {
                return None;
            }
            json!({"resourceLogs":[{"resource":resource(service),"scopeLogs":[{"scope":{"name":"pixstride.http"},"logRecords":[{
                "timeUnixNano":time,"severityNumber":if event.status>=500 {17} else {13},
                "severityText":if event.status>=500 {"ERROR"} else {"WARN"},"body":{"stringValue":"HTTP image request failed"},
                "attributes":attributes(event.status)}]}]}]})
        }
    })
}
pub(super) fn parent(value: &str) -> Option<Parent> {
    let fields = value.split('-').collect::<Vec<_>>();
    if fields.len() != 4
        || fields[0] != "00"
        || fields[1].len() != 32
        || fields[2].len() != 16
        || fields[3].len() != 2
        || fields[1..].iter().any(|s| {
            !s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        || fields[1].bytes().all(|b| b == b'0')
        || fields[2].bytes().all(|b| b == b'0')
    {
        return None;
    }
    Some(Parent {
        trace: fields[1].into(),
        span: fields[2].into(),
        sampled: u8::from_str_radix(fields[3], 16).ok()? & 1 != 0,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_sampling_and_metrics_wire_types_are_valid() {
        assert!(parent("00-00000000000000000000000000000000-0123456789abcdef-01").is_none());
        let mut event = Event {
            start: 100,
            duration: 100_000_000,
            status: 502,
            parent: parent("00-0123456789abcdef0123456789abcdef-0123456789abcdef-00"),
        };
        assert!(build(Kind::Traces, &event, 10, 1, "test").is_none());
        assert!(build(Kind::Metrics, &event, 10, 1, "test").is_some());
        event.parent.as_mut().unwrap().sampled = true;
        let payload = build(Kind::Traces, &event, 10, 1, "test").unwrap();
        assert_eq!(
            payload["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["parentSpanId"],
            "0123456789abcdef"
        );
    }
}
