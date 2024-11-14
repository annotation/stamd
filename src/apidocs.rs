use utoipa::ToSchema;

#[derive(ToSchema)]
/// An object in STAM JSON. See <https://github.com/annotation/stam/blob/master/README.md#stam-json> . The schema is a rough skeleton and not complete!
#[allow(dead_code)]
pub struct StamJson {
    #[schema(rename = "@id")]
    /// The identifier
    id: Option<String>,

    #[schema(rename = "@type")]
    /// The type of this object (any STAM object)
    r#type: String,
}

#[derive(ToSchema)]
/// An API error in JSON
#[allow(dead_code)]
pub struct ApiError {
    #[schema(rename = "@type")]
    /// The type of error, this will be "ApiError"
    r#type: String,

    /// The error name (MissingArgument, InternalError, NotFound, CustomNotFound, NotAcceptable, PermissionDenied)
    name: String,

    /// The error message
    message: String,
}

#[derive(ToSchema)]
/// A STAM error in JSON
#[allow(dead_code)]
pub struct StamError {
    #[schema(rename = "@type")]
    /// The type of error, this will be "StamError"
    r#type: String,

    /// The error message
    message: String,
}

#[derive(ToSchema)]
/// A web annotation in JSON-LD, this schema is a rough skeleton and not complete! See <https://www.w3.org/TR/annotation-model/>
#[allow(dead_code)]
pub struct WebAnnotation {
    #[schema(rename = "@context")]
    /// JSON-LD context
    context: String,

    #[schema(rename = "@type")]
    /// The type of the RDF resource.
    r#type: String,

    #[schema(rename = "@id")]
    /// The identifier
    id: Option<String>,
}
