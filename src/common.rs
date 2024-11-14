use axum::{
    http::HeaderValue,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Response},
};
use serde::ser::SerializeStruct;
use serde::Serialize;
use stam::StamError;
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum ApiResponse {
    Text(String),
    Html(String),
    Json(String),
    /// W3C Web Annotations in JSON-LD
    JsonLd(String),
    JsonList(Vec<String>),
    JsonMapList(Vec<BTreeMap<String, String>>),
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        match self {
            Self::Text(s) => (StatusCode::OK, s).into_response(),
            Self::Html(s) => (StatusCode::OK, Html(s)).into_response(),
            Self::JsonLd(data) => (
                StatusCode::OK,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(
                        "application/ld+json; profile=\"http://www.w3.org/ns/anno.jsonld\"",
                    ),
                )],
                data,
            )
                .into_response(),
            Self::Json(data) => (StatusCode::OK, Json(data)).into_response(),
            Self::JsonList(data) => (StatusCode::OK, Json(data)).into_response(),
            Self::JsonMapList(data) => (StatusCode::OK, Json(data)).into_response(),
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
