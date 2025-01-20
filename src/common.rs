use axum::{
    http::HeaderValue,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Response},
};
use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::value::Value;
use stam::StamError;
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum ApiResponse {
    Created(),
    Text(String),
    Html(String),
    RawJson(String),
    /// W3C Web Annotations in JSON-LD
    RawJsonLd(String),
    JsonList(Vec<Value>),
    JsonMap(Vec<BTreeMap<String, Value>>),
    QueryUI(Vec<String>), //takes a list of store IDs
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        match self {
            Self::Created() => (StatusCode::CREATED, "created").into_response(),
            Self::Text(s) => (StatusCode::OK, s).into_response(),
            Self::Html(s) => (StatusCode::OK, Html(s)).into_response(),
            Self::RawJsonLd(data) => (
                StatusCode::OK,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(
                        "application/ld+json", //; profile=\"http://www.w3.org/ns/anno.jsonld\"",
                    ),
                )],
                data,
            )
                .into_response(),
            Self::RawJson(data) => (
                StatusCode::OK,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(
                        "application/ld+json", //; profile=\"http://www.w3.org/ns/anno.jsonld\"",
                    ),
                )],
                data,
            )
                .into_response(),
            Self::JsonList(data) => (StatusCode::OK, Json(data)).into_response(),
            Self::JsonMap(data) => (StatusCode::OK, Json(data)).into_response(),
            Self::QueryUI(store_ids) => {
                let options: Vec<_> = store_ids
                    .into_iter()
                    .map(|s| format!("<option value=\"{}\">{}</option>", s, s))
                    .collect();
                let html: String = format!(
                    "<html>
<head>
    <meta content=\"text/html; charset=utf8\" http-equiv=\"content-type\">
    <title>stamd</title>
</head>
<body>
<h1>stamd</h1>
<p>See <a href=\"/swagger-ui\">OpenAPI specification</a></p>
<hr/>
<form method=\"post\" action=\"/query\">
<label>Store:</label> <select name=\"store\">{}</select><br/>
<label>Query (<a href=\"https://github.com/annotation/stam/tree/master/extensions/stam-query\">STAMQL</a>):</label><br/><textarea name=\"query\" style=\"width: 60%; min-height: 360px;\" spellcheck=\"false\"></textarea><br/>
<input type=\"submit\" />
</form>
</body></html>",
                    options.join("")
                );
                (StatusCode::OK, Html(html)).into_response()
            }
        }
    }
}

#[derive(Debug)]
pub enum ApiError {
    MissingArgument(&'static str),
    InternalError(&'static str),
    NotFound(&'static str),
    CustomNotFound(String),
    NotAcceptable(&'static str),
    PermissionDenied(&'static str),
    StamError(StamError),
}

impl Serialize for ApiError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Self::StamError(e) = self {
            e.serialize(serializer)
        } else {
            let mut state = serializer.serialize_struct("ApiError", 3)?;
            state.serialize_field("@type", "ApiError")?;
            match self {
                Self::MissingArgument(s) => {
                    state.serialize_field("name", "MissingArgument")?;
                    state.serialize_field("message", s)?;
                }
                Self::NotFound(s) => {
                    state.serialize_field("name", "NotFound")?;
                    state.serialize_field("message", s)?;
                }
                Self::CustomNotFound(s) => {
                    state.serialize_field("name", "NotFound")?;
                    state.serialize_field("message", s)?;
                }
                Self::NotAcceptable(s) => {
                    state.serialize_field("name", "NotAcceptable")?;
                    state.serialize_field("message", s)?;
                }
                Self::PermissionDenied(s) => {
                    state.serialize_field("name", "PermissionDenied")?;
                    state.serialize_field("message", s)?;
                }
                Self::InternalError(s) => {
                    state.serialize_field("name", "InternalError")?;
                    state.serialize_field("message", s)?;
                }
                Self::StamError(_) => unreachable!("Already handled"),
            }
            state.end()
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let statuscode = match self {
            Self::InternalError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PermissionDenied(..) => StatusCode::FORBIDDEN,
            Self::NotAcceptable(..) => StatusCode::NOT_ACCEPTABLE,
            _ => StatusCode::NOT_FOUND,
        };
        (statuscode, Json(self)).into_response()
    }
}
