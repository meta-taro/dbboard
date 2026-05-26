//! Adapter-neutral representation of a single database cell.
//!
//! Each adapter (Turso, Neon, Supabase, ...) translates its native
//! driver value into this enum so the UI layer never sees adapter
//! types. Variants intentionally mirror SQLite storage classes plus
//! the bare minimum needed for PostgreSQL adapters to widen later.

use std::fmt;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Integer(n) => write!(f, "{n}"),
            Self::Real(x) => write!(f, "{x}"),
            Self::Text(s) => write!(f, "{s}"),
            Self::Blob(b) => write!(f, "<blob: {} bytes>", b.len()),
        }
    }
}

/// JSON object key that tags a base64-encoded blob. Fixed by the API
/// contract (`docs/api-contract.md`); both dbboard and dbboard-web
/// must agree on it. The `$` prefix keeps it from colliding with any
/// natural string value, which serializes as a bare JSON string.
const BLOB_KEY: &str = "$blob";

// Value maps onto native JSON scalars rather than serde's default
// externally-tagged form (`{"Integer":1}`) so the wire format reads
// like ordinary JSON and mirrors dbboard-web. Blobs are the one
// exception: JSON has no byte type, so they ride inside a tagged
// object `{"$blob":"<base64>"}`.
impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Integer(n) => serializer.serialize_i64(*n),
            Self::Real(x) => serializer.serialize_f64(*x),
            Self::Text(s) => serializer.serialize_str(s),
            Self::Blob(bytes) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(BLOB_KEY, &BASE64.encode(bytes))?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // JSON is self-describing, so dispatch on the encountered token.
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "null, a number, a string, or a {{\"{BLOB_KEY}\":...}} object"
        )
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Integer(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        i64::try_from(v)
            .map(Value::Integer)
            .map_err(de::Error::custom)
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Real(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Text(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::Text(v))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let key: Option<String> = map.next_key()?;
        match key.as_deref() {
            Some(BLOB_KEY) => {
                let encoded: String = map.next_value()?;
                let bytes = BASE64.decode(encoded).map_err(de::Error::custom)?;
                if map.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom("unexpected extra key in blob object"));
                }
                Ok(Value::Blob(bytes))
            }
            Some(other) => Err(de::Error::custom(format!(
                "unexpected key {other:?}, expected {BLOB_KEY}"
            ))),
            None => Err(de::Error::custom(
                "expected a blob object, got an empty map",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    fn round_trip(value: &Value) -> Value {
        let json = serde_json::to_string(value).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn null_is_null_other_variants_are_not() {
        assert!(Value::Null.is_null());
        assert!(!Value::Integer(0).is_null());
        assert!(!Value::Real(0.0).is_null());
        assert!(!Value::Text(String::new()).is_null());
        assert!(!Value::Blob(Vec::new()).is_null());
    }

    #[test]
    fn display_renders_null_as_keyword() {
        assert_eq!(Value::Null.to_string(), "NULL");
    }

    #[test]
    fn display_renders_text_without_quotes() {
        assert_eq!(Value::Text("hello".into()).to_string(), "hello");
    }

    #[test]
    fn display_renders_integer_in_decimal() {
        assert_eq!(Value::Integer(42).to_string(), "42");
        assert_eq!(Value::Integer(-7).to_string(), "-7");
    }

    #[test]
    fn display_renders_blob_as_byte_count_summary() {
        assert_eq!(Value::Blob(vec![0; 12]).to_string(), "<blob: 12 bytes>");
    }

    #[test]
    fn null_serializes_as_json_null() {
        assert_eq!(serde_json::to_string(&Value::Null).unwrap(), "null");
        assert_eq!(round_trip(&Value::Null), Value::Null);
    }

    #[test]
    fn integer_serializes_as_bare_number() {
        assert_eq!(serde_json::to_string(&Value::Integer(-7)).unwrap(), "-7");
        assert_eq!(round_trip(&Value::Integer(-7)), Value::Integer(-7));
    }

    #[test]
    fn real_serializes_as_bare_number() {
        assert_eq!(round_trip(&Value::Real(1.5)), Value::Real(1.5));
    }

    #[test]
    fn text_serializes_as_bare_string() {
        assert_eq!(
            serde_json::to_string(&Value::Text("hi".into())).unwrap(),
            "\"hi\""
        );
        assert_eq!(
            round_trip(&Value::Text("hi".into())),
            Value::Text("hi".into())
        );
    }

    #[test]
    fn blob_serializes_as_tagged_base64_object() {
        let blob = Value::Blob(vec![0, 255, 12]);
        assert_eq!(serde_json::to_string(&blob).unwrap(), r#"{"$blob":"AP8M"}"#);
        assert_eq!(round_trip(&blob), blob);
    }

    #[test]
    fn empty_blob_round_trips() {
        assert_eq!(
            round_trip(&Value::Blob(Vec::new())),
            Value::Blob(Vec::new())
        );
    }

    #[test]
    fn unsigned_integer_in_i64_range_deserializes_as_integer() {
        let v: Value = serde_json::from_str("9223372036854775807").unwrap();
        assert_eq!(v, Value::Integer(i64::MAX));
    }

    #[test]
    fn malformed_blob_object_is_rejected() {
        assert!(serde_json::from_str::<Value>(r#"{"$blob":"not base64!!"}"#).is_err());
        assert!(serde_json::from_str::<Value>(r#"{"other":"x"}"#).is_err());
    }
}
