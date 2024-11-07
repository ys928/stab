//! the web server to manage the link

use axum::{
    extract::Path,
    http::StatusCode,
    response::Html,
    routing::{delete, get},
    Json, Router,
};

use log::{error, info};

use crate::{
    config::G_CFG,
    server::{CtlConInfo, CTL_CONNS},
};

/// run the web server
pub async fn run() {
    let app = Router::new()
        .route("/", get(root))
        .route("/api/connects", get(get_connects))
        .route("/api/connects/:port", delete(del_connect));

    let port = G_CFG.get().unwrap().web_port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await;
    let Ok(listener) = listener else {
        error!("start web server failed: {}", listener.unwrap_err());
        return;
    };
    info!("web server:http://localhost:{}", port);
    axum::serve(listener, app).await.unwrap();
}

/// basic handler that responds with a static string
#[cfg(not(test))]
async fn root() -> Html<String> {
    let str = include_str!("index.html");
    Html(str.to_string())
}

#[cfg(test)]
async fn root() -> Html<String> {
    let str = std::fs::read_to_string("./src/web/index.html");
    return Html(str.unwrap());
}

/// get all connections
async fn get_connects() -> Json<Vec<CtlConInfo>> {
    let conn = CTL_CONNS.get().unwrap().view().await;
    let mut ret = Vec::new();
    for con in conn {
        ret.push(CtlConInfo {
            port: con.port,
            src: con.src.clone(),
            time: con.time.clone(),
            data: con.data,
        });
    }
    Json(ret)
}

/// delete a connection
async fn del_connect(Path(port): Path<u16>) -> StatusCode {
    let ret = CTL_CONNS.get().unwrap().remove(port).await;
    if ret.is_none() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    }
}
