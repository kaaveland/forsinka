// Infrastructure concerns: error handling, signals, response types
use crate::api::TrainsPage;
use askama::Template;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tokio::signal;
use tokio::sync::watch::Sender;
use tracing::{error, info};

pub struct WebappError {
    inner: anyhow::Error,
}

impl IntoResponse for WebappError {
    fn into_response(self) -> Response {
        error!("Error: {:?}", self.inner);
        Response::builder()
            .status(500)
            .body("Internal Server Error".into())
            .unwrap()
    }
}

impl<E> From<E> for WebappError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self { inner: err.into() }
    }
}

const TEMPLATE_ERROR_HTML: &str = r#"<!DOCTYPE html>
<html lang="no">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Feil - forsinka</title>
    <style>
        body { font-family: sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .error { background: white; padding: 40px; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); text-align: center; }
        h1 { color: #d32f2f; margin-bottom: 20px; }
        p { color: #666; }
        a { color: #667eea; text-decoration: none; }
    </style>
</head>
<body>
    <div class="error">
        <h1>⚠️ En ukjent feil oppsto</h1>
        <p>Beklager, vi kunne ikke vise siden.</p>
        <p><a href="/trains">Prøv JSON API</a> eller <a href="/">tilbake til forsiden</a></p>
    </div>
</body>
</html>"#;

impl IntoResponse for TrainsPage {
    fn into_response(self) -> Response {
        if let Ok(html) = self.render() {
            axum::response::Html(html).into_response()
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::response::Html(TEMPLATE_ERROR_HTML),
            )
                .into_response()
        }
    }
}

pub async fn shutdown_signal(terminate_jobs: Sender<bool>) {
    let interrupt = async {
        signal::ctrl_c()
            .await
            .expect("Unable to set signal handler for Ctrl+C");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => {
            info!("Received Ctrl+C signal");
            terminate_jobs.send(true).ok();
        },
        _ = terminate => {
            info!("Received terminate signal");
            terminate_jobs.send(true).ok();
        },
    }
}
