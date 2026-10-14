//! The bodies the control routes accept, checked before anything is applied.
//!
//! The wire form is the one of the existing server: JSON objects whose key
//! order is meaningful (a replacement dictionary applies its pairs in order),
//! so objects are read into ordered pairs. A body that fails any check
//! changes nothing. The error texts are part of the wire and are not translated.

use std::collections::HashMap;
use std::fmt;

use hayamimi_asr::vad::Sensitivity;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::error::Category;
use serde_json::{Map, Value};

/// The largest body a control route reads: these are small hand-edited
/// dictionaries, not uploads.
pub const MAX_BODY: usize = 1_000_000;

/// A body that is refused; the route answers 400 with this text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadRequest(pub String);

impl BadRequest {
    fn new(text: &str) -> BadRequest {
        BadRequest(text.to_owned())
    }
}

impl fmt::Display for BadRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The pairs of a JSON object in the order they were written; a repeated
/// key keeps its first place and its last value.
#[derive(Debug)]
pub struct Ordered(pub Vec<(String, Value)>);

struct OrderedVisitor;

impl<'de> Visitor<'de> for OrderedVisitor {
    type Value = Ordered;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
        let mut pairs: Vec<(String, Value)> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            match index.get(&key) {
                Some(at) => pairs[*at].1 = value,
                None => {
                    index.insert(key.clone(), pairs.len());
                    pairs.push((key, value));
                }
            }
        }
        Ok(Ordered(pairs))
    }
}

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Ordered, D::Error> {
        deserializer.deserialize_map(OrderedVisitor)
    }
}

/// Writes ordered string pairs as a JSON object, in order.
pub struct PairsOut<'a>(pub &'a [(String, String)]);

impl Serialize for PairsOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(key, value)| (key, value)))
    }
}

/// Checks the size of a body: not empty, not past [`MAX_BODY`].
pub fn check_size(body: &[u8]) -> Result<(), BadRequest> {
    if body.is_empty() || body.len() > MAX_BODY {
        return Err(BadRequest::new("missing or oversized body"));
    }
    Ok(())
}

/// Parses a body that must be a JSON object, keeping the order of its pairs.
fn parse_ordered(body: &[u8]) -> Result<Ordered, BadRequest> {
    check_size(body)?;
    serde_json::from_slice::<Ordered>(body).map_err(|error| match error.classify() {
        Category::Data => BadRequest::new("body must be a JSON object"),
        _ => BadRequest(format!("invalid JSON: {error}")),
    })
}

/// Parses a body that must be a JSON object, with its members as a map.
fn parse_object(body: &[u8]) -> Result<Map<String, Value>, BadRequest> {
    check_size(body)?;
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(object)) => Ok(object),
        Ok(_) => Err(BadRequest::new("body must be a JSON object")),
        Err(error) => Err(BadRequest(format!("invalid JSON: {error}"))),
    }
}

/// The pairs of an object whose values are all strings.
fn string_pairs(pairs: Vec<(String, Value)>) -> Result<Vec<(String, String)>, BadRequest> {
    pairs
        .into_iter()
        .map(|(key, value)| match value {
            Value::String(text) => Ok((key, text)),
            _ => Err(BadRequest(format!("the value of {key:?} must be a string"))),
        })
        .collect()
}

/// `POST /replacements`: `{"wrong": "right", ...}`, replacing the whole dictionary.
pub fn parse_replacements(body: &[u8]) -> Result<Vec<(String, String)>, BadRequest> {
    string_pairs(parse_ordered(body)?.0)
}

/// What `POST /itn_overrides` carries.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ItnBody {
    pub exclude: Vec<String>,
    pub force: Vec<(String, String)>,
}

const ITN_SHAPE: &str = "\"exclude\" must be a list, \"force\" a dict";

#[derive(Deserialize)]
struct ForceOnly {
    force: Ordered,
}

/// `exclude`, absent or null as an empty list; every item a string.
fn exclude_list(object: &Map<String, Value>) -> Result<Vec<String>, BadRequest> {
    match object.get("exclude") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| BadRequest::new(ITN_SHAPE)),
        Some(_) => Err(BadRequest::new(ITN_SHAPE)),
    }
}

/// `force`, absent or null as no pairs; read again from the body to keep the order.
fn force_pairs(
    object: &Map<String, Value>,
    body: &[u8],
) -> Result<Vec<(String, String)>, BadRequest> {
    match object.get("force") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(_)) => {
            let ordered = serde_json::from_slice::<ForceOnly>(body)
                .map_err(|error| BadRequest(format!("invalid JSON: {error}")))?;
            string_pairs(ordered.force.0)
        }
        Some(_) => Err(BadRequest::new(ITN_SHAPE)),
    }
}

/// `POST /itn_overrides`: `{"exclude": [...], "force": {...}}`.
pub fn parse_itn(body: &[u8]) -> Result<ItnBody, BadRequest> {
    let object = parse_object(body)?;
    Ok(ItnBody {
        exclude: exclude_list(&object)?,
        force: force_pairs(&object, body)?,
    })
}

/// A change of the voice detector's settings; a member left out stays as it is.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct VadChange {
    pub threshold: Option<f32>,
    pub min_silence: Option<f32>,
    pub max_speech: Option<f32>,
}

impl VadChange {
    /// The settings after the change.
    pub fn apply(self, current: Sensitivity) -> Sensitivity {
        let mut next = current;
        if let Some(value) = self.threshold {
            next.threshold = value;
        }
        if let Some(value) = self.min_silence {
            next.min_silence = value;
        }
        if let Some(value) = self.max_speech {
            next.max_speech = value;
        }
        next
    }
}

/// What `POST /config` changes in this build.
#[derive(Debug, Default, PartialEq)]
pub struct ConfigChange {
    pub punctuate: Option<bool>,
    pub vad: Option<VadChange>,
}

/// A number member: absent or null is `None`, a non-number is refused.
fn number(object: &Map<String, Value>, key: &str) -> Result<Option<f64>, BadRequest> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(found)) => Ok(found.as_f64()),
        Some(_) => Err(BadRequest(format!("{key} must be a number"))),
    }
}

/// A number member that must be above zero, and at most `most` when given.
fn bounded(
    object: &Map<String, Value>,
    key: &str,
    most: Option<f64>,
) -> Result<Option<f32>, BadRequest> {
    let Some(value) = number(object, key)? else {
        return Ok(None);
    };
    if value <= 0.0 || most.is_some_and(|limit| value > limit) {
        let bound = if most.is_some() {
            "> 0 and <= 1"
        } else {
            "> 0"
        };
        return Err(BadRequest(format!("{key} must be {bound}, got {value}")));
    }
    Ok(Some(value as f32))
}

fn parse_vad(value: &Value) -> Result<VadChange, BadRequest> {
    let Value::Object(object) = value else {
        return Err(BadRequest::new("\"vad\" must be an object"));
    };
    Ok(VadChange {
        threshold: bounded(object, "threshold", Some(1.0))?,
        min_silence: bounded(object, "min_silence", None)?,
        max_speech: bounded(object, "max_speech", None)?,
    })
}

/// A member that this build does not have: accepted only as null.
fn only_null(object: &Map<String, Value>, key: &str) -> Result<(), BadRequest> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(_) => Err(BadRequest(format!("{key} is not available in this build"))),
    }
}

fn check_lang(object: &Map<String, Value>) -> Result<(), BadRequest> {
    match object.get("lang") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(code)) if code == "ja" => Ok(()),
        Some(_) => Err(BadRequest::new("lang must be \"ja\" in this build")),
    }
}

fn check_translate(object: &Map<String, Value>) -> Result<(), BadRequest> {
    match object.get("translate") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(items)) if items.is_empty() => Ok(()),
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => {
            Err(BadRequest::new("translate is not available in this build"))
        }
        Some(_) => Err(BadRequest::new(
            "\"translate\" must be a list of language codes",
        )),
    }
}

fn parse_punctuate(object: &Map<String, Value>) -> Result<Option<bool>, BadRequest> {
    match object.get("punctuate") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(BadRequest::new("\"punctuate\" must be a boolean")),
    }
}

/// `POST /config`: any subset of the members `GET /config` returns.
pub fn parse_config(body: &[u8]) -> Result<ConfigChange, BadRequest> {
    let object = parse_object(body)?;
    check_lang(&object)?;
    for key in ["dual_confirm", "lid_switch_confirm", "min_switch_s"] {
        only_null(&object, key)?;
    }
    check_translate(&object)?;
    let vad = match object.get("vad") {
        None | Some(Value::Null) => None,
        Some(value) => Some(parse_vad(value)?),
    };
    Ok(ConfigChange {
        punctuate: parse_punctuate(&object)?,
        vad,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(result: Result<impl fmt::Debug, BadRequest>) -> Option<String> {
        result.err().map(|error| error.0)
    }

    #[test]
    fn replacements_keep_the_order_they_were_written_in() -> Result<(), BadRequest> {
        let pairs = parse_replacements(br#"{"b":"1","a":"2","c":"3"}"#)?;
        let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, vec!["b", "a", "c"]);
        Ok(())
    }

    #[test]
    fn a_repeated_key_keeps_its_first_place_and_last_value() -> Result<(), BadRequest> {
        let pairs = parse_replacements(br#"{"a":"1","b":"2","a":"3"}"#)?;
        assert_eq!(
            pairs,
            vec![
                (String::from("a"), String::from("3")),
                (String::from("b"), String::from("2"))
            ]
        );
        Ok(())
    }

    #[test]
    fn an_empty_object_is_a_valid_empty_dictionary() -> Result<(), BadRequest> {
        assert!(parse_replacements(b"{}")?.is_empty());
        Ok(())
    }

    #[test]
    fn bodies_that_are_empty_oversized_unparsable_or_not_objects_are_named() {
        assert_eq!(
            refused(parse_replacements(b"")).as_deref(),
            Some("missing or oversized body")
        );
        let big = vec![b' '; MAX_BODY + 1];
        assert_eq!(
            refused(parse_replacements(&big)).as_deref(),
            Some("missing or oversized body")
        );
        assert!(refused(parse_replacements(b"{")).is_some_and(|t| t.starts_with("invalid JSON:")));
        assert_eq!(
            refused(parse_replacements(b"[1]")).as_deref(),
            Some("body must be a JSON object")
        );
        assert_eq!(
            refused(parse_replacements(b"\"x\"")).as_deref(),
            Some("body must be a JSON object")
        );
    }

    #[test]
    fn a_body_of_exactly_the_limit_is_read() {
        let mut body = br#"{"a":""#.to_vec();
        body.resize(MAX_BODY - 2, b'x');
        body.extend_from_slice(br#""}"#);
        assert_eq!(body.len(), MAX_BODY);
        assert!(parse_replacements(&body).is_ok());
    }

    #[test]
    fn a_replacement_value_that_is_not_a_string_is_named() {
        let said = refused(parse_replacements(br#"{"a":1}"#));
        assert!(said.is_some_and(|text| text.contains("\"a\"")));
    }

    #[test]
    fn itn_overrides_read_both_members_and_default_to_none() -> Result<(), BadRequest> {
        let body = parse_itn(r#"{"exclude":["一旦"],"force":{"b":"1","a":"2"}}"#.as_bytes())?;
        assert_eq!(body.exclude, vec![String::from("一旦")]);
        assert_eq!(body.force[0].0, "b");
        assert_eq!(parse_itn(b"{}")?, ItnBody::default());
        assert_eq!(
            parse_itn(br#"{"exclude":null,"force":null}"#)?,
            ItnBody::default()
        );
        Ok(())
    }

    #[test]
    fn itn_members_of_the_wrong_shape_are_refused_with_the_shape() {
        let bodies: [&[u8]; 4] = [
            br#"{"exclude":"x"}"#,
            br#"{"exclude":[1]}"#,
            br#"{"force":[]}"#,
            br#"{"force":{"a":1}}"#,
        ];
        for body in bodies {
            let said = refused(parse_itn(body));
            assert!(said.is_some(), "{}", String::from_utf8_lossy(body));
        }
        assert_eq!(
            refused(parse_itn(br#"{"exclude":"x"}"#)).as_deref(),
            Some(ITN_SHAPE)
        );
    }

    #[test]
    fn config_reads_punctuate_and_the_vad_members() -> Result<(), BadRequest> {
        let change =
            parse_config(br#"{"punctuate":false,"vad":{"threshold":0.6,"min_silence":0.4}}"#)?;
        assert_eq!(change.punctuate, Some(false));
        assert_eq!(
            change.vad,
            Some(VadChange {
                threshold: Some(0.6),
                min_silence: Some(0.4),
                max_speech: None
            })
        );
        assert_eq!(parse_config(b"{}")?, ConfigChange::default());
        Ok(())
    }

    #[test]
    fn config_refuses_values_out_of_range_and_names_the_member() {
        let cases: [(&[u8], &str); 5] = [
            (br#"{"vad":{"threshold":0}}"#, "threshold"),
            (br#"{"vad":{"threshold":1.5}}"#, "threshold"),
            (br#"{"vad":{"min_silence":-1}}"#, "min_silence"),
            (br#"{"vad":{"max_speech":0}}"#, "max_speech"),
            (br#"{"vad":{"max_speech":"x"}}"#, "max_speech"),
        ];
        for (body, key) in cases {
            let said = refused(parse_config(body));
            assert!(said.is_some_and(|text| text.contains(key)), "{key}");
        }
        assert!(refused(parse_config(br#"{"vad":1}"#)).is_some());
        assert!(refused(parse_config(br#"{"punctuate":"yes"}"#)).is_some());
    }

    #[test]
    fn the_upper_bound_of_the_threshold_is_included() {
        assert!(parse_config(br#"{"vad":{"threshold":1}}"#).is_ok());
    }

    #[test]
    fn members_this_build_lacks_are_accepted_only_as_null() {
        assert!(parse_config(br#"{"lang":"ja","dual_confirm":null,"translate":[]}"#).is_ok());
        assert!(refused(parse_config(br#"{"lang":"en"}"#)).is_some());
        assert!(refused(parse_config(br#"{"dual_confirm":true}"#)).is_some());
        assert!(refused(parse_config(br#"{"min_switch_s":2.0}"#)).is_some());
        assert!(refused(parse_config(br#"{"translate":["en"]}"#)).is_some());
        assert!(refused(parse_config(br#"{"translate":"en"}"#)).is_some());
    }

    #[test]
    fn a_vad_change_keeps_the_members_it_does_not_name() {
        let current = Sensitivity {
            threshold: 0.5,
            min_silence: 0.3,
            max_speech: 12.0,
        };
        let changed = VadChange {
            min_silence: Some(0.5),
            ..VadChange::default()
        }
        .apply(current);
        assert_eq!(
            changed,
            Sensitivity {
                threshold: 0.5,
                min_silence: 0.5,
                max_speech: 12.0
            }
        );
    }

    #[test]
    fn pairs_are_written_as_an_object_in_order() -> Result<(), serde_json::Error> {
        let pairs = vec![
            (String::from("b"), String::from("1")),
            (String::from("a"), String::from("2")),
        ];
        assert_eq!(
            serde_json::to_string(&PairsOut(&pairs))?,
            r#"{"b":"1","a":"2"}"#
        );
        Ok(())
    }
}
