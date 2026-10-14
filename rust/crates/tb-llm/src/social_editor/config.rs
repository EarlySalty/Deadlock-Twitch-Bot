use serde::{Deserialize, Serialize};

use super::{EditorError, ErrorKind};

const APPROVED_INITIAL_CAP_MICRO_USD: i64 = 2_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorConfig {
    pub schema_version: u32,
    pub provider: ProviderConfig,
    pub runtime: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub endpoint: String,
    pub model: String,
    pub effort: Effort,
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
    pub max_frames: usize,
    pub max_frame_bytes: usize,
    pub max_text_bytes: usize,
    pub max_schema_bytes: usize,
    pub request_timeout_ms: u64,
    pub total_deadline_ms: u64,
    pub max_retries: u8,
    pub retry_backoff_ms: u64,
    pub initial_total_cap_micro_usd: i64,
    pub pricing_version: u32,
    pub pricing: Pricing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    pub input_micro_usd_per_million: i64,
    pub output_micro_usd_per_million: i64,
    pub cache_creation_micro_usd_per_million: i64,
    pub cache_read_micro_usd_per_million: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedPricing {
    pub version: u32,
    pub model: String,
    pub rates: Pricing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocialAiSection {
    pub schema_version: u32,
    pub provider: ProviderConfig,
    pub pricing: TrustedPricing,
    pub runtime: serde_json::Value,
}

impl SocialAiSection {
    pub fn into_editor_config(self) -> Result<EditorConfig, EditorError> {
        self.provider.check_trusted(&self.pricing)?;
        let config = EditorConfig {
            schema_version: self.schema_version,
            provider: self.provider,
            runtime: self.runtime,
        };
        config.validate()?;
        Ok(config)
    }
}

impl EditorConfig {
    pub fn validate(&self) -> Result<(), EditorError> {
        if self.schema_version != 1 || !self.runtime.is_object() {
            return Err(EditorError::new(ErrorKind::Configuration));
        }
        self.provider.validate()
    }
}

impl ProviderConfig {
    pub fn check_trusted(&self, trusted: &TrustedPricing) -> Result<(), EditorError> {
        if self.pricing_version != trusted.version
            || self.model != trusted.model
            || self.pricing != trusted.rates
            || !trusted.rates.valid()
        {
            return Err(EditorError::new(ErrorKind::Configuration));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), EditorError> {
        let url = reqwest::Url::parse(&self.endpoint)
            .map_err(|_| EditorError::new(ErrorKind::Configuration))?;
        let official = url.scheme() == "https"
            && url.host_str() == Some("api.anthropic.com")
            && (url.port().is_none() || url.port() == Some(443));
        #[cfg(test)]
        let local = url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            && url.port().is_some();
        #[cfg(not(test))]
        let local = false;
        if !(official || local)
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || self.model.is_empty()
            || self.model.len() > 80
            || !self
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || self.max_input_tokens == 0
            || self.max_input_tokens > 128_000
            || self.max_output_tokens == 0
            || self.max_output_tokens > 4_096
            || self.max_frames > 8
            || self.max_frame_bytes == 0
            || self.max_frame_bytes > 256_000
            || self.max_text_bytes == 0
            || self.max_text_bytes > 32_000
            || self.max_schema_bytes == 0
            || self.max_schema_bytes > 32_000
            || !(1_000..=120_000).contains(&self.request_timeout_ms)
            || self.total_deadline_ms < self.request_timeout_ms
            || self.total_deadline_ms > 360_000
            || self.max_retries > 2
            || self.retry_backoff_ms > 3_000
            || !(1..=APPROVED_INITIAL_CAP_MICRO_USD).contains(&self.initial_total_cap_micro_usd)
            || !self.pricing.valid()
        {
            return Err(EditorError::new(ErrorKind::Configuration));
        }
        Ok(())
    }
}

impl Pricing {
    fn valid(&self) -> bool {
        [
            self.input_micro_usd_per_million,
            self.output_micro_usd_per_million,
            self.cache_creation_micro_usd_per_million,
            self.cache_read_micro_usd_per_million,
        ]
        .into_iter()
        .all(|rate| (1..=100_000_000).contains(&rate))
    }

    fn charge(tokens: u64, rate: i64) -> Option<i64> {
        let units = (tokens as u128).checked_mul(rate as u128)?;
        i64::try_from(units.checked_add(999_999)? / 1_000_000).ok()
    }

    pub(crate) fn reservation(&self, input_bound: u64, output_bound: u64) -> Option<i64> {
        let maximum_input_rate = self
            .input_micro_usd_per_million
            .max(self.cache_creation_micro_usd_per_million)
            .max(self.cache_read_micro_usd_per_million);
        Self::charge(input_bound, maximum_input_rate)?.checked_add(Self::charge(
            output_bound,
            self.output_micro_usd_per_million,
        )?)
    }

    pub(crate) fn measured(
        &self,
        input: u64,
        output: u64,
        cache_creation: u64,
        cache_read: u64,
    ) -> Option<i64> {
        let prices = [
            (input, self.input_micro_usd_per_million),
            (output, self.output_micro_usd_per_million),
            (cache_creation, self.cache_creation_micro_usd_per_million),
            (cache_read, self.cache_read_micro_usd_per_million),
        ];
        prices.into_iter().try_fold(0_i64, |total, (tokens, rate)| {
            total.checked_add(Self::charge(tokens, rate)?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social_editor::fixture::SOCIAL_AI_SECTION;

    fn section(raw: &str) -> SocialAiSection {
        toml::from_str(raw).expect("TOML")
    }

    #[test]
    fn toml_is_strict_and_cap_cannot_be_raised() {
        let raw = SOCIAL_AI_SECTION;
        let mut config = section(raw)
            .into_editor_config()
            .expect("valid initial config");
        config.provider.initial_total_cap_micro_usd += 1;
        assert!(config.validate().is_err());
        for unknown in [
            format!("unknown_setting = true\n{raw}"),
            raw.replace("[provider]", "[provider]\nunknown_setting = true"),
            raw.replace(
                "[provider.pricing]",
                "[provider.pricing]\nunknown_setting = true",
            ),
            raw.replace("[pricing]", "[pricing]\nunknown_setting = true"),
            raw.replace("[pricing.rates]", "[pricing.rates]\nunknown_setting = true"),
        ] {
            let error = toml::from_str::<SocialAiSection>(&unknown).expect_err("unknown setting");
            assert!(error.to_string().contains("unknown field"));
            assert!(error.to_string().contains("unknown_setting"));
        }
    }

    #[test]
    fn provider_must_match_the_trusted_pricing_table() {
        let mutations: [fn(&mut SocialAiSection); 6] = [
            |s| s.provider.pricing.input_micro_usd_per_million = 1,
            |s| s.provider.pricing_version += 1,
            |s| s.provider.model = "unapproved-model".to_owned(),
            |s| s.pricing.rates.output_micro_usd_per_million = 1,
            |s| s.pricing.version += 1,
            |s| s.pricing.model = "unapproved-model".to_owned(),
        ];
        for mutate in mutations {
            let mut config = section(SOCIAL_AI_SECTION);
            mutate(&mut config);
            assert_eq!(
                config.into_editor_config().err().map(|error| error.kind),
                Some(ErrorKind::Configuration)
            );
        }
    }

    #[test]
    fn exact_integer_pricing_ceil() {
        let config = section(SOCIAL_AI_SECTION)
            .into_editor_config()
            .expect("configuration");
        assert_eq!(config.provider.pricing.reservation(1, 1), Some(25));
    }
}
