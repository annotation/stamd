use axum::{
    extract::Query,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use clap::Parser;
use stam::{AnnotationStore, Config, StamError};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value_os = "127.0.0.1:8080")]
    bind: String,

    #[arg()]
    annotationstore: String,
}

struct SharedState {
    store: AnnotationStore,
}

#[derive(Debug)]
enum ApiResponse {
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
enum ApiError {
    MissingArgument(&'static str),
    StamError(StamError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        //TODO: return a generic JSON structure for errors
        match self {
            Self::MissingArgument(s) => {
                (StatusCode::NOT_FOUND, format!("Missing argument: {s}")).into_response()
            }
            Self::StamError(e) => {
                (StatusCode::NOT_FOUND, format!("STAM Error: {e}")).into_response()
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let store = match AnnotationStore::from_file(&args.annotationstore, Config::default()) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let shared_state = SharedState { store };

    let app = Router::new()
        .route("/", get(root))
        .route("/query", get(query))
        .with_state(shared_state.into());

    eprintln!("[stamd] listening at {}", args.bind);
    let listener = tokio::net::TcpListener::bind(args.bind).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root(_state: State<Arc<SharedState>>) -> String {
    format!("stamd {}", VERSION)
}

async fn query(
    Query(params): Query<HashMap<String, String>>,
    state: State<Arc<SharedState>>,
) -> Result<ApiResponse, ApiError> {
    if let Some(querystring) = params.get("query") {
        let (query, _) = stam::Query::parse(querystring)?;
        match state.store.query(query) {
            Err(err) => Err(ApiError::StamError(err)),
            Ok(queryiter) => {
                let mut ser_results = Vec::new();
                for resultitems in queryiter {
                    let mut responsemap = BTreeMap::new();
                    for (i, (result, name)) in
                        resultitems.iter().zip(resultitems.names()).enumerate()
                    {
                        responsemap.insert(
                            name.map(|s| s.to_string()).unwrap_or(format!("{i}")),
                            result.to_json_string()?,
                        );
                    }
                    ser_results.push(responsemap);
                }
                Ok(ApiResponse::Results(ser_results))
            }
        }
    } else {
        Err(ApiError::MissingArgument("query"))
    }
}

impl From<StamError> for ApiError {
    fn from(e: StamError) -> ApiError {
        ApiError::StamError(e)
    }
}
