use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

/// All health routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(health))
}

#[derive(Serialize, ToSchema)]
pub struct Health {
    /// The service itself: this endpoint answering proves the process is
    /// alive.
    pub status: String,
    /// Database round-trip: `ok` or `unreachable`.
    pub db: String,
}

/// Liveness probe with a database round-trip, for the Docker healthcheck and
/// load balancers. The body reports the two concerns separately; the HTTP
/// status is 200 only when both are healthy (503 when the database is
/// unreachable).
#[utoipa::path(
    get,
    path = "/health",
    tag = "System",
    operation_id = "getHealth",
    summary = "Liveness",
    responses(
        (status = 200, description = "Service and database are healthy", body = Health),
        (status = 503, description = "Database unreachable", body = Health)
    )
)]
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let db_status = match sqlx::query("SELECT 1").execute(state.db.reader()).await {
        Ok(_) => "ok",
        Err(err) => {
            tracing::error!(error = %err, "health check: database unreachable");
            "unreachable"
        }
    };

    let code = if db_status == "ok" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        code,
        Json(Health {
            status: "ok".into(),
            db: db_status.into(),
        }),
    )
}
