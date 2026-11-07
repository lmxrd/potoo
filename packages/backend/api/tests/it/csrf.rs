//! Requests from other origins.

use crate::common::*;

#[tokio::test]
async fn requests_from_other_origins_are_refused() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;

    // A sibling subdomain (same-site) or another site cannot change
    // anything, signed in or not; the login and logout included.
    for site in ["same-site", "cross-site"] {
        for path in ["/api/sites", "/api/auth/logout", "/api/auth/login"] {
            let response = from_site(&app, path, Some(&admin), site).await;
            assert_eq!(response.status_code(), 403, "{site} {path}");
        }
    }
    assert!(session_of(&app, Some(&admin)).await["user"].is_object());

    // A multipart upload (no CORS preflight) from a sibling subdomain.
    let form = "--b\r\nContent-Disposition: form-data; name=\"file\"\r\n\r\ncreated_at,site,kind,path\n\r\n--b--\r\n";
    let response = app
        .server
        .post("/api/import")
        .add_header("cookie", &admin)
        .add_header("sec-fetch-site", "same-site")
        .add_header("content-type", "multipart/form-data; boundary=b")
        .bytes(form.into())
        .await;
    assert_eq!(response.status_code(), 403);

    // Without Fetch Metadata, the Origin decides.
    let response = app
        .server
        .post("/api/sites")
        .add_header("cookie", &admin)
        .add_header("origin", "https://blog.example.com")
        .json(&json!({ "hostname": "a.example.com" }))
        .await;
    assert_eq!(response.status_code(), 403);

    // Potoo's own pages, the tracker and API keys pass.
    let response = app
        .server
        .post("/api/sites")
        .add_header("cookie", &admin)
        .add_header("sec-fetch-site", "same-origin")
        .add_header("origin", &app.base)
        .json(&json!({ "hostname": "a.example.com" }))
        .await;
    assert_eq!(response.status_code(), 201);
    let response = from_site(&app, "/api/collect", None, "cross-site").await;
    assert_eq!(response.status_code(), 204);
    let key = create_key(&app, &admin, "write").await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let response = app
        .server
        .post("/api/sites")
        .add_header("authorization", format!("Bearer {key}"))
        .add_header("sec-fetch-site", "cross-site")
        .json(&json!({ "hostname": "b.example.com" }))
        .await;
    assert_eq!(response.status_code(), 201);
}
