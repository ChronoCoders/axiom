#![deny(warnings)]

use axiom_api::{app_router, AppState};
use axiom_mempool::Mempool;
use axiom_primitives::StateHash;
use axiom_state::State;
use axiom_storage::Storage;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tower::Service;
use tower::ServiceExt;

// Helper to setup the app with dummy storage
async fn setup_app() -> (
    axum::Router,
    tempfile::TempDir,
    Arc<Storage>,
    Arc<Mutex<Mempool>>,
) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("axiom.db");

    let storage = Storage::initialize(db_path.to_str().unwrap()).unwrap();

    // Create dummy genesis state
    let state = State {
        total_supply: 0,
        block_reward: 10,
        accounts: BTreeMap::new(),
        validators: BTreeMap::new(),
    };

    // We need a dummy genesis hash
    let genesis_hash = StateHash([0u8; 32]);
    storage.store_genesis(&state, &genesis_hash).unwrap();

    let mempool = Mempool::new(100);
    let mempool_arc = Arc::new(Mutex::new(mempool));
    let storage_arc = Arc::new(storage);

    let app_state = Arc::new(AppState {
        storage: storage_arc.clone(),
        mempool: mempool_arc.clone(),
        peers: Arc::new(Mutex::new(HashMap::new())),
        auth_tokens: Arc::new(RwLock::new(HashMap::new())),
        console_user: "operator".to_string(),
        console_pass: "axiom".to_string(),
        max_tx_bytes: 65536,
    });

    (
        app_router(app_state, PathBuf::from("web")),
        dir,
        storage_arc,
        mempool_arc,
    )
}

#[tokio::test]
async fn test_health_live() {
    let (app, _dir, _, _) = setup_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_health_ready_ok() {
    let (app, _dir, _, _) = setup_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(start_paused = true)]
async fn test_rate_limiting() {
    let (mut app, _dir, _, _) = setup_app().await;

    // Rate limit is 100 req/sec
    // We send 100 requests, they should all succeed
    for _ in 0..100 {
        let req = Request::builder()
            .uri("/health/live")
            .body(Body::empty())
            .unwrap();
        // Clone the request service handle? No, we call the mutable app.
        // We need to poll the future.
        let response = app.call(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // The 101st request should be rate limited
    let req = Request::builder()
        .uri("/health/live")
        .body(Body::empty())
        .unwrap();
    let response = app.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_request_timeout() {
    // Since we cannot easily inject a 10s delay in the real app_router without modifying production code,
    // we verify the TimeoutLayer mechanism using a similar stack.
    // This ensures the library we rely on behaves as expected.

    use axum::{routing::get, Router};
    use std::time::Duration;
    use tower_http::timeout::TimeoutLayer;

    let app = Router::new()
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                "done"
            }),
        )
        .layer(TimeoutLayer::new(Duration::from_millis(100))); // Timeout 100ms

    let req = Request::builder().uri("/slow").body(Body::empty()).unwrap();
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn test_v2_api_endpoints_exist() {
    let (app, _dir, _, _) = setup_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/staking")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/consensus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_body_limit_rejects_large_payload() {
    let (app, _dir, _, _) = setup_app().await;

    let big = vec![b'a'; 400_000];
    let req = Request::builder()
        .method("POST")
        .uri("/api/transactions")
        .header("content-type", "application/json")
        .body(Body::from(big))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
