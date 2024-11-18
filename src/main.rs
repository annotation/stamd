use axum::{
    body::Body, extract::Path, extract::Query, extract::State, http::Request, routing::get,
    routing::post, Router, ServiceExt,
};
use clap::Parser;
use serde_json::value::Value;
use stam::FindText;
use stam::WebAnnoConfig;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tower::layer::Layer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, error};

use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use stam::{Config, Offset, QueryIter, StamError, Text};
use stamtools::view::HtmlWriter;

mod apidocs;
mod common;
mod multistore;
use common::{ApiError, ApiResponse};
use multistore::StorePool;

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const CONTENT_TYPE_JSON: &'static str = "application/json";
const CONTENT_TYPE_JSONLD: &'static str = "application/ld+json";
const CONTENT_TYPE_HTML: &'static str = "text/html";
const CONTENT_TYPE_TEXT: &'static str = "text/plain";

#[derive(Parser, Debug)]
struct Args {
    #[arg(
        short,
        long,
        default_value_os = "127.0.0.1:8080",
        help = "The host and port to bind to"
    )]
    bind: String,

    #[arg(
        short = 'd',
        long,
        default_value_os = ".",
        help = "The base directory to serve from"
    )]
    basedir: String,

    #[arg(
        short = 'u',
        long,
        help = "The public-facing base URL. Also used as IRI for webannotations."
    )]
    baseurl: Option<String>,

    #[arg(
        short = 'e',
        long,
        default_value_os = "store.stam.json",
        help = "The extension for annotation stores"
    )]
    extension: String,

    #[arg(
        long,
        default_value_t = 600,
        help = "Number of seconds before stores are unloaded again"
    )]
    unload_time: u64,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "Sets all underlying stores as read-only"
    )]
    readonly: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Output logging info on incoming requests"
    )]
    debug: bool,

    #[arg(
        long = "add-context",
        help = "(for Web Annotation output only) URL to a JSONLD context to include"
    )]
    add_context: Vec<String>,

    #[arg(
        long = "ns",
        help = "(for Web Annotation output only) Add a namespace to the JSON-LD context, syntax is: namespace: uri"
    )]
    namespaces: Vec<String>,

    #[arg(
        long = "no-extra-target",
        help = "(for Web Annotation output only) By default, stamd adds an extra target to Web Annotations with a TextPositionSelector, this is a URL that can be resolved directly by stamd. If you don't want this behaviour, set this."
    )]
    no_extra_target: bool,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_stores,
        get_query,
        create_store,
        create_resource,
        get_annotation_list,
        get_annotation,
        get_resource_list,
        get_resource,
        get_textselection,
    ),
    tags(
        (name = "stamd", description = "WebAPI for stam")
    )
)]
pub struct ApiDoc;

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // set up config for webannotations
    let mut context_namespaces = Vec::new();
    for assignment in args.namespaces.iter() {
        let result: Vec<_> = assignment.splitn(2, ":").collect();
        if result.len() != 2 {
            error!("Syntax for --ns should be `ns: uri_prefix`");
        } else {
            context_namespaces.push((result[1].trim().to_string(), result[0].trim().to_string()));
        }
    }
    let webannoconfig = WebAnnoConfig {
        extra_context: args.add_context,
        context_namespaces,
        ..WebAnnoConfig::default()
    };

    let storepool = StorePool::new(
        args.basedir,
        if let Some(baseurl) = args.baseurl.as_ref() {
            baseurl.to_string()
        } else {
            format!("http://{}/", args.bind)
        },
        args.extension,
        args.readonly,
        args.unload_time,
        args.no_extra_target,
        webannoconfig,
        Config::default(),
    )
    .expect("Base directory must exist");

    if args.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

    let storepool: Arc<StorePool> = storepool.into();
    let storepool_flush = storepool.clone();

    std::thread::spawn(move || loop {
        std::thread::sleep(FLUSH_INTERVAL);
        match storepool_flush.flush(false) {
            Err(e) => error!("Flush failed! {:?}", e),
            Ok(v) => {
                if args.debug {
                    debug!("Flushed {} store(s)", v.len());
                }
            }
        }
    });

    let app = Router::new()
        .route("/", get(list_stores))
        .route("/:store_id", post(create_store))
        .route("/:store_id", get(get_query))
        .route("/:store_id/annotations/:annotation_id", get(get_annotation))
        .route("/:store_id/annotations", get(get_annotation_list))
        .route(
            "/:store_id/resources/:resource_id/:begin/:end",
            get(get_textselection),
        )
        .route("/:store_id/resources", get(get_resource_list))
        .route("/:store_id/resources/:resource_id", get(get_resource))
        .route("/:store_id/resources/:resource_id", post(create_resource))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .with_state(storepool.clone());

    //allow trailing slashes as well: (conflicts with swagger-ui!)
    //let app = NormalizePathLayer::trim_trailing_slash().layer(app);

    eprintln!("[stamd] listening on {}", args.bind);
    let listener = tokio::net::TcpListener::bind(args.bind).await.unwrap();
    axum::serve(
        listener, app,
        //ServiceExt::<axum::http::Request<Body>>::into_make_service(app),
    )
    .with_graceful_shutdown(shutdown_signal(storepool))
    .await
    .unwrap();
}

async fn shutdown_signal(storepool: Arc<StorePool>) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            storepool.flush(true).expect("Clean shutdown failed");
        }
        _ = terminate => {
            storepool.flush(true).expect("Clean shutdown failed");
        }
    }
}

#[utoipa::path(
    get,
    path = "/",
    responses(
        (status = 200, body = [String], description = "Returns a simple list of all available annotation stores"),
    )
)]
/// Runs all available annotation stores.
async fn list_stores(
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    if let Ok(CONTENT_TYPE_JSON) = negotiate_content_type(&request, &[CONTENT_TYPE_JSON]) {
        let extension = format!(".{}", storepool.extension());
        let mut store_ids: Vec<serde_json::Value> = Vec::new();
        for entry in std::fs::read_dir(storepool.basedir())
            .map_err(|_| ApiError::InternalError("Unable to read base directory"))?
        {
            let entry = entry.unwrap();
            if let Some(filename) = entry.file_name().to_str() {
                if let Some(pos) = filename.find(&extension) {
                    store_ids.push(filename[0..pos].into());
                }
            }
        }
        Ok(ApiResponse::JsonList(store_ids))
    } else {
        Err(ApiError::NotAcceptable(
            "Accept headed could not be satisfied (try application/json)",
        ))
    }
}

#[utoipa::path(
    post,
    path = "/{store_id}",
    responses(
        (status = 201, description = "Returned when successfully created"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance the store is configured as read-only or the store already exists", content_type = "application/json")
    )
)]
/// Create a new annotation store
async fn create_store(
    Path(store_id): Path<String>,
    storepool: State<Arc<StorePool>>,
) -> Result<ApiResponse, ApiError> {
    storepool.new_store(&store_id)?;
    Ok(ApiResponse::Created())
}

#[utoipa::path(
    post,
    path = "/{store_id}/resources/{resource_id}",
    request_body(content_type = "text/plain", description = "The full text of the resource"),
    responses(
        (status = 201, description = "Returned when successfully created"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance the store is configured as read-only or the resource already exists", content_type = "application/json")
    )
)]
/// Create a new text resource, the request body contains the text.
async fn create_resource(
    Path((store_id, resource_id)): Path<(String, String)>,
    storepool: State<Arc<StorePool>>,
    text: String,
) -> Result<ApiResponse, ApiError> {
    storepool.new_resource(&store_id, &resource_id, text)?;
    Ok(ApiResponse::Created())
}

#[utoipa::path(
    get,
    path = "/{store_id}",
    params(
        ("store_id" = String, Path, description = "The identifier of the store"),
        ("query" = String, Query, description = "A query in STAMQL, see <https://github.com/annotation/stam/tree/master/extensions/stam-query> for the syntax.", allow_reserved),
        ("use" = Option<String>, Query, description = "Select a single variable from the query (by name, without '?' prefix), to constrain the result set accordingly.")
    ),
    responses(
        (status = 200, description = "Query result. Several return types are supported via content negotation, but not all content types can be used for all queries. Most notably, the plain text type only works if the query produces a single item that holds text as result.",content(
            ([BTreeMap<String,apidocs::StamJson>] = "application/json"),
            ([apidocs::StamJson] = "application/json"),
            (String = "text/html"),
            (String = "text/plain"),
        )),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered for your query.", content_type = "application/json"),
        (status = 404, body = apidocs::StamError, description = "Return when the query is invalid or another error occurs", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "Returned with name `MissingArgument` if you forget the 'query' parameter", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "Returned with name `NotFound` if the store does not exist", content_type = "application/json"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance when you send a query that edits the data but the store is configured as read-only", content_type = "application/json")
    )
)]
/// Run a query on an annotation store. The query is formulated in STAMQL.
async fn get_query(
    Path(store_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    if let Some(querystring) = params.get("query") {
        let (query, _) = stam::Query::parse(querystring)?;
        if let Ok(CONTENT_TYPE_HTML) = negotiate_content_type(
            &request,
            &[CONTENT_TYPE_JSON, CONTENT_TYPE_HTML, CONTENT_TYPE_TEXT],
        ) {
            storepool.map(&store_id, |store| {
                let htmlwriter =
                    HtmlWriter::new(&store, query, params.get("use").map(|s| s.as_str()))
                        .map_err(|e| ApiError::CustomNotFound(e))?;
                Ok(ApiResponse::Html(htmlwriter.to_string()))
            })
        } else if query.querytype().readonly() {
            storepool.map(&store_id, |store| match store.query(query) {
                Err(err) => Err(ApiError::StamError(err)),
                Ok(queryiter) => {
                    query_results(queryiter, &request, params.get("use").map(|s| s.as_str()))
                }
            })
        } else {
            storepool.map_mut(&store_id, |store| match store.query_mut(query) {
                Err(err) => Err(ApiError::StamError(err)),
                Ok(queryiter) => {
                    query_results(queryiter, &request, params.get("use").map(|s| s.as_str()))
                }
            })
        }
    } else {
        Err(ApiError::MissingArgument("query"))
    }
}

#[utoipa::path(
    get,
    path = "/{store_id}/annotations",
    params(
        ("store_id" = String, Path, description = "The identifier of the store"),
    ),
    responses(
        (status = 200, body = [String], description = "Returns a simple list of all available annotations (IDs), for the given store"),
        (status = 404, body = apidocs::ApiError, description = "Returned with name `NotFound` if the store does not exist", content_type = "application/json"),
    )
)]
/// Returns the public identifiers of all available annotations in a given annotation store
async fn get_annotation_list(
    Path(store_id): Path<String>,
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    storepool.map(&store_id, |store| {
        match negotiate_content_type(&request, &[CONTENT_TYPE_JSON]) {
            Ok(CONTENT_TYPE_JSON) => {
                //TODO: may be a fairly expensive copy if there are lots of annotations, no pagination either here
                let annotations: Vec<serde_json::Value> = store
                    .annotations()
                    .filter_map(|a| a.id().map(|s| s.into()))
                    .collect();
                Ok(ApiResponse::JsonList(annotations))
            }
            _ => Err(ApiError::NotAcceptable(
                "Accept headed could not be satisfied (try application/json)",
            )),
        }
    })
}

#[utoipa::path(
    get,
    path = "/{store_id}/resources",
    params(
        ("store_id" = String, Path, description = "The identifier of the store"),
    ),
    responses(
        (status = 200, body = [String], description = "Returns a simple list of all available resources (IDs), for the given store"),
        (status = 404, body = apidocs::ApiError, description = "Returned with name `NotFound` if the store does not exist", content_type = "application/json"),
    )
)]
/// Returns the public identifiers of all available resources in a given annotation store
async fn get_resource_list(
    Path(store_id): Path<String>,
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    storepool.map(&store_id, |store| {
        match negotiate_content_type(&request, &[CONTENT_TYPE_JSON]) {
            Ok(CONTENT_TYPE_JSON) => {
                //TODO: may be a fairly expensive copy if there are lots of resources, no pagination either here
                let resources: Vec<serde_json::Value> = store
                    .resources()
                    .filter_map(|r| r.id().map(|s| s.into()))
                    .collect();
                Ok(ApiResponse::JsonList(resources))
            }
            _ => Err(ApiError::NotAcceptable(
                "Accept headed could not be satisfied (try application/json)",
            )),
        }
    })
}

#[utoipa::path(
    get,
    path = "/{store_id}/annotations/{annotation_id}",
    params(
        ("store_id" = String, Path, description = "The identifier of the store the annotation is in"),
        ("annotation_id" = String, Path, description = "The identifier of the annotation"),
    ),
    responses(
        (status = 200, description = "The annotation. Several return types are supported via content negotation.",content(
            (apidocs::StamJson = "application/json"),
            (apidocs::WebAnnotation = "application/ld+json"),
            (String = "text/plain"),
        )),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "Returned with name `NotFound` if the store or annotation does not exist", content_type = "application/json"),
        (status = 404, body = apidocs::StamError, description = "Returned when a STAM error occurs", content_type = "application/json"),
    )
)]
/// Returns an annotation given its identifier
async fn get_annotation(
    Path((store_id, annotation_id)): Path<(String, String)>,
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    storepool.map(&store_id, |store| match store.annotation(annotation_id) {
        None => Err(ApiError::NotFound("No such annotation")),
        Some(annotation) => {
            match negotiate_content_type(
                &request,
                &[CONTENT_TYPE_JSON, CONTENT_TYPE_JSONLD, CONTENT_TYPE_TEXT],
            ) {
                Ok(CONTENT_TYPE_JSON) => Ok(ApiResponse::RawJson(
                    annotation.as_ref().to_json_string(store)?,
                )),
                Ok(CONTENT_TYPE_JSONLD) => {
                    if let Ok(webannoconfigs) = storepool.webannoconfigs().read() {
                        if let Some(webannoconfig) = webannoconfigs.get(&store_id) {
                            Ok(ApiResponse::RawJsonLd(
                                annotation.to_webannotation(webannoconfig).to_string(),
                            ))
                        } else {
                            Err(ApiError::InternalError("Webannoconfig must exist"))
                        }
                    } else {
                        Err(ApiError::InternalError("Webannoconfigs lock poisoned"))
                    }
                }
                Ok(CONTENT_TYPE_TEXT) => Ok(ApiResponse::Text(annotation.text_join("\t"))),
                _ => Err(ApiError::NotAcceptable(
                    "Accept headed could not be satisfied (try application/json)",
                )),
            }
        }
    })
}

#[utoipa::path(
    get,
    path = "/{store_id}/resources/{resource_id}",
    params(
        ("store_id" = String, Path, description = "The identifier of the store the resource is in"),
        ("resource_id" = String, Path, description = "The identifier of the resource"),
    ),
    responses(
        (status = 200, description = "The resource. Several return types are supported via content negotation.",content(
            (apidocs::StamJson = "application/json"),
            (String = "text/plain"),
        )),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
        (status = 404, body = apidocs::StamError, description = "Returned when a STAM error occurs", content_type = "application/json"),
    )
)]
/// Returns a text resource given its identifier
async fn get_resource(
    Path((store_id, resource_id)): Path<(String, String)>,
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    storepool.map(&store_id, |store| match store.resource(resource_id) {
        None => Err(ApiError::NotFound("No such resource")),
        Some(resource) => match negotiate_content_type(&request, &[CONTENT_TYPE_TEXT]) {
            Ok(CONTENT_TYPE_TEXT) => Ok(ApiResponse::Text(resource.text().to_string())),
            _ => Err(ApiError::NotAcceptable(
                "Accept headed could not be satisfied (try application/json)",
            )),
        },
    })
}

#[utoipa::path(
    get,
    path = "/{store_id}/resources/{resource_id}/{begin}/{end}",
    params(
        ("store_id" = String, Path, description = "The identifier of the store the resource is in"),
        ("resource_id" = String, Path, description = "The identifier of the resource"),
        ("begin" = isize, Path, description = "An integer indicating the begin offset in unicode points (0-indexed). This may be a negative integer for end-aligned cursors."),
        ("end" = isize, Path, description = "An integer indicating the non-inclusive end offset in unicode points (0-indexed). This may be a negative integer for end-aligned cursors. `-0` is a special value in this context, which means until the very end."),
    ),
    responses(
        (status = 200, description = "The resource. Several return types are supported via content negotation.",content(
            (apidocs::StamJson = "application/json"),
            (String = "text/plain"),
        )),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
        (status = 404, body = apidocs::StamError, description = "Returned when a STAM error occurs, such as invalid offsets.", content_type = "application/json"),
    )
)]
/// Returns an text selection given a resource identifier and an offset
async fn get_textselection(
    Path((store_id, resource_id, begin, end)): Path<(String, String, String, String)>,
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    let offset = Offset::new(begin.as_str().try_into()?, end.as_str().try_into()?);
    storepool.map(&store_id, |store| match store.resource(resource_id) {
        None => Err(ApiError::NotFound("No such resource")),
        Some(resource) => {
            let textselection = resource.textselection(&offset)?;
            match negotiate_content_type(&request, &[CONTENT_TYPE_JSON, CONTENT_TYPE_TEXT]) {
                Ok(CONTENT_TYPE_JSON) => Ok(ApiResponse::RawJson(textselection.to_json_string()?)),
                Ok(CONTENT_TYPE_TEXT) => Ok(ApiResponse::Text(textselection.text().to_string())),
                _ => Err(ApiError::NotAcceptable(
                    "Accept headed could not be satisfied (try application/json)",
                )),
            }
        }
    })
}

fn negotiate_content_type(
    request: &Request<Body>,
    offer_types: &[&'static str],
) -> Result<&'static str, ApiError> {
    if let Some(accept_types) = request.headers().get(axum::http::header::ACCEPT) {
        let mut match_accept_index = None;
        let mut matching_offer = None;
        for (i, accept_type) in accept_types
            .to_str()
            .map_err(|_| ApiError::NotAcceptable("Invalid Accept header"))
            .unwrap_or(CONTENT_TYPE_JSON)
            .split(",")
            .enumerate()
        {
            let accept_type = accept_type.split(";").next().unwrap();
            for offer_type in offer_types.iter() {
                if *offer_type == accept_type || accept_type == "*/*" {
                    if match_accept_index.is_none()
                        || (match_accept_index.is_some() && match_accept_index.unwrap() > i)
                    {
                        match_accept_index = Some(i);
                        matching_offer = Some(*offer_type);
                    }
                }
            }
        }
        if let Some(matching_offer) = matching_offer {
            Ok(matching_offer)
        } else {
            Err(ApiError::NotAcceptable("No matching content type on offer"))
        }
    } else {
        Ok(offer_types[0])
    }
}

fn query_results(
    queryiter: QueryIter,
    request: &Request<Body>,
    use_variable: Option<&str>,
) -> Result<ApiResponse, ApiError> {
    match negotiate_content_type(request, &[CONTENT_TYPE_JSON, CONTENT_TYPE_TEXT]) {
        Ok(CONTENT_TYPE_JSON) => {
            if let Some(use_variable) = use_variable {
                //output only one variable
                let mut ser_results = Vec::new();
                for resultitems in queryiter {
                    if let Ok(result) = resultitems.get_by_name(use_variable) {
                        ser_results.push(result.to_json_value()?);
                    }
                }
                Ok(ApiResponse::JsonList(ser_results))
            } else {
                //output all variables
                let mut ser_results = Vec::new();
                for resultitems in queryiter {
                    let mut responsemap = BTreeMap::new();
                    for (i, (result, name)) in
                        resultitems.iter().zip(resultitems.names()).enumerate()
                    {
                        responsemap.insert(
                            name.map(|s| s.to_string()).unwrap_or(format!("{i}")),
                            result.to_json_value()?,
                        );
                    }
                    ser_results.push(responsemap);
                }
                Ok(ApiResponse::JsonMap(ser_results))
            }
        }
        Ok(CONTENT_TYPE_TEXT) => {
            for (i, resultitems) in queryiter.enumerate() {
                if i > 0 {
                    return Err(ApiError::NotAcceptable(
                        "Plain text can not be returned for queries with multiple results (try application/json instead)",
                    ));
                }
                if let Ok(result) = resultitems.get_by_name_or_first(use_variable) {
                    return Ok(ApiResponse::Text(result.text(Some("\t"))?.to_string()));
                } else {
                    return Err(ApiError::NotFound("No results found"));
                }
            }
            Err(ApiError::NotFound("No results found"))
        }
        _ => Err(ApiError::NotAcceptable(
            "Requested accept type can not be accommodated (try application/json instead)",
        )),
    }
}

impl From<StamError> for ApiError {
    fn from(e: StamError) -> ApiError {
        ApiError::StamError(e)
    }
}
