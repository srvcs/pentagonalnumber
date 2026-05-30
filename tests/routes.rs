use axum::body::Body;
use axum::extract::Json as AxumJson;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use srvcs_pentagonalnumber::{api::Deps, health, router, telemetry};
use tower::ServiceExt;

const DEAD_URL: &str = "http://127.0.0.1:1";

/// Spawn a *computing* mock `srvcs-multiply`: reads `{"a": x, "b": y}` and
/// returns `{"result": x * y}` — the real product. The pentagonal-number
/// orchestration is genuinely driven by this answer rather than a canned value.
async fn spawn_multiply() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|AxumJson(body): AxumJson<Value>| async move {
            let a = body.get("a").and_then(Value::as_i64).unwrap_or(0);
            let b = body.get("b").and_then(Value::as_i64).unwrap_or(0);
            Json(json!({ "result": a * b }))
        }),
    );
    serve(app).await
}

/// Spawn a *computing* mock `srvcs-divide`: reads `{"a": x, "b": y}` and
/// returns `{"result": x / y}` (integer division), or `422` on divide-by-zero.
async fn spawn_divide() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|AxumJson(body): AxumJson<Value>| async move {
            let a = body.get("a").and_then(Value::as_i64).unwrap_or(0);
            let b = body.get("b").and_then(Value::as_i64).unwrap_or(1);
            if b == 0 {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": "divide by zero" })),
                );
            }
            (StatusCode::OK, Json(json!({ "result": a / b })))
        }),
    );
    serve(app).await
}

/// Spawn a mock returning a fixed status + body (used for error-path tests).
async fn spawn_fixed(status: StatusCode, body: Value) -> String {
    let app = AxumRouter::new().route(
        "/",
        post(move || {
            let body = body.clone();
            async move { (status, Json(body)) }
        }),
    );
    serve(app).await
}

async fn serve(app: AxumRouter) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn app(multiply_url: &str, divide_url: &str) -> axum::Router {
    router(
        telemetry::metrics_handle_for_tests(),
        Deps {
            multiply_url: multiply_url.to_string(),
            divide_url: divide_url.to_string(),
        },
    )
}

async fn pentagonal(multiply_url: &str, divide_url: &str, value: i64) -> (StatusCode, Value) {
    let res = app(multiply_url, divide_url)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "value": value }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn status_of(uri: &str) -> StatusCode {
    app(DEAD_URL, DEAD_URL)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

// --- Standard endpoints. ---

#[tokio::test]
async fn healthz_ok() {
    assert_eq!(status_of("/healthz").await, StatusCode::OK);
}

#[tokio::test]
async fn readyz_reflects_state() {
    health::set_ready(true);
    assert_eq!(status_of("/readyz").await, StatusCode::OK);
}

#[tokio::test]
async fn metrics_ok() {
    assert_eq!(status_of("/metrics").await, StatusCode::OK);
}

#[tokio::test]
async fn openapi_ok() {
    assert_eq!(status_of("/openapi.json").await, StatusCode::OK);
}

#[tokio::test]
async fn generates_request_id_when_absent() {
    let res = app(DEAD_URL, DEAD_URL)
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        res.headers().contains_key("x-request-id"),
        "response must carry a generated x-request-id"
    );
}

#[tokio::test]
async fn index_reports_identity() {
    let res = app(DEAD_URL, DEAD_URL)
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["service"], "srvcs-pentagonalnumber");
    assert_eq!(body["concern"], "sequences: nth pentagonal number");
    assert_eq!(
        body["depends_on"],
        json!(["srvcs-multiply", "srvcs-divide"])
    );
}

// --- Correctness cases, against the computing mocks. ---

#[tokio::test]
async fn pentagonal_5_is_35() {
    let (m, d) = (spawn_multiply().await, spawn_divide().await);
    let (status, body) = pentagonal(&m, &d, 5).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], 5);
    // m = 3*5-1 = 14; 5*14 = 70; 70/2 = 35
    assert_eq!(body["result"], 35);
}

#[tokio::test]
async fn pentagonal_1_is_1() {
    let (m, d) = (spawn_multiply().await, spawn_divide().await);
    let (status, body) = pentagonal(&m, &d, 1).await;
    assert_eq!(status, StatusCode::OK);
    // m = 3*1-1 = 2; 1*2 = 2; 2/2 = 1
    assert_eq!(body["result"], 1);
}

#[tokio::test]
async fn pentagonal_2_is_5() {
    let (m, d) = (spawn_multiply().await, spawn_divide().await);
    let (status, body) = pentagonal(&m, &d, 2).await;
    assert_eq!(status, StatusCode::OK);
    // m = 3*2-1 = 5; 2*5 = 10; 10/2 = 5
    assert_eq!(body["result"], 5);
}

#[tokio::test]
async fn pentagonal_3_is_12() {
    let (m, d) = (spawn_multiply().await, spawn_divide().await);
    let (status, body) = pentagonal(&m, &d, 3).await;
    assert_eq!(status, StatusCode::OK);
    // m = 3*3-1 = 8; 3*8 = 24; 24/2 = 12
    assert_eq!(body["result"], 12);
}

#[tokio::test]
async fn pentagonal_10_is_145() {
    let (m, d) = (spawn_multiply().await, spawn_divide().await);
    let (status, body) = pentagonal(&m, &d, 10).await;
    assert_eq!(status, StatusCode::OK);
    // m = 3*10-1 = 29; 10*29 = 290; 290/2 = 145
    assert_eq!(body["result"], 145);
}

#[tokio::test]
async fn pentagonal_0_is_0() {
    let (m, d) = (spawn_multiply().await, spawn_divide().await);
    let (status, body) = pentagonal(&m, &d, 0).await;
    assert_eq!(status, StatusCode::OK);
    // m = 3*0-1 = -1; 0*-1 = 0; 0/2 = 0
    assert_eq!(body["result"], 0);
}

// --- Error / degraded paths. ---

#[tokio::test]
async fn degrades_when_multiply_unreachable() {
    let d = spawn_divide().await;
    let (status, body) = pentagonal(DEAD_URL, &d, 5).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-multiply");
}

#[tokio::test]
async fn degrades_when_divide_unreachable() {
    // multiply is reachable, so the pipeline reaches the divide call.
    let m = spawn_multiply().await;
    let (status, body) = pentagonal(&m, DEAD_URL, 5).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-divide");
}

#[tokio::test]
async fn forwards_422_from_multiply() {
    let d = spawn_divide().await;
    let m = spawn_fixed(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "error": "value is not an integer" }),
    )
    .await;
    let (status, _) = pentagonal(&m, &d, 5).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn forwards_422_from_divide() {
    // multiply computes a real result so the pipeline reaches divide,
    // which rejects -> forward 422.
    let m = spawn_multiply().await;
    let d = spawn_fixed(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "error": "bad operand" }),
    )
    .await;
    let (status, _) = pentagonal(&m, &d, 5).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn malformed_multiply_result_is_500() {
    // multiply answers 200 but with no integer result -> contract violation -> 500.
    let d = spawn_divide().await;
    let m = spawn_fixed(StatusCode::OK, json!({ "result": "not-a-number" })).await;
    let (status, body) = pentagonal(&m, &d, 5).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["dependency"], "srvcs-multiply");
}

#[tokio::test]
async fn malformed_divide_result_is_500() {
    let m = spawn_multiply().await;
    let d = spawn_fixed(StatusCode::OK, json!({ "result": "not-a-number" })).await;
    let (status, body) = pentagonal(&m, &d, 5).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["dependency"], "srvcs-divide");
}
