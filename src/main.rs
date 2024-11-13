use axum::{
    body::Body, extract::Path, extract::Query, extract::State, http::HeaderValue, http::Request,
    response::Html, routing::get, Router,
};
use clap::Parser;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, Level};

use stam::{Config, QueryIter, StamError};
use stamtools::view::HtmlWriter;

mod common;
mod multistore;
use common::{ApiError, ApiResponse};
use multistore::StorePool;

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const CONTENT_TYPE_JSON: &'static str = "application/json";
const CONTENT_TYPE_HTML: &'static str = "text/html";

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
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let storepool = StorePool::new(
        args.basedir,
        args.extension,
        args.readonly,
        args.unload_time,
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
        .route("/", get(root))
        .route("/query/:store_id/", get(query))
        .layer(TraceLayer::new_for_http())
        .with_state(storepool.clone());

    eprintln!("[stamd] listening on {}", args.bind);
    let listener = tokio::net::TcpListener::bind(args.bind).await.unwrap();
    axum::serve(listener, app)
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

async fn root(_state: State<Arc<StorePool>>) -> String {
    format!("stamd {}", VERSION)
}

async fn query(
    Path(store_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    storepool: State<Arc<StorePool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    if let Some(querystring) = params.get("query") {
        let (query, _) = stam::Query::parse(querystring)?;
        if let Ok(CONTENT_TYPE_HTML) =
            negotiate_content_type(&request, &[CONTENT_TYPE_JSON, CONTENT_TYPE_HTML])
        {
            storepool.map(&store_id, |store| {
                let htmlwriter = HtmlWriter::new(&store, query, None)
                    .map_err(|e| ApiError::CustomNotFound(e))?;
                Ok(ApiResponse::Html(htmlwriter.to_string()))
            })
        } else if query.querytype().readonly() {
            storepool.map(&store_id, |store| match store.query(query) {
                Err(err) => Err(ApiError::StamError(err)),
                Ok(queryiter) => query_results(queryiter, &request),
            })
        } else {
            storepool.map_mut(&store_id, |store| match store.query_mut(query) {
                Err(err) => Err(ApiError::StamError(err)),
                Ok(queryiter) => query_results(queryiter, &request),
            })
        }
    } else {
        Err(ApiError::MissingArgument("query"))
    }
}

fn negotiate_content_type(
    request: &Request<Body>,
    offer_types: &[&'static str],
) -> Result<&'static str, ApiError> {
    if let Some(accept_types) = request.headers().get(axum::http::header::ACCEPT) {
        let mut matching_offer = None;
        for accept_type in accept_types
            .to_str()
            .map_err(|_| ApiError::NotAcceptable("Invalid Accept header"))
            .unwrap_or(CONTENT_TYPE_JSON)
            .split(",")
        {
            let accept_type = accept_type.split(";").next().unwrap();
            for (i, offer_type) in offer_types.iter().enumerate() {
                if *offer_type == accept_type || accept_type == "*/*" {
                    if matching_offer.is_none()
                        || (matching_offer.is_some() && matching_offer.unwrap() > i)
                    {
                        matching_offer = Some(i);
                    }
                }
            }
        }
        if let Some(matching_offer) = matching_offer {
            Ok(offer_types[matching_offer])
        } else {
            Err(ApiError::NotAcceptable("No matching content type on offer"))
        }
    } else {
        Ok(offer_types[0])
    }
}

fn query_results(queryiter: QueryIter, request: &Request<Body>) -> Result<ApiResponse, ApiError> {
    match negotiate_content_type(request, &[CONTENT_TYPE_JSON]) {
        Ok(CONTENT_TYPE_JSON) => {
            let mut ser_results = Vec::new();
            for resultitems in queryiter {
                let mut responsemap = BTreeMap::new();
                for (i, (result, name)) in resultitems.iter().zip(resultitems.names()).enumerate() {
                    responsemap.insert(
                        name.map(|s| s.to_string()).unwrap_or(format!("{i}")),
                        result.to_json_string()?,
                    );
                }
                ser_results.push(responsemap);
            }
            Ok(ApiResponse::Results(ser_results))
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
