//! the web server to manage the link

use axum::{
    extract::{Path, Request},
    http::StatusCode,
    middleware::{self, Next},
    response::{Html, Response},
    routing::{delete, get},
    Json, Router,
};

use tracing::{error, info};

use crate::{
    config::{hash_key, G_CFG},
    server::{CtlConInfo, CTL_CONNS},
};

/// run the web server
pub async fn run() {
    let app = Router::new()
        .route("/", get(root))
        .route("/api/connects", get(get_connects))
        .route("/api/connects/{port}", delete(del_connect))
        .layer(middleware::from_fn(web_auth));

    let port = G_CFG.get().unwrap().web_port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await;
    let Ok(listener) = listener else {
        error!("start web server failed: {}", listener.unwrap_err());
        return;
    };
    info!("web server:http://localhost:{}", port);
    axum::serve(listener, app).await.unwrap();
}

/// Protect API routes when `web_key` is configured. The HTML shell stays open
/// so the page can prompt for a key.
async fn web_auth(req: Request, next: Next) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if !path.starts_with("/api/") {
        return Ok(next.run(req).await);
    }

    let Some(expected) = G_CFG.get().unwrap().web_key.as_ref() else {
        return Ok(next.run(req).await);
    };

    let provided = req
        .headers()
        .get("x-web-key")
        .and_then(|v| v.to_str().ok())
        .map(hash_key);

    if provided.as_ref() == Some(expected) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Serve the management page.
///
/// Debug builds reload from disk (edit HTML without rebuild).
/// Release builds embed the file at compile time.
async fn root() -> Html<String> {
    #[cfg(debug_assertions)]
    {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/web/index.html");
        Html(std::fs::read_to_string(path).expect("read index.html"))
    }
    #[cfg(not(debug_assertions))]
    {
        Html(include_str!("index.html").to_string())
    }
}

/// get all connections
async fn get_connects() -> Json<Vec<CtlConInfo>> {
    let conn = CTL_CONNS.get().unwrap().view();
    let mut ret = Vec::new();
    for con in conn {
        ret.push(CtlConInfo {
            port: con.port,
            src: con.src.clone(),
            time: con.time.clone(),
            upstream: con.upstream,
            downstream: con.downstream,
            total: con.total,
        });
    }
    Json(ret)
}

/// delete a connection
async fn del_connect(Path(port): Path<u16>) -> StatusCode {
    CTL_CONNS.get().unwrap().remove(port);
    StatusCode::OK
}
