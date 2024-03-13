use std::{fmt::format, net::SocketAddr, string, sync::{atomic::AtomicU16, Arc}, time::Duration};

use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade}, response::{Html, IntoResponse}, routing::get, Router
};
use axum::extract::ws::WebSocket;
use sysinfo::System;
use tokio::sync::broadcast::{self, Sender};
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

struct AppState {
    rc: AtomicU16,
    tx: Sender<Snapshot>,
}

type Snapshot = Vec<f32>;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "htmx_testing=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let (tx, _) = broadcast::channel::<Snapshot>(1);

    let state = Arc::new(AppState {
        rc: AtomicU16::new(0),
        tx: tx.clone(),
    });

    tokio::task::spawn_blocking(move || {
        let mut sys = System::new();
        loop {
            sys.refresh_cpu();
            let v: Vec<_> = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).collect();
            let _ = tx.send(v);
            std::thread::sleep(Duration::from_millis(500));
        }
    });

    let app = Router::new()
        .route("/hw", get(handler))
        .route("/trigger", get(trigger))
        .route("/ws", get(ws_handler))
        .nest_service("/", ServeDir::new("frontend"))
        .with_state(state);

    // run it
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    tracing::info!("listening on {}", listener.local_addr().unwrap());

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    ).await.unwrap();
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {

    tracing::info!("UPGRADING WS Request for: {addr}");
    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    ws.on_upgrade(move |socket| handle_socket(socket, addr, state))
}

/// Actual websocket statemachine (one will be spawned per connection)
async fn handle_socket(mut socket: WebSocket, addr: SocketAddr, state: Arc<AppState>) {
    tracing::info!("UPGRADED WS Request for: {addr}");

    let mut rx = state.tx.subscribe();

    while let Ok(cpus) = rx.recv().await {
        let mut total: f32 = 0.0;
        let mut msg: String = String::new();
        let mut i: usize = 0;

        for core_temp in cpus {
            total += core_temp;
            i += 1;
            msg = msg + &format!("<p>{core_temp} - C{i}</p>\n");
        }

        let avg = total / i as f32;

        // need id to match an element in the frontend
        msg = format!("<div id='cpus'>\n<h1>{avg}</h1>\n{msg}</div>");

        tracing::info!("Sending: \n{msg}");

        socket.send(msg.into())
            .await
            .unwrap();
    }

    socket.send("Goodbye, World!".into()).await.unwrap();
}

async fn handler() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}

async fn trigger(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::info!("Triggered");
    
    // let time = std::time::SystemTime::now()
    //     .duration_since(std::time::UNIX_EPOCH)
    //     .unwrap()
    //     .as_secs();

    // format!("{time}")
    let rc = state.rc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("{rc}")
}