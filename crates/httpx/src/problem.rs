use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};

/// The media type for RFC 9457 problem details.
pub const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";

/// An RFC 9457 (Problem Details for HTTP APIs) error body. It gives every
/// service one machine-readable error shape instead of ad-hoc `{"error": …}`
/// JSON. `type` defaults to `about:blank` and `title` to the status' canonical
/// reason phrase when left unset. Extensions are merged in as top-level members,
/// as the RFC allows (e.g. `code`, `errors`, `trace_id`).
///
/// It implements [`IntoResponse`], so an axum handler can `return problem`
/// directly, and the response carries the `application/problem+json` type.
#[derive(Debug, Clone)]
pub struct Problem {
    status: StatusCode,
    type_uri: Option<String>,
    title: Option<String>,
    detail: Option<String>,
    instance: Option<String>,
    extensions: Map<String, Value>,
}

impl Problem {
    /// A problem for `status` with an occurrence-specific `detail`.
    #[must_use]
    pub fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        Self {
            status,
            type_uri: None,
            title: None,
            detail: Some(detail.into()),
            instance: None,
            extensions: Map::new(),
        }
    }

    /// Set the problem `type` URI.
    #[must_use]
    pub fn with_type(mut self, type_uri: impl Into<String>) -> Self {
        self.type_uri = Some(type_uri.into());
        self
    }

    /// Set the short, human-readable `title`.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the `instance` URI identifying this specific occurrence.
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// Add a top-level extension member (e.g. `code`).
    #[must_use]
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key.into(), value.into());
        self
    }

    /// Render the flat JSON object: the standard members plus any extensions.
    fn to_json(&self) -> Value {
        let mut map = self.extensions.clone();
        map.insert(
            "type".to_owned(),
            Value::String(
                self.type_uri
                    .clone()
                    .unwrap_or_else(|| "about:blank".to_owned()),
            ),
        );
        map.insert(
            "title".to_owned(),
            Value::String(
                self.title.clone().unwrap_or_else(|| {
                    self.status.canonical_reason().unwrap_or("Error").to_owned()
                }),
            ),
        );
        map.insert("status".to_owned(), Value::from(self.status.as_u16()));
        if let Some(detail) = &self.detail {
            map.insert("detail".to_owned(), Value::String(detail.clone()));
        }
        if let Some(instance) = &self.instance {
            map.insert("instance".to_owned(), Value::String(instance.clone()));
        }
        Value::Object(map)
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let body = serde_json::to_vec(&self.to_json()).unwrap_or_default();
        Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, PROBLEM_CONTENT_TYPE)
            .body(Body::from(body))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }
}
