use std::convert::Infallible;

use axum::{extract::FromRequestParts, http::request::Parts};
use tb_domain::DashboardLanguage;

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestLanguage(pub DashboardLanguage);

impl<S> FromRequestParts<S> for RequestLanguage
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get(DashboardLanguage::HEADER)
            .and_then(|raw| raw.to_str().ok());
        Ok(Self(DashboardLanguage::from_header_value(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::RequestLanguage;
    use axum::{extract::FromRequestParts, http::Request};
    use tb_domain::DashboardLanguage;

    async fn language_of(request: Request<()>) -> DashboardLanguage {
        let (mut parts, ()) = request.into_parts();
        let Ok(RequestLanguage(lang)) = RequestLanguage::from_request_parts(&mut parts, &()).await;
        lang
    }

    #[tokio::test]
    async fn reads_the_dashboard_language_header() {
        let english = Request::builder()
            .header("X-Dashboard-Language", "en")
            .body(())
            .expect("request");
        assert_eq!(language_of(english).await, DashboardLanguage::En);

        let missing = Request::builder().body(()).expect("request");
        assert_eq!(language_of(missing).await, DashboardLanguage::De);
    }
}
