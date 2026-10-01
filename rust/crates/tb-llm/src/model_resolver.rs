//! Consumer lesen ausschließlich den zentral geprüften lokalen Stand.
use crate::hub::LlmError;

/// Startkonfiguration und Mock-Verträge. Echte Requests lesen immer den State.
pub fn configured_fireworks_model() -> String {
    resolved_fireworks_model()
        .unwrap_or_else(|| "accounts/fireworks/models/deepseek-v4p1-flash".into())
}
pub fn allowed_fireworks_model(model: &str) -> bool {
    fireworks_model_selection::version(model).is_some()
}
pub fn resolved_fireworks_model() -> Option<String> {
    fireworks_model_selection::selected_model().ok()
}
pub fn selected_model() -> Result<String, LlmError> {
    fireworks_model_selection::selected_model().map_err(|e| LlmError::Unavailable(e.to_string()))
}
