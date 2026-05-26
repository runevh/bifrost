use axum::Router;
use axum::extract::{Path, State};
use axum::routing::post;

use bifrost_api::config::Z2mServer;

use crate::backend::default::DefaultBackend;
use crate::backend::z2m::Z2mBackend;
use crate::routes::bifrost::BifrostApiResult;
use crate::routes::extractor::Json;
use crate::server::appstate::AppState;

#[axum::debug_handler]
async fn post_backend_z2m(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(server): Json<Z2mServer>,
) -> BifrostApiResult<Json<()>> {
    log::info!("Adding new z2m backend: {name:?}");

    let mut mgr = state.manager();

    let svc = Z2mBackend::new(name.clone(), server, state.config(), state.res.clone())?;
    let name = format!("z2m-{name}");

    mgr.register_service(&name, svc).await?;
    mgr.start(&name).await?;

    Ok(Json(()))
}

#[axum::debug_handler]
async fn post_backend_default(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> BifrostApiResult<Json<()>> {
    log::info!("Adding new default backend: {name:?}");

    let mut mgr = state.manager();

    let svc = DefaultBackend::new(name.clone(), state.clone());
    let name = format!("default-{name}");

    mgr.register_service(&name, svc).await?;
    mgr.start(&name).await?;

    Ok(Json(()))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/z2m/{name}", post(post_backend_z2m))
        .route("/default/{name}", post(post_backend_default))
}
