use serde_json::Value;

use super::Pricing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

impl Usage {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let get = |key: &str| object.get(key).and_then(Value::as_u64);
        Some(Self {
            input_tokens: get("input_tokens"),
            output_tokens: get("output_tokens"),
            cache_creation_input_tokens: get("cache_creation_input_tokens"),
            cache_read_input_tokens: get("cache_read_input_tokens"),
        })
    }

    pub(crate) fn estimated_cost(&self, pricing: &Pricing) -> Option<i64> {
        pricing.measured(
            self.input_tokens?,
            self.output_tokens?,
            self.cache_creation_input_tokens?,
            self.cache_read_input_tokens?,
        )
    }
}
