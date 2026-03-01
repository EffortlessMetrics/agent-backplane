// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_daemon::middleware::{
    CorsConfig, RateLimiter, RequestId, RequestLogger, request_id_middleware,
};
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::middleware;
use axum::routing::get;
use http_body_util::BodyExt;
use std::collections::HashSet;
use std::time::Duration;
use tower::ServiceExt;

/// Helper: minimal router with only the request-id middleware.
fn app_with_request_id() -> Router {
    Router::new()
        .route("/ping", get(|| async { "pong" }))
        .layer(middleware::from_fn(request_id_middleware))
}

/// Helper: router with logger middleware.
fn app_with_logger() -> Router {
    Router::new()
        .route("/ok", get(|| async { "ok" }))
        .route("/not-found", get(|| async { StatusCode::NOT_FOUND }))
        .route(
            "/error",
            get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .layer(middleware::from_fn(RequestLogger::layer))
}

// -----------------------------------------------------------------------
// RequestId tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn request_id_is_generated() {
    let app = app_with_request_id();
    let resp = app
        .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let hdr = resp
        .headers()
        .get("x-request-id")
        .expect("missing x-request-id");
    let parsed: uuid::Uuid = hdr.to_str().unwrap().parse().expect("not a valid uuid");
    assert_ne!(parsed, uuid::Uuid::nil());
}

#[tokio::test]
async fn request_id_is_unique_per_request() {
    let app = app_with_request_id();

    let mut ids = HashSet::new();
    for _ in 0..5 {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let id_str = resp
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        ids.insert(id_str);
    }
    assert_eq!(ids.len(), 5, "all request ids should be unique");
}

#[tokio::test]
async fn request_id_available_as_extension() {
    let app = Router::new()
        .route(
            "/ext",
            get(|ext: axum::Extension<RequestId>| async move { ext.0.0.to_string() }),
        )
        .layer(middleware::from_fn(request_id_middleware));

    let resp = app
        .oneshot(Request::builder().uri("/ext").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    let _parsed: uuid::Uuid = body_str.parse().expect("body should be a uuid");
}

// -----------------------------------------------------------------------
// RequestLogger tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn logger_does_not_panic_on_200() {
    let app = app_with_logger();
    let resp = app
        .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn logger_does_not_panic_on_404() {
    let app = app_with_logger();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/not-found")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logger_does_not_panic_on_500() {
    let app = app_with_logger();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/error")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// -----------------------------------------------------------------------
// RateLimiter tests
// -----------------------------------------------------------------------

fn app_with_rate_limiter(max: u32, window: Duration) -> Router {
    let limiter = RateLimiter::new(max, window);
    Router::new()
        .route("/ping", get(|| async { "pong" }))
        .layer(limiter.into_layer())
}

#[tokio::test]
async fn rate_limiter_allows_requests_within_limit() {
    let app = app_with_rate_limiter(3, Duration::from_secs(60));
    for _ in 0..3 {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn rate_limiter_blocks_over_limit() {
    let app = app_with_rate_limiter(2, Duration::from_secs(60));

    // First two succeed.
    for _ in 0..2 {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Third is rejected.
    let resp = app
        .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn rate_limiter_check_method_works() {
    let limiter = RateLimiter::new(1, Duration::from_secs(60));
    assert!(limiter.check().await.is_ok());
    assert_eq!(
        limiter.check().await.unwrap_err(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

// -----------------------------------------------------------------------
// CorsConfig tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn cors_headers_present() {
    let cors = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".into()],
        allowed_methods: vec!["GET".into(), "POST".into()],
        allowed_headers: vec!["content-type".into()],
    };

    let app = Router::new()
        .route("/ping", get(|| async { "pong" }))
        .layer(cors.to_cors_layer());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/ping")
                .header("origin", "http://localhost:3000")
                .header("access-control-request-method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        resp.headers().contains_key("access-control-allow-origin"),
        "CORS allow-origin header should be present"
    );
}

#[tokio::test]
async fn cors_rejects_disallowed_origin() {
    let cors = CorsConfig {
        allowed_origins: vec!["http://allowed.example".into()],
        allowed_methods: vec!["GET".into()],
        allowed_headers: vec![],
    };

    let app = Router::new()
        .route("/ping", get(|| async { "pong" }))
        .layer(cors.to_cors_layer());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/ping")
                .header("origin", "http://evil.example")
                .header("access-control-request-method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // tower-http CORS layer will not include the allow-origin header for a
    // disallowed origin.
    let hdr = resp.headers().get("access-control-allow-origin");
    assert!(
        hdr.is_none() || hdr.unwrap() != "http://evil.example",
        "disallowed origin should not be echoed"
    );
}

// -----------------------------------------------------------------------
// Composition tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn multiple_middlewares_compose_correctly() {
    let limiter = RateLimiter::new(10, Duration::from_secs(60));
    let cors = CorsConfig {
        allowed_origins: vec!["http://localhost".into()],
        allowed_methods: vec!["GET".into()],
        allowed_headers: vec![],
    };

    let app = Router::new()
        .route("/ping", get(|| async { "pong" }))
        .layer(middleware::from_fn(request_id_middleware))
        .layer(middleware::from_fn(RequestLogger::layer))
        .layer(limiter.into_layer())
        .layer(cors.to_cors_layer());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/ping")
                .header("origin", "http://localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().contains_key("x-request-id"));
}
