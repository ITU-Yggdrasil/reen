use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn canonicalize_json_value(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(canonicalize_json_value)
                .collect::<Vec<_>>(),
        ),
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::new();
            for (key, value) in entries {
                out.insert(key, canonicalize_json_value(value));
            }
            Value::Object(out)
        }
        other => other,
    }
}

pub fn canonical_json_string<T: Serialize>(input: &T) -> String {
    serde_json::to_value(input)
        .map(canonicalize_json_value)
        .and_then(|value| serde_json::to_string(&value))
        .unwrap_or_else(|_| "{}".to_string())
}

pub fn cache_key_for_input<T: Serialize>(agent_instructions: &str, input: &T) -> String {
    let input_json = canonical_json_string(input);
    let composite = format!("{}:{}", agent_instructions, input_json);
    let mut hasher = Sha256::new();
    hasher.update(composite.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{cache_key_for_input, canonical_json_string};
    use serde::Serialize;
    use serde_json::json;
    use std::collections::HashMap;

    #[derive(Serialize)]
    struct NestedInput {
        name: String,
        additional: HashMap<String, serde_json::Value>,
    }

    #[test]
    fn canonical_json_string_sorts_nested_object_keys_recursively() {
        let mut additional = HashMap::new();
        additional.insert(
            "zeta".to_string(),
            json!({
                "beta": 2,
                "alpha": 1
            }),
        );
        additional.insert(
            "alpha".to_string(),
            json!([
                {"delta": 4, "charlie": 3},
                {"bravo": 2, "alpha": 1}
            ]),
        );

        let input = NestedInput {
            name: "demo".to_string(),
            additional,
        };

        let canonical = canonical_json_string(&input);
        assert_eq!(
            canonical,
            r#"{"additional":{"alpha":[{"charlie":3,"delta":4},{"alpha":1,"bravo":2}],"zeta":{"alpha":1,"beta":2}},"name":"demo"}"#
        );
    }

    #[test]
    fn cache_key_for_input_is_stable_across_hashmap_insertion_order() {
        let mut additional_a = HashMap::new();
        additional_a.insert("zeta".to_string(), json!({"beta": 2, "alpha": 1}));
        additional_a.insert("alpha".to_string(), json!(true));

        let mut additional_b = HashMap::new();
        additional_b.insert("alpha".to_string(), json!(true));
        additional_b.insert("zeta".to_string(), json!({"alpha": 1, "beta": 2}));

        let input_a = NestedInput {
            name: "demo".to_string(),
            additional: additional_a,
        };
        let input_b = NestedInput {
            name: "demo".to_string(),
            additional: additional_b,
        };

        assert_eq!(
            cache_key_for_input("instructions", &input_a),
            cache_key_for_input("instructions", &input_b)
        );
    }
}
