// Copyright 2023 Greptime Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod health;
mod heartbeat;
mod leader;
mod maintenance;
mod node_lease;
mod util;

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use tonic::body::BoxBody;
use tonic::codegen::{empty_body, http, BoxFuture, Service};
use tonic::server::NamedService;

use crate::metasrv::Metasrv;

pub fn make_admin_service(metasrv: Arc<Metasrv>) -> Admin {
    let router = Router::new().route("/health", health::HealthHandler);

    let router = router.route(
        "/node-lease",
        node_lease::NodeLeaseHandler {
            meta_peer_client: metasrv.meta_peer_client().clone(),
        },
    );

    let handler = heartbeat::HeartBeatHandler {
        meta_peer_client: metasrv.meta_peer_client().clone(),
    };
    let router = router
        .route("/heartbeat", handler.clone())
        .route("/heartbeat/help", handler);

    let router = router.route(
        "/leader",
        leader::LeaderHandler {
            election: metasrv.election().cloned(),
        },
    );

    let router = router.route(
        "/maintenance",
        maintenance::MaintenanceHandler {
            manager: metasrv.maintenance_mode_manager().clone(),
        },
    );
    let router = Router::nest("/admin", router);

    Admin::new(router)
}

#[async_trait::async_trait]
pub trait HttpHandler: Send + Sync {
    async fn handle(
        &self,
        path: &str,
        method: http::Method,
        params: &HashMap<String, String>,
    ) -> crate::Result<http::Response<String>>;
}

#[derive(Clone)]
pub struct Admin
where
    Self: Send,
{
    router: Arc<Router>,
}

impl Admin {
    pub fn new(router: Router) -> Self {
        Self {
            router: Arc::new(router),
        }
    }
}

impl NamedService for Admin {
    const NAME: &'static str = "admin";
}

impl<T> Service<http::Request<T>> for Admin
where
    T: Send,
{
    type Response = http::Response<BoxBody>;
    type Error = Infallible;
    type Future = BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<T>) -> Self::Future {
        let router = self.router.clone();
        let query_params = req
            .uri()
            .query()
            .map(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .into_owned()
                    .collect()
            })
            .unwrap_or_default();
        let path = req.uri().path().to_owned();
        let method = req.method().clone();
        Box::pin(async move { router.call(&path, method, query_params).await })
    }
}

#[derive(Default)]
pub struct Router {
    handlers: HashMap<String, Box<dyn HttpHandler>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::default(),
        }
    }

    pub fn nest(path: &str, router: Router) -> Self {
        check_path(path);

        let handlers = router
            .handlers
            .into_iter()
            .map(|(url, handler)| (format!("{path}{url}"), handler))
            .collect();

        Self { handlers }
    }

    pub fn route(mut self, path: &str, handler: impl HttpHandler + 'static) -> Self {
        check_path(path);

        let _ = self.handlers.insert(path.to_owned(), Box::new(handler));

        self
    }

    pub async fn call(
        &self,
        path: &str,
        method: http::Method,
        params: HashMap<String, String>,
    ) -> Result<http::Response<BoxBody>, Infallible> {
        let handler = match self.handlers.get(path) {
            Some(handler) => handler,
            None => {
                return Ok(http::Response::builder()
                    .status(http::StatusCode::NOT_FOUND)
                    .body(empty_body())
                    .unwrap())
            }
        };

        let res = match handler.handle(path, method, &params).await {
            Ok(res) => res.map(boxed),
            Err(e) => http::Response::builder()
                .status(http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(boxed(e.to_string()))
                .unwrap(),
        };

        Ok(res)
    }
}

fn check_path(path: &str) {
    if path.is_empty() || !path.starts_with('/') {
        panic!("paths must start with a `/`")
    }
}

/// Returns a [BoxBody] from a string.
/// The implementation follows [empty_body()].
fn boxed(body: String) -> BoxBody {
    Full::new(Bytes::from(body))
        .map_err(|err| match err {})
        .boxed_unsync()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error;

    struct MockOkHandler;

    #[async_trait::async_trait]
    impl HttpHandler for MockOkHandler {
        async fn handle(
            &self,
            _: &str,
            _: http::Method,
            _: &HashMap<String, String>,
        ) -> crate::Result<http::Response<String>> {
            Ok(http::Response::builder()
                .status(http::StatusCode::OK)
                .body("Ok".to_string())
                .unwrap())
        }
    }
    struct MockEmptyKeyErrorHandler;

    #[async_trait::async_trait]
    impl HttpHandler for MockEmptyKeyErrorHandler {
        async fn handle(
            &self,
            _: &str,
            _: http::Method,
            _: &HashMap<String, String>,
        ) -> crate::Result<http::Response<String>> {
            error::EmptyKeySnafu {}.fail()
        }
    }

    #[test]
    fn test_route_nest() {
        let mock_handler = MockOkHandler {};
        let router = Router::new().route("/test_node", mock_handler);
        let router = Router::nest("/test_root", router);

        assert_eq!(1, router.handlers.len());
        assert!(router.handlers.contains_key("/test_root/test_node"));
    }

    #[should_panic]
    #[test]
    fn test_invalid_path() {
        check_path("test_node")
    }

    #[should_panic]
    #[test]
    fn test_empty_path() {
        check_path("")
    }

    #[tokio::test]
    async fn test_route_call_ok() {
        let mock_handler = MockOkHandler {};
        let router = Router::new().route("/test_node", mock_handler);
        let router = Router::nest("/test_root", router);

        let res = router
            .call(
                "/test_root/test_node",
                http::Method::GET,
                HashMap::default(),
            )
            .await
            .unwrap();

        assert!(res.status().is_success());
    }

    #[tokio::test]
    async fn test_route_call_no_handler() {
        let router = Router::new();

        let res = router
            .call(
                "/test_root/test_node",
                http::Method::GET,
                HashMap::default(),
            )
            .await
            .unwrap();

        assert_eq!(http::StatusCode::NOT_FOUND, res.status());
    }

    #[tokio::test]
    async fn test_route_call_err() {
        let mock_handler = MockEmptyKeyErrorHandler {};
        let router = Router::new().route("/test_node", mock_handler);
        let router = Router::nest("/test_root", router);

        let res = router
            .call(
                "/test_root/test_node",
                http::Method::GET,
                HashMap::default(),
            )
            .await
            .unwrap();

        assert_eq!(http::StatusCode::INTERNAL_SERVER_ERROR, res.status());
    }
}
