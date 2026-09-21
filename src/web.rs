use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/dist/"]
struct ReleaseAssets;

/// A request for a file rather than a client-side route. Asset URLs carry an
/// extension and the console's own routes never do.
fn looks_like_asset(path: &str) -> bool {
    path.starts_with("assets/")
        || path
            .rsplit('/')
            .next()
            .is_some_and(|last| last.contains('.'))
}

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let found = ReleaseAssets::get(path).map(|asset| (path, asset));
    // Only client-side routes fall back to the shell. Serving it for a missing
    // asset returned HTML with a 200 for a `.js` request, which the browser
    // reports as a syntax error rather than as a missing file — the failure a
    // stale `index.html` produces, and the hardest kind to diagnose.
    let asset = found.or_else(|| {
        (!looks_like_asset(path))
            .then(|| ReleaseAssets::get("index.html").map(|asset| ("index.html", asset)))
            .flatten()
    });
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
        None if looks_like_asset(path) => (
            StatusCode::NOT_FOUND,
            "This asset is not part of the running build",
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Web assets are unavailable; build web/dist",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn embedded_spa_serves_direct_admin_and_auth_routes() {
        for route in [
            "/login",
            "/setup",
            "/channels",
            "/operations/traces/example",
        ] {
            let response = serve(route.parse().unwrap()).await;
            assert_eq!(response.status(), StatusCode::OK, "{route}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
            let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&body).contains("id=\"root\""));
        }
    }

    #[tokio::test]
    async fn a_missing_asset_is_not_answered_with_the_shell() {
        // Returning the shell for a `.js` request produced a 200 of the wrong
        // content type, which the browser reports as a syntax error instead of a
        // missing file. That is what a stale index.html looks like in practice.
        for path in [
            "/assets/index-does-not-exist.js",
            "/assets/index-does-not-exist.css",
            "/logo-not-in-this-build.webp",
        ] {
            let response = serve(path.parse().unwrap()).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
        // A client-side route still gets the shell.
        let response = serve("/channels".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn content_hashed_assets_are_immutable_and_the_shell_is_not() {
        let entry = ReleaseAssets::iter()
            .find(|name| name.starts_with("assets/") && name.ends_with(".js"))
            .expect("web/dist must contain a built entry script");
        let response = serve(format!("/{entry}").parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        let shell = serve("/".parse().unwrap()).await;
        assert_eq!(shell.headers()[header::CACHE_CONTROL], "no-cache");
    }
}
