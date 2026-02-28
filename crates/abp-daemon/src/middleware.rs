// SPDX-License-Identifier: MIT OR Apache-2.0
//! Middleware stack for the ABP daemon HTTP API.

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tracing::info;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// RequestId middleware
// ---------------------------------------------------------------------------

/// A unique request identifier, available as an Axum extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub Uuid);

/// Axum middleware that generates a [`RequestId`] for each request and sets
/// the `X-Request-Id` response header.
pub async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let id = RequestId(Uuid::new_v4());
    req.extensions_mut().insert(id);
    let mut resp = next.run(req).await;
    resp.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&id.0.to_string()).unwrap(),
    );
    resp
}

// ---------------------------------------------------------------------------
// RequestLogger
// ---------------------------------------------------------------------------

/// Axum middleware that logs method, path, status code, and duration for each
/// request using [`tracing`] structured fields.
pub struct RequestLogger;

impl RequestLogger {
    /// Axum-compatible handler function.
    pub async fn layer(req: Request, next: Next) -> Response {
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let start = Instant::now();

        let resp = next.run(req).await;

        let duration = start.elapsed();
        let status = resp.status().as_u16();

        info!(
            http.method = %method,
            http.path = %path,
            http.status = status,
            http.duration_ms = duration.as_millis() as u64,
            "request completed"
        );

        resp
    }
}

// ---------------------------------------------------------------------------
// RateLimiter
// ---------------------------------------------------------------------------

/// Simple in-memory sliding-window rate limiter.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
    max_requests: u32,
    window: Duration,
}

struct RateLimiterInner {
    timestamps: VecDeque<Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter that allows `max_requests` within `window`.
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                timestamps: VecDeque::new(),
            })),
            max_requests,
            window,
        }
    }

    /// Axum-compatible middleware function.
    ///
    /// Because the limiter carries state we cannot use a bare `async fn`
    /// directly; instead callers should use [`axum::middleware::from_fn`]
    /// together with a closure, or use [`Self::layer`].
    pub async fn check(&self) -> Result<(), StatusCode> {
        let now = Instant::now();
        let mut guard = self.inner.lock().await;

        // Expire timestamps outside the window.
        while let Some(&front) = guard.timestamps.front() {
            if now.duration_since(front) > self.window {
                guard.timestamps.pop_front();
            } else {
                break;
            }
        }

        if guard.timestamps.len() as u32 >= self.max_requests {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        guard.timestamps.push_back(now);
        Ok(())
    }

    /// Create a Tower [`Layer`](tower::Layer) from this rate limiter.
    pub fn into_layer(self) -> RateLimiterLayer {
        RateLimiterLayer(self)
    }
}

/// Tower [`Layer`] that wraps services with [`RateLimiter`] enforcement.
#[derive(Clone)]
pub struct RateLimiterLayer(RateLimiter);

impl<S: Clone> tower::Layer<S> for RateLimiterLayer {
    type Service = RateLimiterService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimiterService {
            limiter: self.0.clone(),
            inner,
        }
    }
}

/// Tower [`Service`] that enforces rate limiting before forwarding to the
/// inner service.
#[derive(Clone)]
pub struct RateLimiterService<S> {
    limiter: RateLimiter,
    inner: S,
}

impl<S> tower::Service<Request<Body>> for RateLimiterService<S>
where
    S: tower::Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: IntoResponse,
{
    type Response = Response;
    type Error = S::Error;
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let limiter = self.limiter.clone();
        let mut inner = self.inner.clone();
        Box::pin(async move {
            if let Err(status) = limiter.check().await {
                return Ok((status, "too many requests").into_response());
            }
            inner.call(req).await
        })
    }
}

// ---------------------------------------------------------------------------
// CorsConfig
// ---------------------------------------------------------------------------

/// Configuration for CORS headers.
#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
}

impl CorsConfig {
    /// Convert this configuration into a [`tower_http::cors::CorsLayer`].
    pub fn to_cors_layer(&self) -> CorsLayer {
        let origins: Vec<HeaderValue> = self
            .allowed_origins
            .iter()
            .filter_map(|o| HeaderValue::from_str(o).ok())
            .collect();

        let methods: Vec<axum::http::Method> = self
            .allowed_methods
            .iter()
            .filter_map(|m| m.parse().ok())
            .collect();

        let headers: Vec<axum::http::HeaderName> = self
            .allowed_headers
            .iter()
            .filter_map(|h| h.parse().ok())
            .collect();

        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(AllowMethods::list(methods))
            .allow_headers(AllowHeaders::list(headers))
    }
}
