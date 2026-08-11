use crate::AppState;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// Stored trace span extracted from an OTLP export.
#[derive(Clone, serde::Serialize)]
pub(crate) struct StoredSpan {
    trace_id: String,
    span_id: String,
    parent_span_id: String,
    name: String,
    kind: i32,
    start_time_unix_nano: u64,
    end_time_unix_nano: u64,
    attributes: Vec<(String, String)>,
    status_code: i32,
    service_name: String,
}

/// Stored metric data point extracted from an OTLP export. `value` is the
/// point value for sums/gauges and the bucket sum for histograms; `count` is
/// 1 for sums/gauges and the observation count for histograms. The CLI
/// exports delta temporality, so totals are sums over stored points.
#[derive(Clone, serde::Serialize)]
pub(crate) struct StoredMetric {
    name: String,
    unit: String,
    value: f64,
    count: u64,
    attributes: Vec<(String, String)>,
    time_unix_nano: u64,
}
// --- OTLP collector ---
// stdlib TCP listener, no server framework. Accepts OTLP/HTTP in both
// http/protobuf (what the Go SDK exporters in the CLI and gateway send -
// Go never implemented http/json) and http/json, dispatched by Content-Type.
// The protobuf path is a hand-rolled wire-format walker over just the trace
// fields we store; no proto codegen dependency.

pub(crate) const COLLECTOR_PORT: u16 = 4318;
pub(crate) const MAX_SPANS: usize = 5000;
pub(crate) const MAX_METRICS: usize = 5000;
pub(crate) const MAX_BODY: usize = 32 * 1024 * 1024;

/// Push into a bounded queue, evicting the oldest entries.
pub(crate) fn push_bounded<T>(queue: &Mutex<VecDeque<T>>, items: Vec<T>, cap: usize) {
    if items.is_empty() {
        return;
    }
    let Ok(mut guard) = queue.lock() else {
        return;
    };
    for item in items {
        if guard.len() >= cap {
            guard.pop_front();
        }
        guard.push_back(item);
    }
}

/// Extract an i64 from a JSON value that may be a number or a string.
pub(crate) fn json_val_i64(val: &serde_json::Value) -> i64 {
    val.as_i64()
        .or_else(|| val.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

/// Extract a string value from an OTLP AnyValue.
pub(crate) fn attr_value(val: &serde_json::Value) -> String {
    if let Some(s) = val.get("stringValue").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(s) = val.get("intValue").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(n) = val.get("intValue").and_then(|v| v.as_i64()) {
        return n.to_string();
    }
    if let Some(n) = val.get("doubleValue").and_then(|v| v.as_f64()) {
        return n.to_string();
    }
    if let Some(b) = val.get("boolValue").and_then(|v| v.as_bool()) {
        return b.to_string();
    }
    String::new()
}

/// Parse OTLP ExportTraceServiceRequest JSON body, extract StoredSpans.
pub(crate) fn extract_spans(body: &str) -> Vec<StoredSpan> {
    let val: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut spans = Vec::new();
    let Some(rs_arr) = val.get("resourceSpans").and_then(|v| v.as_array()) else {
        return spans;
    };
    for rs_item in rs_arr {
        let resource_attrs = rs_item
            .get("resource")
            .and_then(|r| r.get("attributes"))
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|attr| {
                        let key = attr.get("key").and_then(|k| k.as_str())?;
                        let val = attr_value(attr.get("value")?);
                        Some((key.to_string(), val))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let service_name = resource_attrs
            .iter()
            .find(|(k, _)| k == "service.name")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let Some(ss_arr) = rs_item.get("scopeSpans").and_then(|v| v.as_array()) else {
            continue;
        };
        for ss_item in ss_arr {
            let Some(spans_arr) = ss_item.get("spans").and_then(|v| v.as_array()) else {
                continue;
            };
            for span_val in spans_arr {
                let trace_id = span_val
                    .get("traceId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let span_id = span_val
                    .get("spanId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let parent_span_id = span_val
                    .get("parentSpanId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = span_val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let kind = span_val.get("kind").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let start_time = span_val
                    .get("startTimeUnixNano")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let end_time = span_val
                    .get("endTimeUnixNano")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let status_code = span_val
                    .get("status")
                    .and_then(|s| s.get("code"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let attributes = span_val
                    .get("attributes")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|attr| {
                                let key = attr.get("key").and_then(|k| k.as_str())?;
                                let val = attr_value(attr.get("value")?);
                                Some((key.to_string(), val))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                spans.push(StoredSpan {
                    trace_id,
                    span_id,
                    parent_span_id,
                    name,
                    kind,
                    start_time_unix_nano: start_time,
                    end_time_unix_nano: end_time,
                    attributes,
                    status_code,
                    service_name: service_name.clone(),
                });
            }
        }
    }
    spans
}

// --- OTLP protobuf decoding ---
// Hand-rolled reader for the protobuf wire format: a message is a flat
// sequence of (field number, wire type, payload) records, and nested messages
// are just length-delimited payloads decoded the same way. `pb_fields` turns
// one message into that record list; the decoders below then read like the
// .proto definitions they mirror. Unknown fields are skipped; malformed input
// yields whatever parsed before the error. Field numbers are from
// opentelemetry-proto v1 (trace.proto, metrics.proto, common.proto) and named
// as MESSAGE_FIELD below.

pub(crate) const WIRE_VARINT: u64 = 0;
pub(crate) const WIRE_FIXED64: u64 = 1;
pub(crate) const WIRE_LEN_DELIMITED: u64 = 2;
pub(crate) const WIRE_FIXED32: u64 = 5;

// trace.proto
pub(crate) const EXPORT_TRACE_REQUEST_RESOURCE_SPANS: u32 = 1;
pub(crate) const RESOURCE_SPANS_RESOURCE: u32 = 1;
pub(crate) const RESOURCE_SPANS_SCOPE_SPANS: u32 = 2;
pub(crate) const SCOPE_SPANS_SPANS: u32 = 2;
pub(crate) const SPAN_TRACE_ID: u32 = 1;
pub(crate) const SPAN_SPAN_ID: u32 = 2;
pub(crate) const SPAN_PARENT_SPAN_ID: u32 = 4;
pub(crate) const SPAN_NAME: u32 = 5;
pub(crate) const SPAN_KIND: u32 = 6;
pub(crate) const SPAN_START_TIME_UNIX_NANO: u32 = 7;
pub(crate) const SPAN_END_TIME_UNIX_NANO: u32 = 8;
pub(crate) const SPAN_ATTRIBUTES: u32 = 9;
pub(crate) const SPAN_STATUS: u32 = 15;
pub(crate) const STATUS_CODE: u32 = 3;

// common.proto / resource.proto
pub(crate) const RESOURCE_ATTRIBUTES: u32 = 1;
pub(crate) const KEY_VALUE_KEY: u32 = 1;
pub(crate) const KEY_VALUE_VALUE: u32 = 2;
pub(crate) const ANY_VALUE_STRING: u32 = 1;
pub(crate) const ANY_VALUE_BOOL: u32 = 2;
pub(crate) const ANY_VALUE_INT: u32 = 3;
pub(crate) const ANY_VALUE_DOUBLE: u32 = 4;

// metrics.proto
pub(crate) const EXPORT_METRICS_REQUEST_RESOURCE_METRICS: u32 = 1;
pub(crate) const RESOURCE_METRICS_SCOPE_METRICS: u32 = 2;
pub(crate) const SCOPE_METRICS_METRICS: u32 = 2;
pub(crate) const METRIC_NAME: u32 = 1;
pub(crate) const METRIC_UNIT: u32 = 3;
pub(crate) const METRIC_GAUGE: u32 = 5;
pub(crate) const METRIC_SUM: u32 = 7;
pub(crate) const METRIC_HISTOGRAM: u32 = 9;
pub(crate) const DATA_POINTS: u32 = 1;
pub(crate) const NUMBER_POINT_TIME_UNIX_NANO: u32 = 3;
pub(crate) const NUMBER_POINT_AS_DOUBLE: u32 = 4;
pub(crate) const NUMBER_POINT_AS_INT: u32 = 6;
pub(crate) const NUMBER_POINT_ATTRIBUTES: u32 = 7;
pub(crate) const HISTOGRAM_POINT_TIME_UNIX_NANO: u32 = 3;
pub(crate) const HISTOGRAM_POINT_COUNT: u32 = 4;
pub(crate) const HISTOGRAM_POINT_SUM: u32 = 5;
pub(crate) const HISTOGRAM_POINT_ATTRIBUTES: u32 = 9;

/// One decoded protobuf field payload.
pub(crate) enum PbValue<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
}

/// Read one varint: 7 payload bits per byte, least-significant group first;
/// a set high bit means another byte follows.
pub(crate) fn pb_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        val |= u64::from(byte & 0x7f) << shift;
        let has_more_bytes = byte & 0x80 != 0;
        if !has_more_bytes {
            return Some(val);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

/// Decode one message into its list of (field number, payload) records.
/// Each record starts with a tag varint packing `field_number << 3 | wire_type`.
pub(crate) fn pb_fields(buf: &[u8]) -> Vec<(u32, PbValue<'_>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let Some(tag) = pb_varint(buf, &mut pos) else {
            break;
        };
        let field_number = (tag >> 3) as u32;
        let wire_type = tag & 7;
        match wire_type {
            WIRE_VARINT => match pb_varint(buf, &mut pos) {
                Some(v) => out.push((field_number, PbValue::Varint(v))),
                None => break,
            },
            WIRE_FIXED64 => {
                let Some(bytes) = buf.get(pos..pos + 8) else {
                    break;
                };
                let v = u64::from_le_bytes(bytes.try_into().expect("slice is 8 bytes"));
                out.push((field_number, PbValue::Fixed64(v)));
                pos += 8;
            }
            WIRE_LEN_DELIMITED => {
                let Some(len) = pb_varint(buf, &mut pos) else {
                    break;
                };
                let end = pos + len as usize;
                let Some(bytes) = buf.get(pos..end) else {
                    break;
                };
                out.push((field_number, PbValue::Bytes(bytes)));
                pos = end;
            }
            WIRE_FIXED32 => {
                if buf.len() < pos + 4 {
                    break;
                }
                pos += 4;
            }
            _ => break,
        }
    }
    out
}

pub(crate) fn pb_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Render an OTLP AnyValue message to a string, mirroring `attr_value`.
pub(crate) fn pb_any_value(buf: &[u8]) -> String {
    for (field, val) in pb_fields(buf) {
        match (field, val) {
            (ANY_VALUE_STRING, PbValue::Bytes(b)) => {
                return String::from_utf8_lossy(b).into_owned();
            }
            (ANY_VALUE_BOOL, PbValue::Varint(v)) => return (v != 0).to_string(),
            (ANY_VALUE_INT, PbValue::Varint(v)) => return (v as i64).to_string(),
            (ANY_VALUE_DOUBLE, PbValue::Fixed64(v)) => return f64::from_bits(v).to_string(),
            _ => {}
        }
    }
    String::new()
}

/// Parse a KeyValue message into a (key, value) pair.
pub(crate) fn pb_key_value(buf: &[u8]) -> Option<(String, String)> {
    let mut key = None;
    let mut value = String::new();
    for (field, val) in pb_fields(buf) {
        match (field, val) {
            (KEY_VALUE_KEY, PbValue::Bytes(b)) => {
                key = Some(String::from_utf8_lossy(b).into_owned())
            }
            (KEY_VALUE_VALUE, PbValue::Bytes(b)) => value = pb_any_value(b),
            _ => {}
        }
    }
    key.map(|k| (k, value))
}

pub(crate) fn pb_span(buf: &[u8], service_name: &str) -> StoredSpan {
    let mut span = StoredSpan {
        trace_id: String::new(),
        span_id: String::new(),
        parent_span_id: String::new(),
        name: String::new(),
        kind: 0,
        start_time_unix_nano: 0,
        end_time_unix_nano: 0,
        attributes: Vec::new(),
        status_code: 0,
        service_name: service_name.to_string(),
    };
    for (field, val) in pb_fields(buf) {
        match (field, val) {
            (SPAN_TRACE_ID, PbValue::Bytes(b)) => span.trace_id = pb_hex(b),
            (SPAN_SPAN_ID, PbValue::Bytes(b)) => span.span_id = pb_hex(b),
            (SPAN_PARENT_SPAN_ID, PbValue::Bytes(b)) => span.parent_span_id = pb_hex(b),
            (SPAN_NAME, PbValue::Bytes(b)) => span.name = String::from_utf8_lossy(b).into_owned(),
            (SPAN_KIND, PbValue::Varint(v)) => span.kind = v as i32,
            (SPAN_START_TIME_UNIX_NANO, PbValue::Fixed64(v)) => span.start_time_unix_nano = v,
            (SPAN_END_TIME_UNIX_NANO, PbValue::Fixed64(v)) => span.end_time_unix_nano = v,
            (SPAN_ATTRIBUTES, PbValue::Bytes(b)) => span.attributes.extend(pb_key_value(b)),
            (SPAN_STATUS, PbValue::Bytes(status)) => {
                for (field, val) in pb_fields(status) {
                    if let (STATUS_CODE, PbValue::Varint(code)) = (field, val) {
                        span.status_code = code as i32;
                    }
                }
            }
            _ => {}
        }
    }
    span
}

/// Parse a protobuf ExportTraceServiceRequest body, extract StoredSpans.
pub(crate) fn extract_spans_proto(body: &[u8]) -> Vec<StoredSpan> {
    let mut spans = Vec::new();
    for (field, val) in pb_fields(body) {
        let (EXPORT_TRACE_REQUEST_RESOURCE_SPANS, PbValue::Bytes(resource_spans)) = (field, val)
        else {
            continue;
        };
        let mut service_name = String::new();
        let fields = pb_fields(resource_spans);
        for (field, val) in &fields {
            if let (RESOURCE_SPANS_RESOURCE, PbValue::Bytes(resource)) = (*field, val) {
                for (field, val) in pb_fields(resource) {
                    if let (RESOURCE_ATTRIBUTES, PbValue::Bytes(kv)) = (field, val)
                        && let Some((k, v)) = pb_key_value(kv)
                        && k == "service.name"
                    {
                        service_name = v;
                    }
                }
            }
        }
        for (field, val) in &fields {
            if let (RESOURCE_SPANS_SCOPE_SPANS, PbValue::Bytes(scope_spans)) = (*field, val) {
                for (field, val) in pb_fields(scope_spans) {
                    if let (SCOPE_SPANS_SPANS, PbValue::Bytes(span)) = (field, val) {
                        spans.push(pb_span(span, &service_name));
                    }
                }
            }
        }
    }
    spans
}

/// Which OTLP data-point message a metric carries. Number points come from
/// gauges and sums; histogram points share field numbers for time but not
/// for value, count, or attributes.
#[derive(Clone, Copy)]
pub(crate) enum PointKind {
    Number,
    Histogram,
}

/// Parse a NumberDataPoint or HistogramDataPoint into a StoredMetric.
pub(crate) fn pb_data_point(buf: &[u8], kind: PointKind, name: &str, unit: &str) -> StoredMetric {
    let mut point = StoredMetric {
        name: name.to_string(),
        unit: unit.to_string(),
        value: 0.0,
        count: 1,
        attributes: Vec::new(),
        time_unix_nano: 0,
    };
    for (field, val) in pb_fields(buf) {
        match (kind, field, val) {
            (PointKind::Number, NUMBER_POINT_TIME_UNIX_NANO, PbValue::Fixed64(v))
            | (PointKind::Histogram, HISTOGRAM_POINT_TIME_UNIX_NANO, PbValue::Fixed64(v)) => {
                point.time_unix_nano = v
            }
            (PointKind::Number, NUMBER_POINT_AS_DOUBLE, PbValue::Fixed64(v)) => {
                point.value = f64::from_bits(v)
            }
            (PointKind::Number, NUMBER_POINT_AS_INT, PbValue::Fixed64(v)) => {
                point.value = v as i64 as f64
            }
            (PointKind::Number, NUMBER_POINT_ATTRIBUTES, PbValue::Bytes(b)) => {
                point.attributes.extend(pb_key_value(b))
            }
            (PointKind::Histogram, HISTOGRAM_POINT_COUNT, PbValue::Fixed64(v)) => point.count = v,
            (PointKind::Histogram, HISTOGRAM_POINT_SUM, PbValue::Fixed64(v)) => {
                point.value = f64::from_bits(v)
            }
            (PointKind::Histogram, HISTOGRAM_POINT_ATTRIBUTES, PbValue::Bytes(b)) => {
                point.attributes.extend(pb_key_value(b))
            }
            _ => {}
        }
    }
    point
}

/// Parse one Metric message. Exponential histograms and summaries are skipped.
pub(crate) fn pb_metric(metric: &[u8]) -> Vec<StoredMetric> {
    let fields = pb_fields(metric);
    let mut name = String::new();
    let mut unit = String::new();
    for (field, val) in &fields {
        match (*field, val) {
            (METRIC_NAME, PbValue::Bytes(b)) => name = String::from_utf8_lossy(b).into_owned(),
            (METRIC_UNIT, PbValue::Bytes(b)) => unit = String::from_utf8_lossy(b).into_owned(),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (field, val) in &fields {
        let kind = match *field {
            METRIC_GAUGE | METRIC_SUM => PointKind::Number,
            METRIC_HISTOGRAM => PointKind::Histogram,
            _ => continue,
        };
        let PbValue::Bytes(data) = val else {
            continue;
        };
        for (field, val) in pb_fields(data) {
            let (DATA_POINTS, PbValue::Bytes(point)) = (field, val) else {
                continue;
            };
            out.push(pb_data_point(point, kind, &name, &unit));
        }
    }
    out
}

/// Parse a protobuf ExportMetricsServiceRequest body into StoredMetrics.
pub(crate) fn extract_metrics_proto(body: &[u8]) -> Vec<StoredMetric> {
    let mut out = Vec::new();
    for (field, val) in pb_fields(body) {
        let (EXPORT_METRICS_REQUEST_RESOURCE_METRICS, PbValue::Bytes(resource_metrics)) =
            (field, val)
        else {
            continue;
        };
        for (field, val) in pb_fields(resource_metrics) {
            let (RESOURCE_METRICS_SCOPE_METRICS, PbValue::Bytes(scope_metrics)) = (field, val)
            else {
                continue;
            };
            for (field, val) in pb_fields(scope_metrics) {
                let (SCOPE_METRICS_METRICS, PbValue::Bytes(metric)) = (field, val) else {
                    continue;
                };
                out.extend(pb_metric(metric));
            }
        }
    }
    out
}

/// Handle a single OTLP HTTP request on the collector's TCP socket. The
/// response carries `Connection: close`; Go's http client replays the export
/// on a fresh connection, so one-request-per-connection is safe.
pub(crate) fn handle_otlp_request(
    stream: &mut std::net::TcpStream,
    spans_store: &Mutex<VecDeque<StoredSpan>>,
    metrics_store: &Mutex<VecDeque<StoredMetric>>,
) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut data: Vec<u8> = Vec::with_capacity(8192);
    let mut buf = [0u8; 65536];
    let body_start = loop {
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                data.extend_from_slice(&buf[..n]);
                if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
                if data.len() > 64 * 1024 {
                    return;
                }
            }
            Err(_) => return,
        }
    };
    let header = String::from_utf8_lossy(&data[..body_start]).to_string();
    if !header.starts_with("POST") {
        return;
    }
    let header_line = |name: &str| -> Option<String> {
        header
            .lines()
            .find(|l| l.to_lowercase().starts_with(name))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_lowercase()))
    };
    let content_len = header_line("content-length:")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_len > MAX_BODY {
        return;
    }
    while data.len() < body_start + content_len {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => return,
        }
    }
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}";
    let _ = stream.write_all(response);
    let _ = stream.flush();
    // Traces and metrics exports share the outer protobuf shape - Metric.name
    // sits at Span.trace_id's field number - so the URL path decides the
    // parser; any other path is acked but never parsed.
    let path = header.split_whitespace().nth(1).unwrap_or("");
    let body = &data[body_start..];
    if path.starts_with("/v1/traces") {
        let spans = if header_line("content-type:").is_some_and(|ct| ct.contains("json")) {
            extract_spans(&String::from_utf8_lossy(body))
        } else {
            extract_spans_proto(body)
        };
        push_bounded(spans_store, spans, MAX_SPANS);
    } else if path.starts_with("/v1/metrics") {
        push_bounded(metrics_store, extract_metrics_proto(body), MAX_METRICS);
    }
}

/// Spawn the OTLP collector thread.
pub(crate) fn start_collector(
    spans_store: Arc<Mutex<VecDeque<StoredSpan>>>,
    metrics_store: Arc<Mutex<VecDeque<StoredMetric>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let addr = format!("127.0.0.1:{}", COLLECTOR_PORT);
        let listener = match std::net::TcpListener::bind(&addr) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("desktop: OTLP collector failed to bind {}: {}", addr, e);
                return;
            }
        };
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => handle_otlp_request(&mut stream, &spans_store, &metrics_store),
                Err(e) => {
                    eprintln!("desktop: OTLP collector accept error: {}", e);
                    break;
                }
            }
        }
    })
}

/// Return all stored traces, newest first.
#[tauri::command]
pub(crate) fn get_traces(state: tauri::State<'_, AppState>) -> Result<Vec<StoredSpan>, String> {
    let guard = state.stored_traces.lock().map_err(|e| e.to_string())?;
    let mut spans: Vec<StoredSpan> = guard.iter().cloned().collect();
    spans.reverse();
    Ok(spans)
}

/// Return all stored metric data points in arrival order.
#[tauri::command]
pub(crate) fn get_metrics(state: tauri::State<'_, AppState>) -> Result<Vec<StoredMetric>, String> {
    let guard = state.stored_metrics.lock().map_err(|e| e.to_string())?;
    Ok(guard.iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb_len(field: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![(field << 3) as u8 | 2];
        let mut n = payload.len();
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn extract_spans_proto_parses_trace_export() {
        let any_str = pb_len(1, b"infer-cli");
        let mut kv = pb_len(1, b"service.name");
        kv.extend(pb_len(2, &any_str));
        let resource_msg = pb_len(1, &kv);
        let resource = pb_len(1, &resource_msg);

        let mut span = pb_len(1, &[0xab, 0xcd]);
        span.extend(pb_len(2, &[0x01]));
        span.extend(pb_len(5, b"chat"));
        span.extend([6 << 3, 2]);
        span.extend([(7 << 3) | 1]);
        span.extend(5u64.to_le_bytes());
        span.extend([(8 << 3) | 1]);
        span.extend(9u64.to_le_bytes());
        let any_int = [3 << 3, 42];
        let mut attr = pb_len(1, b"tokens");
        attr.extend(pb_len(2, &any_int));
        span.extend(pb_len(9, &attr));
        let status = [3 << 3, 2];
        span.extend(pb_len(15, &status));

        let scope_spans = pb_len(2, &span);
        let mut resource_spans = resource;
        resource_spans.extend(pb_len(2, &scope_spans));
        let body = pb_len(1, &resource_spans);

        let spans = extract_spans_proto(&body);
        assert_eq!(spans.len(), 1);
        let s = &spans[0];
        assert_eq!(s.trace_id, "abcd");
        assert_eq!(s.span_id, "01");
        assert_eq!(s.name, "chat");
        assert_eq!(s.kind, 2);
        assert_eq!(s.start_time_unix_nano, 5);
        assert_eq!(s.end_time_unix_nano, 9);
        assert_eq!(s.attributes, vec![("tokens".to_string(), "42".to_string())]);
        assert_eq!(s.status_code, 2);
        assert_eq!(s.service_name, "infer-cli");
    }

    #[test]
    fn extract_spans_proto_tolerates_garbage() {
        assert!(extract_spans_proto(&[0xff, 0xff, 0xff]).is_empty());
        assert!(extract_spans_proto(&[]).is_empty());
    }

    #[test]
    fn extract_metrics_proto_parses_sum_and_histogram() {
        let mut sum_point = vec![(3 << 3) | 1];
        sum_point.extend(7u64.to_le_bytes());
        sum_point.extend([(6 << 3) | 1]);
        sum_point.extend(42i64.to_le_bytes());
        let mut attr = pb_len(1, b"gen_ai.token.type");
        attr.extend(pb_len(2, &pb_len(1, b"input")));
        sum_point.extend(pb_len(7, &attr));
        let sum = pb_len(1, &sum_point);
        let mut counter = pb_len(1, b"infer.agent.tool.calls");
        counter.extend(pb_len(3, b"1"));
        counter.extend(pb_len(7, &sum));

        let mut histo_point = vec![(4 << 3) | 1];
        histo_point.extend(3u64.to_le_bytes());
        histo_point.extend([(5 << 3) | 1]);
        histo_point.extend(1500f64.to_bits().to_le_bytes());
        let histo = pb_len(1, &histo_point);
        let mut usage = pb_len(1, b"gen_ai.client.token.usage");
        usage.extend(pb_len(9, &histo));

        let mut scope_metrics = pb_len(2, &counter);
        scope_metrics.extend(pb_len(2, &usage));
        let resource_metrics = pb_len(2, &scope_metrics);
        let body = pb_len(1, &resource_metrics);

        let metrics = extract_metrics_proto(&body);
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].name, "infer.agent.tool.calls");
        assert_eq!(metrics[0].unit, "1");
        assert_eq!(metrics[0].value, 42.0);
        assert_eq!(metrics[0].count, 1);
        assert_eq!(metrics[0].time_unix_nano, 7);
        assert_eq!(
            metrics[0].attributes,
            vec![("gen_ai.token.type".to_string(), "input".to_string())]
        );
        assert_eq!(metrics[1].name, "gen_ai.client.token.usage");
        assert_eq!(metrics[1].value, 1500.0);
        assert_eq!(metrics[1].count, 3);
    }
}
