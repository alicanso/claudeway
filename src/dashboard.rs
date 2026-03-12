use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "dashboard/dist/"]
struct DashboardAssets;

pub async fn serve_dashboard(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches("/dashboard").trim_start_matches('/');

    // Try exact file match first
    if let Some(file) = DashboardAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data.to_vec(),
        ).into_response();
    }

    // SPA fallback: serve index.html for all unmatched routes
    if let Some(file) = DashboardAssets::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html")],
            file.data.to_vec(),
        ).into_response();
    }

    (StatusCode::NOT_FOUND, "Dashboard not found").into_response()
}
