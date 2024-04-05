//! the web server to manage the link

use axum::{
    extract::Path,
    http::StatusCode,
    response::Html,
    routing::{delete, get},
    Json, Router,
};

use log::info;
use serde::{Deserialize, Serialize};

/// connection information
#[derive(Deserialize, Serialize)]
struct CtlConInfo {
    port: u16,
    src: String,
}

use crate::{config::G_CFG, server::CTL_CONNS};

/// run the web server
pub async fn run() {
    let app = Router::new()
        .route("/", get(root))
        .route("/api/connects", get(get_connects))
        .route("/api/connects/:port", delete(del_connect));

    let port = G_CFG.get().unwrap().web_port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    info!("web server:http://localhost:{}", port);
    axum::serve(listener, app).await.unwrap();
}

/// basic handler that responds with a static string
async fn root() -> Html<String> {
    let str = include_str!("index.html");
    Html(str.to_string())
}

// async fn root() -> Html<String> {
//     let str = std::fs::read_to_string("index.html");
//     return Html(str.unwrap());
// }

/// get all connections
async fn get_connects() -> Json<Vec<CtlConInfo>> {
    let conn = CTL_CONNS.lock().unwrap();
    let conn = conn.as_ref().unwrap();
    let mut ret = Vec::new();
    for (k, v) in conn {
        ret.push(CtlConInfo {
            port: *k,
            src: v.to_string(),
        });
    }
    Json(ret)
}

/// delete a connection
async fn del_connect(Path(port): Path<u16>) -> StatusCode {
    let mut ctl_conns = CTL_CONNS.lock().unwrap();
    let ret = ctl_conns.as_mut().unwrap().remove(&port);
    if ret.is_none() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    }
}
