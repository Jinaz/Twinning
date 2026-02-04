use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum JsonPayload {
    Obj(serde_json::Value),
    Arr(Vec<serde_json::Value>),
}

#[derive(Debug, Clone)]
pub struct ParsedSample {
    pub logical_topic: String,
    pub data_raw: String,
    pub ts: DateTime<Utc>,
}

pub fn parse_payload(text: &str, mqtt_topic: &str) -> Vec<ParsedSample> {
    let t = text.trim();
    if t.is_empty() {
        return vec![];
    }

    // Try JSON first
    if t.starts_with('{') || t.starts_with('[') || (t.starts_with('"') && t.ends_with('"')) {
        if let Ok(val) = serde_json::from_str::<JsonPayload>(t) {
            return parse_json_payload(val, mqtt_topic);
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            return parse_json_payload(JsonPayload::Obj(v), mqtt_topic);
        }
    }

    // Custom tuple-ish format
    parse_custom_tuple_list(t, mqtt_topic)
}

fn parse_json_payload(p: JsonPayload, mqtt_topic: &str) -> Vec<ParsedSample> {
    fn obj_to_sample(obj: &serde_json::Value, mqtt_topic: &str) -> Option<ParsedSample> {
        let logical_topic = obj
            .get("topic")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| mqtt_topic.to_string());

        let data_raw = match obj.get("data").or_else(|| obj.get("value")) {
            Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
            Some(v) if v.is_number() => v.to_string(),
            Some(v) => v.to_string(),
            None => return None,
        };

        let ts = obj
            .get("datetime")
            .or_else(|| obj.get("ts"))
            .and_then(|v| v.as_str())
            .map(parse_datetime)
            .unwrap_or_else(Utc::now);

        Some(ParsedSample {
            logical_topic,
            data_raw,
            ts,
        })
    }

    match p {
        JsonPayload::Obj(v) => {
            if let Some(s) = v.as_str() {
                return vec![ParsedSample {
                    logical_topic: mqtt_topic.to_string(),
                    data_raw: s.to_string(),
                    ts: Utc::now(),
                }];
            }
            if v.is_object() {
                return obj_to_sample(&v, mqtt_topic).into_iter().collect();
            }
            vec![]
        }
        JsonPayload::Arr(arr) => arr.iter().filter_map(|o| obj_to_sample(o, mqtt_topic)).collect(),
    }
}

/// Accepts:
/// { ( topic: förderband1-4, data: 1.2V, datetime: now), ( topic: x, data: 2.0V, datetime: 2026-01-11T10:00:00Z ) }
fn parse_custom_tuple_list(t: &str, mqtt_topic: &str) -> Vec<ParsedSample> {
    // Strip outer { } if present
    let mut s = t.trim().to_string();
    if s.starts_with('{') && s.ends_with('}') {
        s = s[1..s.len() - 1].trim().to_string();
    }

    // Split by "),"
    let items: Vec<String> = s
        .split("),")
        .map(|x| x.trim().trim_end_matches(')').trim().to_string())
        .filter(|x| !x.is_empty())
        .collect();

    let mut out = vec![];
    for it in items {
        let mut x = it.trim().to_string();
        x = x.trim_start_matches('(').trim_end_matches(')').trim().to_string();
        if x.is_empty() {
            continue;
        }

        let mut topic: Option<String> = None;
        let mut data: Option<String> = None;
        let mut datetime: Option<String> = None;

        for part in x.split(',') {
            let p = part.trim();
            let mut kv = p.splitn(2, ':');
            let k = kv.next().unwrap_or("").trim().to_lowercase();
            let v = kv.next().unwrap_or("").trim().trim_matches('"').to_string();

            if k == "topic" {
                topic = Some(v);
            } else if k == "data" || k == "value" {
                data = Some(v);
            } else if k == "datetime" || k == "ts" || k == "time" {
                datetime = Some(v);
            }
        }

        let logical_topic = topic.unwrap_or_else(|| mqtt_topic.to_string());
        let data_raw = match data {
            Some(d) if !d.is_empty() => d,
            _ => continue,
        };
        let ts = datetime.as_deref().map(parse_datetime).unwrap_or_else(Utc::now);

        out.push(ParsedSample {
            logical_topic,
            data_raw,
            ts,
        });
    }

    out
}

pub fn parse_datetime(s: &str) -> DateTime<Utc> {
    let t = s.trim().trim_matches('"');
    if t.eq_ignore_ascii_case("now") || t.is_empty() {
        return Utc::now();
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(sec) = t.parse::<i64>() {
        if let Some(dt) = DateTime::<Utc>::from_timestamp(sec, 0) {
            return dt;
        }
    }
    Utc::now()
}

pub fn parse_value_and_unit(data_raw: &str) -> (Option<f64>, Option<String>) {
    // Examples: "1.2V", "450 rpm", "-10.0°C", "23.5"
    let re = Regex::new(r"^\s*([+-]?\d+(?:[.,]\d+)?)\s*([^\d\s].*)?\s*$").unwrap();
    let s = data_raw.trim();
    if let Some(caps) = re.captures(s) {
        let num = caps
            .get(1)
            .and_then(|m| m.as_str().replace(',', ".").parse::<f64>().ok());
        let unit = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .filter(|u| !u.is_empty());
        return (num, unit);
    }
    (None, None)
}
