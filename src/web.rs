use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/dist/"]
struct ReleaseAssets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let asset = ReleaseAssets::get(path)
        .map(|asset| (path, asset))
        .or_else(|| ReleaseAssets::get("index.html").map(|asset| ("index.html", asset)));
    match asset {
        Some((asset_path, asset)) => {
            let content_type = mime_guess::from_path(asset_path)
                .first_or_octet_stream()
                .to_string();
            let cache = if asset_path == "index.html" {
                "no-cache"
            } else {
                "public, max-age=31536000, immutable"
            };
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, cache.into()),
                ],
                Body::from(asset.data.into_owned()),
            )
                .into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Web assets are unavailable; build web/dist",
        )
            .into_response(),
    }
}
