use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::{OpenApi, ToSchema};

use crate::client::{self, DepError};

pub const SERVICE: &str = "srvcs-pentagonalnumber";
pub const CONCERN: &str = "sequences: nth pentagonal number";
pub const DEPENDS_ON: &[&str] = &["srvcs-multiply", "srvcs-divide"];

/// Dependency endpoints, injected as router state so tests can point them at
/// mock services.
#[derive(Clone)]
pub struct Deps {
    pub multiply_url: String,
    pub divide_url: String,
}

#[derive(Serialize, ToSchema)]
pub struct Info {
    pub service: &'static str,
    pub concern: &'static str,
    pub depends_on: Vec<&'static str>,
}

/// `GET /` — service identity (srvcs service standard).
#[utoipa::path(get, path = "/", responses((status = 200, body = Info)))]
pub async fn index() -> Json<Info> {
    Json(Info {
        service: SERVICE,
        concern: CONCERN,
        depends_on: DEPENDS_ON.to_vec(),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct EvalRequest {
    pub value: i64,
}

#[derive(Serialize, ToSchema)]
pub struct EvalResponse {
    pub value: i64,
    pub result: i64,
}

fn ok(value: i64, result: i64) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "value": value, "result": result })),
    )
        .into_response()
}

fn degraded(dependency: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "dependency unavailable", "dependency": dependency })),
    )
        .into_response()
}

fn forward(status: u16, body: Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (code, Json(body)).into_response()
}

/// A reachable dependency answered `200` but its body lacked an integer
/// `result`. That is a contract violation we cannot recover from, so surface a
/// `500` rather than guessing.
fn malformed(dependency: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(
            json!({ "error": "dependency returned a malformed result", "dependency": dependency }),
        ),
    )
        .into_response()
}

/// Call one dependency at `url` with `body`, mapping its outcome to either the
/// parsed response body (on `200`) or an early-return `Response` the caller
/// should surface verbatim:
///
/// - unreachable / non-`200`/`422` -> `503` degraded
/// - `422` -> forwarded `422` (the dependency rejected the input)
async fn ask(url: &str, body: &Value, dependency: &str) -> Result<Value, Response> {
    match client::call(url, body).await {
        Err(DepError::Unreachable) => Err(degraded(dependency)),
        Ok((200, body)) => Ok(body),
        Ok((422, body)) => Err(forward(422, body)),
        Ok(_) => Err(degraded(dependency)),
    }
}

/// `POST /` — compute the `n`-th pentagonal number `P(n) = n*(3n-1)/2`.
///
/// This service owns the *control flow* but delegates every headline arithmetic
/// step to its dependencies, exactly as specified:
///
/// 1. compute `m = 3*n - 1` locally (index arithmetic);
/// 2. ask `srvcs-multiply` for `p = n * m`;
/// 3. ask `srvcs-divide` for `result = p / 2`.
///
/// If a dependency is unreachable it reports itself degraded (`503`); if a
/// dependency rejects the input it forwards the `422`.
#[utoipa::path(
    post,
    path = "/",
    request_body = EvalRequest,
    responses(
        (status = 200, body = EvalResponse),
        (status = 422, description = "a dependency rejected the input (forwarded)"),
        (status = 500, description = "a dependency returned a malformed result"),
        (status = 503, description = "a dependency is unavailable")
    )
)]
pub async fn evaluate(State(deps): State<Deps>, Json(req): Json<EvalRequest>) -> Response {
    let n = req.value;

    // 1. m = 3*n - 1 (index arithmetic, computed locally when preparing the
    //    request).
    let m = 3 * n - 1;

    // 2. p = n * m (headline product -> srvcs-multiply).
    let multiply_body = match ask(
        &deps.multiply_url,
        &json!({ "a": n, "b": m }),
        "srvcs-multiply",
    )
    .await
    {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let p = match multiply_body.get("result").and_then(Value::as_i64) {
        Some(p) => p,
        None => return malformed("srvcs-multiply"),
    };

    // 3. result = p / 2 (headline division -> srvcs-divide).
    let divide_body = match ask(&deps.divide_url, &json!({ "a": p, "b": 2 }), "srvcs-divide").await
    {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let result = match divide_body.get("result").and_then(Value::as_i64) {
        Some(r) => r,
        None => return malformed("srvcs-divide"),
    };

    ok(n, result)
}

#[derive(OpenApi)]
#[openapi(
    paths(index, evaluate),
    components(schemas(Info, EvalRequest, EvalResponse))
)]
pub struct ApiDoc;

/// Serve OpenAPI document
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_documents_routes() {
        let doc = ApiDoc::openapi();
        let root = doc.paths.paths.get("/").expect("path / present");
        assert!(root.get.is_some());
        assert!(root.post.is_some());
    }

    #[tokio::test]
    async fn index_reports_all_dependencies() {
        let Json(info) = index().await;
        assert_eq!(info.service, "srvcs-pentagonalnumber");
        assert_eq!(info.concern, "sequences: nth pentagonal number");
        assert_eq!(info.depends_on, vec!["srvcs-multiply", "srvcs-divide"]);
    }
}
