use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::ser::SerializeStruct;
use serde::Serialize;
use stam::StamError;
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum ApiResponse {
    Text(String),
    Results(Vec<BTreeMap<String, String>>),
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        match self {
            Self::Text(s) => (StatusCode::OK, s).into_response(),
            Self::Results(data) => (StatusCode::OK, Json(data)).into_response(),
        }
    }
}

#[derive(Debug)]
pub enum ApiError {
    MissingArgument(&'static str),
    NotFound(&'static str),
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
                Self::StamError(_) => unreachable!("Already handled"),
            }
            state.end()
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::NOT_FOUND, Json(self)).into_response()
    }
}
