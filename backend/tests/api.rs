//! End-to-end tests over real HTTP, against an in-memory database.

use potoo::{app, auth, db, geo, state::AppState};
use reqwest::{Client, Method, Response};
use serde_json::{Value, json};
use std::sync::Arc;

const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

struct TestApp {
    base: String,
    client: Client,
}

async fn spawn_app() -> TestApp {
    potoo::install_tls_provider();
    let db = db::open(":memory:").await;
    let state = AppState {
        ingest: potoo::ingest::Ingest::spawn(db.clone()).0,
        db,
        geo: Arc::new(geo::Geo::open("/nonexistent/country.mmdb", geo::Source::Disabled).await),
        trust_proxy: true,
        throttle: Arc::new(auth::LoginThrottle::default()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    TestApp {
        base: format!("http://{addr}"),
        client: Client::new(),
    }
}

/// The session cookie of a response.
fn cookie(response: &Response) -> String {
    let set_cookie = response
        .headers()
        .get("set-cookie")
        .expect("a set-cookie header")
        .to_str()
        .unwrap()
        .to_string();
    set_cookie
        .split(';')
        .next()
        .expect("a cookie name and value")
        .to_string()
}

async fn request(
    app: &TestApp,
    method: Method,
    path: &str,
    session: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut builder = app
        .client
        .request(method, format!("{}{path}", app.base))
        .header("user-agent", UA)
        .header("x-forwarded-for", "203.0.113.5");
    if let Some(session) = session {
        builder = builder.header("cookie", session);
    }
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    builder.send().await.unwrap()
}

async fn json(response: Response) -> Value {
    response.json().await.unwrap()
}

#[tokio::test]
async fn setup_users_sites_collection_stats_and_exports() {
    let app = spawn_app().await;
    let admin = json!({ "email": "a@example.com", "password": "correct horse battery" });

    // First run: no users yet, every page redirects to setup.
    let session = json(request(&app, Method::GET, "/api/auth/session", None, None).await).await;
    assert_eq!(session["needsSetup"], json!(true));
    assert_eq!(session["user"], json!(null));

    let response = request(
        &app,
        Method::POST,
        "/api/auth/setup",
        None,
        Some(admin.clone()),
    )
    .await;
    assert_eq!(response.status(), 201);
    let session = cookie(&response);
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/setup",
            None,
            Some(admin.clone())
        )
        .await
        .status(),
        409
    );

    // A wrong password is a 401; the right one a 204 with a new cookie.
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "a@example.com", "password": "wrong password" })),
        )
        .await
        .status(),
        401
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(admin.clone())
        )
        .await
        .status(),
        204
    );

    // Admin creates a regular user, and a site. Hostnames normalize.
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/users",
            Some(&session),
            Some(json!({ "email": "u@example.com", "password": "another long pass" })),
        )
        .await
        .status(),
        201
    );
    assert_eq!(
        request(&app, Method::GET, "/api/sites", None, None)
            .await
            .status(),
        401
    );
    let site = json(
        request(
            &app,
            Method::POST,
            "/api/sites",
            Some(&session),
            Some(json!({ "hostname": "Blog.Example.com" })),
        )
        .await,
    )
    .await;
    assert_eq!(site["hostname"], json!("blog.example.com"));
    let site_id = site["id"].as_i64().unwrap();

    // Collection: a pageview and a click; unknown hosts are ignored, and
    // so are query strings on paths.
    let hit = |body: Value| request(&app, Method::POST, "/api/collect", None, Some(body));
    assert_eq!(
        hit(json!({ "type": "pageview", "host": "blog.example.com", "path": "/a?secret=1" }))
            .await
            .status(),
        204
    );
    assert_eq!(
        hit(json!({ "type": "click", "host": "blog.example.com", "path": "/a", "name": "signup" }))
            .await
            .status(),
        204
    );
    assert_eq!(
        hit(json!({ "type": "pageview", "host": "unknown.com", "path": "/" }))
            .await
            .status(),
        204
    );

    // One site's dashboard: the stats of that site alone.
    let stats = json(
        request(
            &app,
            Method::GET,
            "/api/stats?sites=Blog.Example.com&range=P1D",
            Some(&session),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(stats["pageviews"], json!(1));
    assert_eq!(stats["paths"][0]["path"], json!("/a"));
    assert_eq!(stats["devices"][0]["device"], json!("mobile"));
    assert_eq!(stats["browsers"][0]["browser"], json!("Safari"));
    assert_eq!(stats["os"][0]["os"], json!("iOS"));
    assert_eq!(stats["clicks"][0], json!({ "name": "signup", "clicks": 1 }));
    assert_eq!(stats["days"], json!(1));

    // The site itself: flags are booleans; an unknown site is a 404.
    let site = json(
        request(
            &app,
            Method::GET,
            &format!("/api/sites/{site_id}"),
            Some(&session),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(site["hostname"], json!("blog.example.com"));
    assert_eq!(site["honor_dnt"], json!(true));
    assert_eq!(
        request(&app, Method::GET, "/api/sites/999999", Some(&session), None)
            .await
            .status(),
        404
    );
    assert_eq!(
        request(&app, Method::GET, "/api/sites/1", None, None)
            .await
            .status(),
        401
    );
    // The per-site stats endpoint is gone: /api/stats takes `sites`.
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("/api/sites/{site_id}/stats"),
            Some(&session),
            None
        )
        .await
        .status(),
        404
    );

    // DNT: honored by default, and the admin can turn that off.
    let response = app
        .client
        .request(Method::PATCH, format!("{}/api/sites/{site_id}", app.base))
        .header("cookie", &session)
        .json(&json!({ "honor_dnt": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);

    // The same stats with an RSQL filter, across sites.
    let stats = json(
        request(
            &app,
            Method::GET,
            "/api/stats?sites=blog.example.com&q=path==/a",
            Some(&session),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(stats["pageviews"], json!(1));
    // An unknown hostname is a 400 that names it, and one of several spoils
    // the request; known ones are matched whatever their case.
    let response = request(
        &app,
        Method::GET,
        "/api/stats?sites=blog.example.com,Nope.example.com",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(
        json(response).await["error"],
        json!("Unknown site: nope.example.com")
    );

    // The export: raw rows only, filtered like the dashboard view.
    let export = request(
        &app,
        Method::GET,
        "/api/export?sites=blog.example.com&range=P1D",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(export.status(), 200);
    let csv = export.text().await.unwrap();
    assert!(
        csv.starts_with("site,created_at,kind,path,name,referrer,country,device,browser,os\r\n")
    );
    assert_eq!(csv.lines().count(), 3); // the header, the pageview and the click
    assert!(csv.contains("blog.example.com,"));
    assert!(csv.contains("pageview,/a,,"));
    // The q filter narrows the export to the same rows the stats show.
    let filtered = request(
        &app,
        Method::GET,
        "/api/export?sites=blog.example.com&range=P1D&q=name==signup",
        Some(&session),
        None,
    )
    .await
    .text()
    .await
    .unwrap();
    assert_eq!(filtered.lines().count(), 2); // the header and the click
    assert!(filtered.contains("click,/a,signup"));

    // The theme setting: stored as the JSON the presets send, and the
    // session serves it back; anything malformed is a 400.
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/settings",
            Some(&session),
            Some(json!({ "theme": "{\"base\":\"dark\",\"accent\":\"#38BDF8\"}" })),
        )
        .await
        .status(),
        204
    );
    let with_theme =
        json(request(&app, Method::GET, "/api/auth/session", Some(&session), None).await).await;
    assert_eq!(
        with_theme["user"]["theme"],
        json!("{\"base\":\"dark\",\"accent\":\"#38bdf8\"}")
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/settings",
            Some(&session),
            Some(json!({ "theme": "{\"base\":\"blue\"}" })),
        )
        .await
        .status(),
        400
    );

    // Users: the last admin cannot be demoted or deleted.
    let users =
        json(request(&app, Method::GET, "/api/auth/users", Some(&session), None).await).await;
    let admin_id = users[0]["id"].as_i64().unwrap();
    assert_eq!(users[0]["is_admin"], json!(true));
    assert_eq!(users[1]["is_admin"], json!(false));
    let user_id = users[1]["id"].as_i64().unwrap();
    let response = app
        .client
        .request(
            Method::PATCH,
            format!("{}/api/auth/users/{admin_id}", app.base),
        )
        .header("cookie", &session)
        .json(&json!({ "is_admin": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/auth/users/{admin_id}"),
            Some(&session),
            None
        )
        .await
        .status(),
        409
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/auth/users/{user_id}"),
            Some(&session),
            None
        )
        .await
        .status(),
        204
    );

    // Logout ends the session.
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/logout",
            Some(&session),
            Some(json!({}))
        )
        .await
        .status(),
        204
    );
    assert_eq!(
        request(&app, Method::GET, "/api/sites", Some(&session), None)
            .await
            .status(),
        401
    );
}

#[tokio::test]
async fn pixel_tracker_and_api_404() {
    let app = spawn_app().await;

    // The no-JS pixel: a GIF that records a pageview from its referer.
    let response = app
        .client
        .get(format!("{}/api/pixel?h=example.com&p=/page", app.base))
        .header("user-agent", UA)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("content-type").unwrap(), "image/gif");
    let bytes = response.bytes().await.unwrap();
    assert_eq!(&bytes[..3], b"GIF");

    // The tracker script points its beacons at /api/collect.
    let response = app
        .client
        .get(format!("{}/tracker.js", app.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/javascript")
    );
    let js = response.text().await.unwrap();
    assert!(js.contains("new URL(\"api/collect\", s.src)"));

    // Liveness, for healthchecks: 200 with an OK body.
    let response = request(&app, Method::GET, "/api/health", None, None).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "OK");

    // Unknown API paths answer with JSON, not the frontend.
    let response = request(&app, Method::GET, "/api/nope", None, None).await;
    assert_eq!(response.status(), 404);
    assert_eq!(json(response).await["error"], json!("Not found"));
}

#[tokio::test]
async fn bots_and_dnt_are_not_recorded() {
    let app = spawn_app().await;
    let admin = json!({ "email": "a@example.com", "password": "correct horse battery" });
    let response = request(&app, Method::POST, "/api/auth/setup", None, Some(admin)).await;
    let session = cookie(&response);
    json(
        request(
            &app,
            Method::POST,
            "/api/sites",
            Some(&session),
            Some(json!({ "hostname": "example.com" })),
        )
        .await,
    )
    .await;

    let bot = app
        .client
        .post(format!("{}/api/collect", app.base))
        .header("user-agent", "curl/8.0")
        .json(&json!({ "type": "pageview", "host": "example.com", "path": "/" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bot.status(), 204);
    let dnt = app
        .client
        .post(format!("{}/api/collect", app.base))
        .header("user-agent", UA)
        .header("dnt", "1")
        .json(&json!({ "type": "pageview", "host": "example.com", "path": "/" }))
        .send()
        .await
        .unwrap();
    assert_eq!(dnt.status(), 204);

    let stats = json(
        request(
            &app,
            Method::GET,
            "/api/stats?sites=example.com",
            Some(&session),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(stats["pageviews"], json!(0));
}

/// The spec is made of the routes, so every documented operation must be
/// served: a request to it is never the JSON 404 of an unknown route, and
/// never a 405 (the path exists, but not for that method).
#[tokio::test]
async fn every_documented_operation_is_routed() {
    let app = spawn_app().await;
    let spec: Value = serde_json::from_str(&potoo::openapi::spec_json()).unwrap();
    let mut checked = 0;
    for (path, item) in spec["paths"].as_object().unwrap() {
        for method in item.as_object().unwrap().keys() {
            let url = path.replace("{id}", "1");
            let response = request(
                &app,
                Method::from_bytes(method.to_uppercase().as_bytes()).unwrap(),
                &url,
                None,
                None,
            )
            .await;
            let status = response.status().as_u16();
            assert_ne!(status, 405, "{method} {path} is documented but not routed");
            if !path.contains('{') {
                assert_ne!(status, 404, "{method} {path} is documented but not routed");
            }
            checked += 1;
        }
    }
    assert!(checked >= 23, "only {checked} operations documented");
}
