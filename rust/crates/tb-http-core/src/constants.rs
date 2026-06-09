//! Header-Konstanten und Basis-Pfad der internen API.

/// Inbound- und Outbound-Auth-Header.
pub const INTERNAL_TOKEN_HEADER: &str = "X-Internal-Token";

/// Idempotency-Key-Header (inbound, ohne `X-`-Präfix).
///
/// Abweichung vom outbound-Header (`X-Idempotency-Key`) — absichtlich,
/// da die interne API einem anderen Stil folgt als die Broker-Calls.
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";

/// Basis-Pfad der internen API.
pub const INTERNAL_API_BASE_PATH: &str = "/internal/twitch/v1";
