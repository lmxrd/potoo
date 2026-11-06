//! Integration tests: the app on axum-test's TestServer, over a real local
//! socket (the app reads the client's address from the connection), against
//! an in-memory database.

use axum::http::Method;
use axum_test::{TestResponse as Response, TestServer};
use potoo::{app, db, geo, oidc, sessions, state::AppState};
use serde_json::{Value, json};
use std::sync::Arc;

const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

struct TestApp {
    server: TestServer,
    /// Where the server listens, e.g. `http://127.0.0.1:41234`.
    base: String,
}

async fn spawn_app() -> TestApp {
    spawn_app_with(oidc::Env::default()).await
}

/// The app, with OIDC_* environment variables as given.
async fn spawn_app_with(oidc_env: oidc::Env) -> TestApp {
    spawn_app_full(oidc_env, false).await
}

/// The app, with OIDC_* environment variables and TOTP_REQUIRED as given.
async fn spawn_app_full(oidc_env: oidc::Env, totp_required: bool) -> TestApp {
    spawn(Options {
        oidc: oidc_env,
        totp_required,
        ..Options::default()
    })
    .await
}

/// What a test app is started with.
struct Options {
    oidc: oidc::Env,
    totp_required: bool,
    /// `PUBLIC_URL`; https makes every cookie Secure and `__Host-`.
    public_url: Option<String>,
    /// The login limits: large ones, but for the tests of the limits.
    limits: potoo::limits::Limits,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            oidc: oidc::Env::default(),
            totp_required: false,
            public_url: None,
            limits: potoo::limits::Limits::generous(),
        }
    }
}

async fn spawn(options: Options) -> TestApp {
    potoo::install_tls_provider();
    let db = db::open(":memory:").await;
    let state = AppState {
        ingest: potoo::ingest::Ingest::spawn(db.clone()).0,
        oidc: Arc::new(oidc::Oidc::load(&db, options.oidc).await),
        db,
        geo: Arc::new(geo::Geo::open("/nonexistent/country.mmdb", geo::Source::Disabled).await),
        trust_proxy: true,
        limits: Arc::new(options.limits),
        secure_cookies: options
            .public_url
            .as_deref()
            .is_some_and(|u| u.starts_with("https://")),
        public_url: options.public_url,
        challenge_key: axum_extra::extract::cookie::Key::generate(),
        totp_required: options.totp_required,
        swagger_ui: false,
    };
    let server = TestServer::builder()
        .http_transport()
        .build(app(state).into_make_service_with_connect_info::<std::net::SocketAddr>());
    let base = server
        .server_address()
        .expect("an http transport")
        .to_string()
        .trim_end_matches('/')
        .to_string();
    TestApp { server, base }
}

/// Now, give or take some hours, as the API takes times.
fn iso_from_now(hours: i64) -> String {
    use jiff::ToSpan;
    format!("{:.3}", jiff::Timestamp::now() + hours.hours())
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
        .server
        .method(method, path)
        .add_header("user-agent", UA)
        .add_header("x-forwarded-for", "203.0.113.5");
    if let Some(session) = session {
        builder = builder.add_header("cookie", session);
    }
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    builder.await
}

async fn json(response: Response) -> Value {
    response.json()
}

#[tokio::test]
async fn setup_users_sites_collection_stats_and_exports() {
    let app = spawn_app().await;
    let admin = json!({ "email": "a@example.com", "password": "correct horse battery" });

    // First run: no users yet, every page redirects to setup.
    let session = session_of(&app, None).await;
    assert_eq!(session["needsSetup"], json!(true));
    assert_eq!(session["user"], json!(null));

    let response = request(
        &app,
        Method::POST,
        "/api/auth/bootstrap",
        None,
        Some(admin.clone()),
    )
    .await;
    assert_eq!(response.status_code(), 201);
    let session = cookie(&response);
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/auth/bootstrap",
            None,
            Some(admin.clone())
        )
        .await
        .status_code(),
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
        .status_code(),
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
        .status_code(),
        204
    );

    // Admin creates a regular user, and a site. Hostnames normalize.
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/admin/users",
            Some(&session),
            Some(json!({ "email": "u@example.com", "password": "another long pass" })),
        )
        .await
        .status_code(),
        201
    );
    assert_eq!(
        request(&app, Method::GET, "/api/sites", None, None)
            .await
            .status_code(),
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
    let site_id = site["id"].as_str().unwrap().to_string();
    // The window of the stats and the exports: twelve hours either side
    // of now, so it covers the hits this test collects.
    let window = (iso_from_now(-12), iso_from_now(12));

    // Collection: a pageview and a click; unknown hosts are ignored, and
    // so are query strings on paths.
    let hit = |body: Value| request(&app, Method::POST, "/api/collect", None, Some(body));
    assert_eq!(
        hit(json!({ "type": "pageview", "host": "blog.example.com", "path": "/a?secret=1" }))
            .await
            .status_code(),
        204
    );
    assert_eq!(
        hit(json!({ "type": "click", "host": "blog.example.com", "path": "/a", "name": "signup" }))
            .await
            .status_code(),
        204
    );
    assert_eq!(
        hit(json!({ "type": "pageview", "host": "unknown.com", "path": "/" }))
            .await
            .status_code(),
        204
    );

    // One site's dashboard: the stats of that site alone.
    let stats = json(
        request(
            &app,
            Method::GET,
            &format!(
                "/api/stats?sites=Blog.Example.com&from={}&to={}",
                window.0, window.1
            ),
            Some(&session),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(stats["pageviews"], json!(1));
    assert_eq!(stats["paths"][0]["path"], json!("/a"));
    assert_eq!(stats["devices"][0]["device"], json!("mobile"));
    assert_eq!(stats["browsers"][0]["browser"], json!("safari"));
    assert_eq!(stats["os"][0]["os"], json!("ios"));
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
        request(
            &app,
            Method::GET,
            "/api/sites/0190f5b2-7c00-7000-8000-000000000000",
            Some(&session),
            None
        )
        .await
        .status_code(),
        404
    );
    assert_eq!(
        request(&app, Method::GET, "/api/sites/1", None, None)
            .await
            .status_code(),
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
        .status_code(),
        404
    );

    // DNT: honored by default, and the admin can turn that off.
    let response = app
        .server
        .method(Method::PATCH, &format!("/api/sites/{site_id}"))
        .add_header("cookie", &session)
        .json(&json!({ "honor_dnt": false }))
        .await;
    assert_eq!(response.status_code(), 204);

    // The same stats with an RSQL filter, across sites.
    let stats = json(
        request(
            &app,
            Method::GET,
            &format!(
                "/api/stats?sites=blog.example.com&from={}&to={}&q=path==/a",
                window.0, window.1
            ),
            Some(&session),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(stats["pageviews"], json!(1));
    // The facets with a fixed vocabulary are stored lowercase, and a filter
    // on them does not care about case: `Safari` and `iOS` find `safari` and
    // `ios`. A path is case sensitive: `/A` is not `/a`.
    assert_eq!(
        pageviews(&app, &session, &window, "browser==Safari;os==iOS").await,
        json!(1)
    );
    assert_eq!(
        pageviews(&app, &session, &window, "browser==SAFARI,browser==Chrome").await,
        json!(1)
    );
    assert_eq!(
        pageviews(&app, &session, &window, "browser==chrome").await,
        json!(0)
    );
    assert_eq!(
        pageviews(&app, &session, &window, "path==/A").await,
        json!(0)
    );
    // An unknown hostname is a 400 that names it, and one of several spoils
    // the request; known ones are matched whatever their case.
    let response = request(
        &app,
        Method::GET,
        &format!(
            "/api/stats?sites=blog.example.com,Nope.example.com&from={}&to={}",
            window.0, window.1
        ),
        Some(&session),
        None,
    )
    .await;
    assert_eq!(response.status_code(), 400);
    assert_eq!(
        json(response).await["error"],
        json!("Unknown site: nope.example.com")
    );

    // The export: raw rows only, filtered like the dashboard view.
    let export = request(
        &app,
        Method::GET,
        &format!(
            "/api/export?sites=blog.example.com&from={}&to={}",
            window.0, window.1
        ),
        Some(&session),
        None,
    )
    .await;
    assert_eq!(export.status_code(), 200);
    let csv = export.text();
    assert!(csv.starts_with(
        "site,created_at,kind,path,name,referrer,country,device,browser,os,visitor\r\n"
    ));
    assert_eq!(csv.lines().count(), 3); // the header, the pageview and the click
    assert!(csv.contains("blog.example.com,"));
    assert!(csv.contains("pageview,/a,,"));
    // The q filter narrows the export to the same rows the stats show.
    let filtered = request(
        &app,
        Method::GET,
        &format!(
            "/api/export?sites=blog.example.com&from={}&to={}&q=name==signup",
            window.0, window.1
        ),
        Some(&session),
        None,
    )
    .await
    .text();
    assert_eq!(filtered.lines().count(), 2); // the header and the click
    assert!(filtered.contains("click,/a,signup"));

    // The theme setting: stored as the JSON the presets send, and the
    // session serves it back; anything malformed is a 400.
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/auth/me",
            Some(&session),
            Some(json!({ "theme": "{\"base\":\"dark\",\"accent\":\"#38BDF8\"}" })),
        )
        .await
        .status_code(),
        200
    );
    let with_theme = session_of(&app, Some(&session)).await;
    assert_eq!(
        with_theme["user"]["theme"],
        json!("{\"base\":\"dark\",\"accent\":\"#38bdf8\"}")
    );
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/auth/me",
            Some(&session),
            Some(json!({ "theme": "{\"base\":\"blue\"}" })),
        )
        .await
        .status_code(),
        400
    );

    // Users: the last admin cannot be demoted or deleted.
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&session), None).await).await;
    let admin_id = users[0]["id"].as_str().unwrap().to_string();
    assert_eq!(users[0]["is_admin"], json!(true));
    assert_eq!(users[1]["is_admin"], json!(false));
    let user_id = users[1]["id"].as_str().unwrap().to_string();
    let response = app
        .server
        .method(Method::PATCH, &format!("/api/admin/users/{admin_id}"))
        .add_header("cookie", &session)
        .json(&json!({ "is_admin": false }))
        .await;
    assert_eq!(response.status_code(), 409);
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/admin/users/{admin_id}"),
            Some(&session),
            None
        )
        .await
        .status_code(),
        409
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/admin/users/{user_id}"),
            Some(&session),
            None
        )
        .await
        .status_code(),
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
        .status_code(),
        204
    );
    assert_eq!(
        request(&app, Method::GET, "/api/sites", Some(&session), None)
            .await
            .status_code(),
        401
    );
}

#[tokio::test]
async fn pixel_tracker_and_api_404() {
    let app = spawn_app().await;

    // The no-JS pixel: a GIF that records a pageview from its referer.
    let response = app
        .server
        .get("/api/pixel?h=example.com&p=/page")
        .add_header("user-agent", UA)
        .await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.headers().get("content-type").unwrap(), "image/gif");
    let bytes = response.into_bytes();
    assert_eq!(&bytes[..3], b"GIF");

    // The tracker script points its beacons at /api/collect.
    let response = app.server.get("/tracker.js").await;
    assert_eq!(response.status_code(), 200);
    assert!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/javascript")
    );
    let js = response.text();
    assert!(js.contains("new URL(\"api/collect\", s.src)"));

    // Liveness, for healthchecks: 200 when the service and its database are up.
    let response = request(&app, Method::GET, "/api/health", None, None).await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(json(response).await, json!({ "status": "ok", "db": "ok" }));

    // Unknown API paths answer with JSON, not the frontend.
    let response = request(&app, Method::GET, "/api/nope", None, None).await;
    assert_eq!(response.status_code(), 404);
    assert_eq!(json(response).await["error"], json!("Not found"));
}

#[tokio::test]
async fn bots_and_dnt_are_not_recorded() {
    let app = spawn_app().await;
    let admin = json!({ "email": "a@example.com", "password": "correct horse battery" });
    let response = request(&app, Method::POST, "/api/auth/bootstrap", None, Some(admin)).await;
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
        .server
        .post("/api/collect")
        .add_header("user-agent", "curl/8.0")
        .json(&json!({ "type": "pageview", "host": "example.com", "path": "/" }))
        .await;
    assert_eq!(bot.status_code(), 204);
    let dnt = app
        .server
        .post("/api/collect")
        .add_header("user-agent", UA)
        .add_header("dnt", "1")
        .json(&json!({ "type": "pageview", "host": "example.com", "path": "/" }))
        .await;
    assert_eq!(dnt.status_code(), 204);

    let window = (iso_from_now(-12), iso_from_now(12));
    let stats = json(
        request(
            &app,
            Method::GET,
            &format!(
                "/api/stats?sites=example.com&from={}&to={}",
                window.0, window.1
            ),
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
            let status = response.status_code().as_u16();
            assert_ne!(status, 405, "{method} {path} is documented but not routed");
            if !path.contains('{') {
                assert_ne!(status, 404, "{method} {path} is documented but not routed");
            }
            checked += 1;
        }
    }
    assert!(checked >= 28, "only {checked} operations documented");
}

/// The pageviews of the test site in a window, under a filter.
async fn pageviews(app: &TestApp, session: &str, window: &(String, String), q: &str) -> Value {
    let url = format!(
        "/api/stats?sites=blog.example.com&from={}&to={}&q={q}",
        window.0, window.1
    );
    json(request(app, Method::GET, &url, Some(session), None).await).await["pageviews"].clone()
}

async fn post_raw(app: &TestApp, path: &str, content_type: &str, body: &str) -> Response {
    app.server
        .post(path)
        .add_header("user-agent", UA)
        .add_header("content-type", content_type)
        .bytes(body.to_string().into())
        .await
}

/// Bodies are JSON and nothing else, except at the tracker's endpoint,
/// which takes the `text/plain` that `sendBeacon` sends too.
#[tokio::test]
async fn bodies_are_json_only_except_at_collect() {
    let app = spawn_app().await;
    let credentials = r#"{"email":"a@example.com","password":"correct horse battery"}"#;

    // Valid JSON in another content type is a 415, in the API's error shape.
    let response = post_raw(&app, "/api/auth/bootstrap", "text/plain", credentials).await;
    assert_eq!(response.status_code(), 415);
    assert_eq!(
        json(response).await["error"],
        json!("Expected a JSON body (Content-Type: application/json)")
    );
    // Invalid JSON, or JSON of the wrong shape, is a 400.
    for body in ["{nope", r#"{"email": 1}"#, ""] {
        let response = post_raw(&app, "/api/auth/bootstrap", "application/json", body).await;
        assert_eq!(response.status_code(), 400, "{body:?}");
        assert_eq!(json(response).await["error"], json!("Invalid request body"));
    }
    // Nothing was created by any of those; the real thing works.
    let response = post_raw(&app, "/api/auth/bootstrap", "application/json", credentials).await;
    assert_eq!(response.status_code(), 201);

    // A login with a body the API cannot read fails like a wrong password.
    for (content_type, body) in [("text/plain", credentials), ("application/json", "{nope")] {
        let response = post_raw(&app, "/api/auth/login", content_type, body).await;
        assert_eq!(response.status_code(), 401, "{content_type} {body}");
    }

    // The tracker's endpoint takes text/plain and JSON, and answers 204 to
    // anything, garbage included.
    let hit = r#"{"type":"pageview","host":"nobody.example.com","path":"/"}"#;
    for (content_type, body) in [
        ("text/plain", hit),
        ("application/json", hit),
        ("text/plain", "garbage"),
    ] {
        let response = post_raw(&app, "/api/collect", content_type, body).await;
        assert_eq!(response.status_code(), 204, "{content_type} {body}");
    }
}

async fn post_csv(app: &TestApp, session: &str, content_type: &str, body: &str) -> Response {
    app.server
        .post("/api/import")
        .add_header("cookie", session)
        .add_header("content-type", content_type)
        .bytes(body.to_string().into())
        .await
}

/// The import: admin only, all or nothing, and what it adds is cleaned like
/// a tracked hit and has no visitor.
#[tokio::test]
async fn import_adds_every_row_or_none() {
    let app = spawn_app().await;
    let credentials = json!({ "email": "a@example.com", "password": "correct horse battery" });
    let session = cookie(
        &request(
            &app,
            Method::POST,
            "/api/auth/bootstrap",
            None,
            Some(credentials),
        )
        .await,
    );
    request(
        &app,
        Method::POST,
        "/api/sites",
        Some(&session),
        Some(json!({ "hostname": "blog.example.com" })),
    )
    .await;
    let header = "site,created_at,kind,path,name,referrer,country,device,browser,os\r\n";
    let good = format!(
        "{header}Blog.Example.com,2026-03-04T12:00:00.000Z,pageview,/a?x=1,,https://news.example.org/post,NO,Desktop,Chrome,macOS\r\n\
         blog.example.com,2026-03-04T14:00:00+02:00,click,/a,signup,,,mobile,safari,ios\r\n\
         blog.example.com,2026-03-04T15:00:00Z,pageview,/c,,,,,,\r\n"
    );
    let fetch_stats = || async {
        json(
            request(
                &app,
                Method::GET,
                "/api/stats?sites=blog.example.com&from=2026-03-01T00:00:00.000Z&to=2026-03-10T00:00:00.000Z",
                Some(&session),
                None,
            )
            .await,
        )
        .await
    };

    // Not signed in, not an admin, wrong type: refused before anything is read.
    request(
        &app,
        Method::POST,
        "/api/admin/users",
        Some(&session),
        Some(json!({ "email": "u@example.com", "password": "another long pass" })),
    )
    .await;
    let member = cookie(
        &request(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "u@example.com", "password": "another long pass" })),
        )
        .await,
    );
    assert_eq!(
        post_csv(&app, &member, "text/csv", &good)
            .await
            .status_code(),
        403
    );
    assert_eq!(
        post_csv(&app, "", "text/csv", &good).await.status_code(),
        401
    );
    assert_eq!(
        post_csv(&app, &session, "application/json", &good)
            .await
            .status_code(),
        415
    );

    // Unknown sites (once each), a bad date and a bad kind: nothing is added
    // and every problem is said, with its line.
    let bad = format!(
        "{header}nope.com,2026-03-04T12:00:00Z,pageview,/,,,,desktop,chrome,macos\n\
         nope.com,2026-03-04T12:00:00Z,pageview,/,,,,desktop,chrome,macos\n\
         blog.example.com,yesterday,pageview,/,,,,desktop,chrome,macos\n\
         blog.example.com,2026-03-04T12:00:00Z,view,/,,,,desktop,chrome,macos\n"
    );
    let response = post_csv(
        &app,
        &session,
        "text/csv",
        &format!("{good}{}", &bad[header.len()..]),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    let body = json(response).await;
    assert_eq!(body["total_problems"], json!(3));
    let problems = body["problems"].as_array().unwrap();
    assert_eq!(problems[0]["line"], json!(5));
    assert_eq!(problems[0]["column"], json!("site"));
    assert_eq!(problems[1]["column"], json!("created_at"));
    assert_eq!(problems[2]["column"], json!("kind"));
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Nothing was imported. line 5:")
    );
    assert_eq!(
        fetch_stats().await["pageviews"],
        json!(0),
        "the good rows were not added either"
    );

    // A wrong header, and a file without rows.
    for wrong in ["site,when\nblog.example.com,now\n", header] {
        assert_eq!(
            post_csv(&app, &session, "text/csv", wrong)
                .await
                .status_code(),
            400
        );
    }

    // The good file, as the body and as a form.
    let response = post_csv(&app, &session, "text/csv", &good).await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(json(response).await["imported"], json!(3));
    let form = format!(
        "--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"hits.csv\"\r\nContent-Type: text/csv\r\n\r\n{good}\r\n--b--\r\n"
    );
    let response = post_csv(&app, &session, "multipart/form-data; boundary=b", &form).await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(json(response).await["imported"], json!(3));

    // Cleaned like a tracked hit; no visitors, only pageviews.
    let stats = fetch_stats().await;
    assert_eq!(stats["pageviews"], json!(4));
    assert_eq!(stats["visitors"], json!(0));
    // Unknown device, browser and os show as `unknown`, and can be filtered on.
    assert!(
        stats["devices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["device"] == json!("unknown")),
        "device is shown as unknown"
    );
    assert!(
        stats["browsers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["browser"] == json!("unknown")),
        "browser is shown as unknown"
    );
    assert!(
        stats["os"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["os"] == json!("unknown")),
        "os is shown as unknown"
    );
    let unknown = json(
        request(
            &app,
            Method::GET,
            "/api/stats?sites=blog.example.com&from=2026-03-01T00:00:00.000Z&to=2026-03-10T00:00:00.000Z&q=device==unknown",
            Some(&session),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(
        unknown["pageviews"],
        json!(2),
        "the two imports of the row without a device"
    );
    let export = request(
        &app,
        Method::GET,
        "/api/export?sites=blog.example.com&from=2026-03-01T00:00:00.000Z&to=2026-03-10T00:00:00.000Z",
        Some(&session),
        None,
    )
    .await
    .text();
    assert!(export.contains("blog.example.com,2026-03-04T12:00:00.000Z,pageview,/a,,news.example.org,no,desktop,chrome,macos"));
    assert!(
        export.contains(
            "blog.example.com,2026-03-04T12:00:00.000Z,click,/a,signup,,,mobile,safari,ios"
        )
    );

    // A file with the visitor column: the same visitor counts once, and an
    // empty one is no visitor. A header with another column is refused.
    let with_visitor = "site,created_at,kind,path,name,referrer,country,device,browser,os,visitor\r\n\
         blog.example.com,2026-03-05T10:00:00Z,pageview,/a,,,no,desktop,chrome,macos,0123456789abcdef0123456789abcdef\r\n\
         blog.example.com,2026-03-05T11:00:00Z,pageview,/b,,,no,desktop,chrome,macos,0123456789abcdef0123456789abcdef\r\n\
         blog.example.com,2026-03-05T12:00:00Z,pageview,/a,,,us,mobile,safari,ios,fedcba9876543210fedcba9876543210\r\n\
         blog.example.com,2026-03-05T13:00:00Z,pageview,/a,,,,,,,\r\n";
    let response = post_csv(&app, &session, "text/csv", with_visitor).await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(json(response).await["imported"], json!(4));
    let stats = fetch_stats().await;
    assert_eq!(stats["pageviews"], json!(8));
    assert_eq!(stats["visitors"], json!(2));
    // Sites that do not exist are refused, unless the import may create them;
    // an invalid hostname is refused either way, and nothing is created then.
    let new_site = "site,created_at,kind,path,name,referrer,country,device,browser,os\r\n\
         new.example.com,2026-03-05T10:00:00Z,pageview,/a,,,,,,\r\n\
         new.example.com,2026-03-05T11:00:00Z,pageview,/a,,,,,,\r\n\
         blog.example.com,2026-03-05T12:00:00Z,pageview,/a,,,,,,\r\n";
    let create = |body: String| {
        let (app, session) = (&app, &session);
        async move {
            app.server
                .post("/api/import?create_sites=true")
                .add_header("cookie", session)
                .add_header("content-type", "text/csv")
                .bytes(body.into())
                .await
        }
    };
    assert_eq!(
        post_csv(&app, &session, "text/csv", new_site)
            .await
            .status_code(),
        400
    );
    let invalid = new_site.replace("new.example.com", "not a host");
    let response = create(invalid).await;
    assert_eq!(response.status_code(), 400);
    assert!(
        json(response).await["error"]
            .as_str()
            .unwrap()
            .contains("not a valid hostname")
    );
    let sites = || async {
        json(request(&app, Method::GET, "/api/sites", Some(&session), None).await).await
    };
    assert_eq!(
        sites().await.as_array().unwrap().len(),
        1,
        "nothing was created"
    );
    let response = create(new_site.to_string()).await;
    assert_eq!(response.status_code(), 200);
    let body = json(response).await;
    assert_eq!(
        (body["imported"].clone(), body["sites_created"].clone()),
        (json!(3), json!(1))
    );
    assert_eq!(sites().await.as_array().unwrap().len(), 2);
    let again = json(create(new_site.to_string()).await).await;
    assert_eq!(again["sites_created"], json!(0), "the site exists now");

    let wrong = with_visitor.replace(",visitor\r\n", ",who\r\n");
    assert_eq!(
        post_csv(&app, &session, "text/csv", &wrong)
            .await
            .status_code(),
        400
    );
}

// Single sign-on. The provider is a stand-in served by these tests: its
// discovery document, its keys (JWKS) and its token endpoint, which answers
// a code with the ID token the test signed for it.

mod provider {
    use axum::extract::{Form, State};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use chrono::{Duration, Utc};
    use openidconnect::core::{
        CoreIdToken, CoreIdTokenClaims, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
        CoreRsaPrivateSigningKey,
    };
    use openidconnect::{
        Audience, EmptyAdditionalClaims, EndUserEmail, IssuerUrl, JsonWebKeyId, Nonce,
        PrivateSigningKey, StandardClaims, SubjectIdentifier,
    };
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub const CLIENT_ID: &str = "potoo";
    pub const CLIENT_SECRET: &str = "the client secret";

    pub struct Provider {
        pub issuer: String,
        key: Arc<CoreRsaPrivateSigningKey>,
        other_key: Arc<CoreRsaPrivateSigningKey>,
        /// The other key under the first one's id.
        other_key_same_id: Arc<CoreRsaPrivateSigningKey>,
        /// Whether the provider has rotated to the other key: it publishes
        /// and signs with that one now.
        rotated: Arc<AtomicBool>,
        /// Whether it kept the first key's id for it.
        kept_id: Arc<AtomicBool>,
        codes: Arc<Mutex<HashMap<String, String>>>,
    }

    /// What an ID token says; [`Claims::new`] is a valid one.
    pub struct Claims {
        pub issuer: String,
        pub audience: String,
        pub subject: String,
        pub email: Option<String>,
        pub email_verified: bool,
        pub nonce: String,
        pub expires_in: Duration,
        /// Signed with the key the provider does not publish.
        pub foreign_key: bool,
    }

    impl Claims {
        pub fn new(provider: &Provider, subject: &str, email: &str, nonce: &str) -> Self {
            Claims {
                issuer: provider.issuer.clone(),
                audience: CLIENT_ID.into(),
                subject: subject.into(),
                email: Some(email.into()),
                email_verified: true,
                nonce: nonce.into(),
                expires_in: Duration::minutes(5),
                foreign_key: false,
            }
        }
    }

    fn signing_key(pem: &str, id: &str) -> CoreRsaPrivateSigningKey {
        CoreRsaPrivateSigningKey::from_pem(pem, Some(JsonWebKeyId::new(id.into()))).unwrap()
    }

    pub async fn spawn() -> Provider {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let key = Arc::new(signing_key(include_str!("fixtures/oidc-key.pem"), "first"));
        let other_key = Arc::new(signing_key(
            include_str!("fixtures/oidc-other-key.pem"),
            "second",
        ));
        let other_key_same_id = Arc::new(signing_key(
            include_str!("fixtures/oidc-other-key.pem"),
            "first",
        ));
        let rotated = Arc::new(AtomicBool::new(false));
        let kept_id = Arc::new(AtomicBool::new(false));
        let codes: Arc<Mutex<HashMap<String, String>>> = Arc::default();
        let discovery = json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}/token"),
            "jwks_uri": format!("{issuer}/jwks"),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
        });
        let jwks = {
            let keys = (
                key.clone(),
                other_key.clone(),
                other_key_same_id.clone(),
                rotated.clone(),
                kept_id.clone(),
            );
            move || {
                let (key, other_key, other_key_same_id, rotated, kept_id) = &keys;
                let published = match (
                    rotated.load(Ordering::SeqCst),
                    kept_id.load(Ordering::SeqCst),
                ) {
                    (false, _) => key,
                    (true, false) => other_key,
                    (true, true) => other_key_same_id,
                };
                let keys = CoreJsonWebKeySet::new(vec![published.as_verification_key()]);
                async move { Json(serde_json::to_value(keys).unwrap()) }
            }
        };
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || async move { Json(discovery) }),
            )
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .with_state(codes.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Provider {
            issuer,
            key,
            other_key,
            other_key_same_id,
            rotated,
            kept_id,
            codes,
        }
    }

    async fn token(
        State(codes): State<Arc<Mutex<HashMap<String, String>>>>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
        let code = form.get("code").cloned().unwrap_or_default();
        let id_token = codes.lock().unwrap().remove(&code).ok_or((
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant" })),
        ))?;
        Ok(Json(json!({
            "access_token": "an access token",
            "token_type": "Bearer",
            "expires_in": 300,
            "id_token": id_token,
        })))
    }

    impl Provider {
        /// Publish and sign with the other key from now on.
        pub fn rotate(&self) {
            self.rotated.store(true, Ordering::SeqCst);
        }

        /// The same, but under the first key's id.
        pub fn rotate_keeping_id(&self) {
            self.kept_id.store(true, Ordering::SeqCst);
            self.rotate();
        }

        /// A code the token endpoint answers with an ID token of these claims.
        pub fn code_for(&self, claims: Claims) -> String {
            let now = Utc::now();
            let standard = StandardClaims::new(SubjectIdentifier::new(claims.subject))
                .set_email(claims.email.map(EndUserEmail::new))
                .set_email_verified(Some(claims.email_verified));
            let token_claims = CoreIdTokenClaims::new(
                IssuerUrl::new(claims.issuer).unwrap(),
                vec![Audience::new(claims.audience)],
                now + claims.expires_in,
                now - Duration::minutes(1),
                standard,
                EmptyAdditionalClaims {},
            )
            .set_nonce(Some(Nonce::new(claims.nonce)));
            let rotated = self.rotated.load(Ordering::SeqCst);
            let key = match (
                claims.foreign_key != rotated,
                self.kept_id.load(Ordering::SeqCst),
            ) {
                (false, _) => &*self.key,
                (true, true) if rotated => &*self.other_key_same_id,
                (true, _) => &*self.other_key,
            };
            let id_token = CoreIdToken::new(
                token_claims,
                key,
                CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
                None,
                None,
            )
            .unwrap();
            let code = potoo::random::hex::<8>();
            self.codes
                .lock()
                .unwrap()
                .insert(code.clone(), id_token.to_string());
            code
        }
    }
}

/// A cookie value as a browser sends it.
fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The value of a cookie a response sets, as a `cookie` header value.
fn set_cookie(response: &Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .find(|pair| pair.starts_with(&format!("{name}=")) && pair.len() > name.len() + 1)
        .map(String::from)
}

fn location(response: &Response) -> String {
    response.headers()["location"].to_str().unwrap().to_string()
}

/// A login in progress: where the app sent the browser.
struct Login {
    url: reqwest::Url,
    state: String,
    nonce: String,
    /// The login cookie, as a `cookie` header value.
    cookie: String,
}

async fn start_login(app: &TestApp) -> Login {
    let response = app.server.get("/api/auth/oidc/login").await;
    assert_eq!(response.status_code(), 303);
    let url = reqwest::Url::parse(&location(&response)).expect("a redirect to the provider");
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.to_string())
            .unwrap_or_default()
    };
    Login {
        state: param("state"),
        nonce: param("nonce"),
        cookie: set_cookie(&response, "potoo_oidc").expect("the login cookie"),
        url,
    }
}

/// The provider's redirect back to the app.
async fn callback(app: &TestApp, query: &str, cookie: Option<&str>) -> Response {
    let mut request = app.server.get(&format!("/api/auth/oidc/callback?{query}"));
    if let Some(cookie) = cookie {
        request = request.add_header("cookie", cookie);
    }
    request.await
}

/// A whole login with these claims (the nonce is the login's): the
/// session cookie, or where the browser was sent instead.
async fn sign_in(
    app: &TestApp,
    provider: &provider::Provider,
    claims: impl FnOnce(&Login) -> provider::Claims,
) -> Result<String, String> {
    let login = start_login(app).await;
    let code = provider.code_for(claims(&login));
    let response = callback(
        app,
        &format!("code={code}&state={}", login.state),
        Some(&login.cookie),
    )
    .await;
    assert_eq!(response.status_code(), 303);
    match set_cookie(&response, "potoo_session") {
        Some(session) => {
            assert_eq!(location(&response), "/");
            Ok(session)
        }
        None => Err(location(&response)),
    }
}

/// The first admin, signed in.
async fn admin_session(app: &TestApp) -> String {
    let response = request(
        app,
        Method::POST,
        "/api/auth/bootstrap",
        None,
        Some(json!({ "email": "admin@example.com", "password": "correct horse battery" })),
    )
    .await;
    cookie(&response)
}

async fn save_oidc(app: &TestApp, session: &str, body: Value) -> Response {
    request(
        app,
        Method::PUT,
        "/api/admin/oidc",
        Some(session),
        Some(body),
    )
    .await
}

async fn configure(app: &TestApp, session: &str, provider: &provider::Provider, extra: Value) {
    let mut body = json!({
        "issuer": provider.issuer,
        "client_id": provider::CLIENT_ID,
        "client_secret": provider::CLIENT_SECRET,
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let response = save_oidc(app, session, body).await;
    assert_eq!(response.status_code(), 200);
}

/// What the frontend knows before routing a page: `/api/auth/status` and,
/// when signed in, `/api/auth/me`, put together as the old session endpoint
/// answered (`needsSetup`, `user`, `oidc.label`, `password_login`,
/// `totp_required`), which is what these tests check.
async fn session_of(app: &TestApp, session: Option<&str>) -> Value {
    let status = json(request(app, Method::GET, "/api/auth/status", None, None).await).await;
    let me = request(app, Method::GET, "/api/auth/me", session, None).await;
    let user = if me.status_code() == 200 {
        json(me).await
    } else {
        assert_eq!(me.status_code(), 401);
        Value::Null
    };
    json!({
        "needsSetup": status["bootstrap_required"],
        "user": user,
        "oidc": if status["sso"].is_null() { Value::Null } else { json!({ "label": status["sso"] }) },
        "password_login": status["password_login"],
        "totp_required": status["totp_required"],
    })
}

/// Single sign-on is off until an admin sets a provider that
/// answers; only admins see or change the settings; a user without a
/// password needs single sign-on to be on.
#[tokio::test]
async fn single_sign_on_is_off_until_a_working_provider_is_set() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    assert_eq!(session_of(&app, None).await["oidc"], json!(null));
    let response = app.server.get("/api/auth/oidc/login").await;
    assert_eq!(response.status_code(), 303);
    assert_eq!(location(&response), "/login?error=unavailable");

    // No password is only for single sign-on.
    let passwordless = json!({ "email": "sso@example.com" });
    let response = request(
        &app,
        Method::POST,
        "/api/admin/users",
        Some(&admin),
        Some(passwordless.clone()),
    )
    .await;
    assert_eq!(response.status_code(), 400);

    // A provider that does not answer, or not over TLS, is refused, and
    // nothing changes.
    for issuer in ["http://127.0.0.1:9", "http://id.example.com"] {
        let response = save_oidc(
            &app,
            &admin,
            json!({ "issuer": issuer, "client_id": "potoo" }),
        )
        .await;
        assert_eq!(response.status_code(), 400, "{issuer}");
    }
    let settings =
        json(request(&app, Method::GET, "/api/admin/oidc", Some(&admin), None).await).await;
    assert_eq!(settings["enabled"], json!(false));
    assert_eq!(settings["issuer"], json!(""));
    assert_eq!(
        settings["callback_url"],
        json!(format!("{}/api/auth/oidc/callback", app.base))
    );

    // Admins only.
    request(
        &app,
        Method::POST,
        "/api/admin/users",
        Some(&admin),
        Some(json!({ "email": "user@example.com", "password": "correct horse battery" })),
    )
    .await;
    let user = cookie(
        &request(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "user@example.com", "password": "correct horse battery" })),
        )
        .await,
    );
    assert_eq!(
        request(&app, Method::GET, "/api/admin/oidc", Some(&user), None)
            .await
            .status_code(),
        403
    );
    assert_eq!(
        save_oidc(&app, &user, json!({ "label": "x" }))
            .await
            .status_code(),
        403
    );
    assert_eq!(
        request(&app, Method::GET, "/api/admin/oidc", None, None)
            .await
            .status_code(),
        401
    );

    // With a working provider it is on, at once, and the secret is never
    // sent back.
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({ "label": "Acme ID" })).await;
    let settings = request(&app, Method::GET, "/api/admin/oidc", Some(&admin), None).await;
    let settings = settings.text();
    assert!(!settings.contains(provider::CLIENT_SECRET), "{settings}");
    let settings: Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(settings["enabled"], json!(true));
    assert_eq!(settings["client_secret_set"], json!(true));
    assert_eq!(
        session_of(&app, None).await["oidc"],
        json!({ "label": "Acme ID" })
    );
    let response = request(
        &app,
        Method::POST,
        "/api/admin/users",
        Some(&admin),
        Some(passwordless),
    )
    .await;
    assert_eq!(response.status_code(), 201);

    // Leaving a field out keeps it; an empty secret removes it.
    let response = save_oidc(&app, &admin, json!({ "client_secret": "" })).await;
    let settings = json(response).await;
    assert_eq!(settings["client_secret_set"], json!(false));
    assert_eq!(settings["label"], json!("Acme ID"));
}

/// The login asks for a code with PKCE and a nonce; an
/// existing user is found by their verified email, then by the provider
/// account, whatever the email becomes.
#[tokio::test]
async fn single_sign_on_signs_existing_users_in() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({})).await;
    request(
        &app,
        Method::POST,
        "/api/admin/users",
        Some(&admin),
        Some(json!({ "email": "b@example.com", "password": "correct horse battery" })),
    )
    .await;

    let login = start_login(&app).await;
    assert_eq!(
        login.url.as_str().split('?').next(),
        Some(&*format!("{}/authorize", provider.issuer))
    );
    let params: std::collections::HashMap<_, _> = login.url.query_pairs().into_owned().collect();
    assert_eq!(params["response_type"], "code");
    assert_eq!(params["client_id"], provider::CLIENT_ID);
    assert_eq!(params["code_challenge_method"], "S256");
    assert!(!params["code_challenge"].is_empty());
    assert!(!login.nonce.is_empty() && !login.state.is_empty());
    assert_eq!(params["scope"], "openid email");
    assert_eq!(
        params["redirect_uri"],
        format!("{}/api/auth/oidc/callback", app.base)
    );

    let session = sign_in(&app, &provider, |login| {
        provider::Claims::new(&provider, "account-1", "B@Example.com", &login.nonce)
    })
    .await
    .expect("signed in");
    let user = &session_of(&app, Some(&session)).await["user"];
    assert_eq!(user["email"], json!("b@example.com"));
    assert_eq!(user["has_password"], json!(true));

    // Linked now: the account signs in as b, whatever its email says.
    let session = sign_in(&app, &provider, |login| {
        provider::Claims::new(&provider, "account-1", "renamed@example.com", &login.nonce)
    })
    .await
    .expect("signed in");
    assert_eq!(
        session_of(&app, Some(&session)).await["user"]["email"],
        json!("b@example.com")
    );
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    let linked: Vec<_> = users
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u["email"].as_str().unwrap(),
                u["single_sign_on"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        linked,
        [("admin@example.com", false), ("b@example.com", true)]
    );
}

/// Every callback that is not the provider's answer to a
/// login of this browser signs nobody in.
#[tokio::test]
async fn single_sign_on_refuses_bad_callbacks() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({})).await;
    request(
        &app,
        Method::POST,
        "/api/admin/users",
        Some(&admin),
        Some(json!({ "email": "b@example.com", "password": "correct horse battery" })),
    )
    .await;
    let valid = |login: &Login| {
        provider::Claims::new(&provider, "account-1", "b@example.com", &login.nonce)
    };
    let refused = |response: Response, error: &str| {
        assert_eq!(response.status_code(), 303);
        assert_eq!(location(&response), format!("/login?error={error}"));
        assert_eq!(set_cookie(&response, "potoo_session"), None);
    };

    // The state: missing, unknown, from another browser, or used before.
    refused(callback(&app, "code=x", None).await, "state");
    let login = start_login(&app).await;
    let code = provider.code_for(valid(&login));
    refused(
        callback(
            &app,
            &format!("code={code}&state=unknown"),
            Some(&login.cookie),
        )
        .await,
        "state",
    );
    refused(
        callback(&app, &format!("code={code}&state={}", login.state), None).await,
        "state",
    );
    let login = start_login(&app).await;
    let other = start_login(&app).await;
    let code = provider.code_for(valid(&login));
    let query = format!("code={code}&state={}", login.state);
    refused(callback(&app, &query, Some(&other.cookie)).await, "state");
    // A login cookie this server did not sign, made from what the
    // provider's URL shows: the state and the nonce.
    let forged = json!({
        "state": login.state,
        "nonce": login.nonce,
        "verifier": "a guess",
        "redirect_url": format!("{}/api/auth/oidc/callback", app.base),
        "started": jiff::Timestamp::now().as_second(),
    });
    let forged = format!("potoo_oidc={}", urlencoding(&forged.to_string()));
    refused(callback(&app, &query, Some(&forged)).await, "state");

    // The provider refused, or does not know the code.
    let login = start_login(&app).await;
    let query = format!("error=access_denied&state={}", login.state);
    refused(
        callback(&app, &query, Some(&login.cookie)).await,
        "provider",
    );
    let login = start_login(&app).await;
    let query = format!("code=unknown&state={}", login.state);
    refused(
        callback(&app, &query, Some(&login.cookie)).await,
        "provider",
    );

    // An ID token that is not for this login, this client, now, or not
    // signed by the provider.
    type Change = fn(&mut provider::Claims);
    let changes: [(&str, Change); 5] = [
        ("nonce", |c| c.nonce = "another nonce".into()),
        ("audience", |c| c.audience = "another client".into()),
        ("issuer", |c| c.issuer = "http://127.0.0.1:9".into()),
        ("expiry", |c| c.expires_in = chrono::Duration::minutes(-5)),
        ("signature", |c| c.foreign_key = true),
    ];
    for (what, change) in changes {
        let result = sign_in(&app, &provider, |login| {
            let mut claims = valid(login);
            change(&mut claims);
            claims
        })
        .await;
        assert_eq!(result, Err("/login?error=token".into()), "{what}");
    }

    // No verified email for an account that is not linked yet, or nobody
    // with that email.
    let result = sign_in(&app, &provider, |login| provider::Claims {
        email_verified: false,
        ..valid(login)
    })
    .await;
    assert_eq!(result, Err("/login?error=email".into()));
    let result = sign_in(&app, &provider, |login| provider::Claims {
        email: None,
        ..valid(login)
    })
    .await;
    assert_eq!(result, Err("/login?error=email".into()));
    let result = sign_in(&app, &provider, |login| {
        provider::Claims::new(&provider, "account-2", "nobody@example.com", &login.nonce)
    })
    .await;
    assert_eq!(result, Err("/login?error=unknown_user".into()));

    // None of that linked or created anything.
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    assert_eq!(users.as_array().unwrap().len(), 2);
    assert!(
        users
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u["single_sign_on"] == json!(false))
    );
    // And the valid login still works, once: a replay of its callback
    // fails at the provider, which takes a code once.
    let login = start_login(&app).await;
    let code = provider.code_for(valid(&login));
    let query = format!("code={code}&state={}", login.state);
    let response = callback(&app, &query, Some(&login.cookie)).await;
    assert!(set_cookie(&response, "potoo_session").is_some());
    refused(
        callback(&app, &query, Some(&login.cookie)).await,
        "provider",
    );
}

/// A created user is a regular one without a password, who can set one
/// without a current one, once.
#[tokio::test]
async fn single_sign_on_creates_regular_users_without_a_password() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({ "auto_create": true })).await;

    let session = sign_in(&app, &provider, |login| {
        provider::Claims::new(&provider, "account-9", "new@example.com", &login.nonce)
    })
    .await
    .expect("signed in");
    let user = &session_of(&app, Some(&session)).await["user"];
    assert_eq!(user["email"], json!("new@example.com"));
    assert_eq!(user["is_admin"], json!(false));
    assert_eq!(user["has_password"], json!(false));

    // No password to sign in with: like a wrong one.
    let login = |password: &str| {
        request(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "new@example.com", "password": password })),
        )
    };
    assert_eq!(login("correct horse battery").await.status_code(), 401);

    // A first password needs no current one ...
    let change = |body: Value| {
        request(
            &app,
            Method::POST,
            "/api/auth/password",
            Some(&session),
            Some(body),
        )
    };
    let response = change(json!({ "new_password": "correct horse battery" })).await;
    assert_eq!(response.status_code(), 204);
    assert_eq!(
        session_of(&app, Some(&session)).await["user"]["has_password"],
        json!(true)
    );
    assert_eq!(login("correct horse battery").await.status_code(), 204);
    // ... the next one does.
    let response = change(json!({ "new_password": "another horse battery" })).await;
    assert_eq!(response.status_code(), 401);
    let response = change(json!({
        "current_password": "correct horse battery",
        "new_password": "another horse battery",
    }))
    .await;
    assert_eq!(response.status_code(), 204);
}

/// An OIDC_* variable turns single sign-on on by itself, and its
/// field cannot be changed in the admin settings.
#[tokio::test]
async fn environment_variables_lock_their_fields() {
    let provider = provider::spawn().await;
    let app = spawn_app_with(oidc::Env {
        issuer: Some(provider.issuer.clone()),
        client_id: Some(provider::CLIENT_ID.into()),
        label: Some("Env ID".into()),
        ..oidc::Env::default()
    })
    .await;
    let admin = admin_session(&app).await;
    assert_eq!(
        session_of(&app, None).await["oidc"],
        json!({ "label": "Env ID" })
    );

    let settings =
        json(request(&app, Method::GET, "/api/admin/oidc", Some(&admin), None).await).await;
    assert_eq!(settings["locked"], json!(["issuer", "client_id", "label"]));
    assert_eq!(settings["issuer"], json!(provider.issuer));
    let response = save_oidc(&app, &admin, json!({ "label": "Other" })).await;
    assert_eq!(response.status_code(), 400);
    assert!(
        json(response).await["error"]
            .as_str()
            .unwrap()
            .contains("OIDC_LABEL")
    );
    let response = save_oidc(&app, &admin, json!({ "auto_create": true })).await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(
        session_of(&app, None).await["oidc"],
        json!({ "label": "Env ID" })
    );

    // Signing in works on the variables alone.
    let session = sign_in(&app, &provider, |login| {
        provider::Claims::new(&provider, "account-1", "admin@example.com", &login.nonce)
    })
    .await
    .expect("signed in");
    assert_eq!(
        session_of(&app, Some(&session)).await["user"]["email"],
        json!("admin@example.com")
    );
}

/// Before the first admin is created on /setup, single sign-on signs nobody
/// in, even with auto-create on: a user made then would take /setup away
/// and leave the instance without an admin.
#[tokio::test]
async fn single_sign_on_signs_nobody_in_before_setup() {
    let provider = provider::spawn().await;
    let app = spawn_app_with(oidc::Env {
        issuer: Some(provider.issuer.clone()),
        client_id: Some(provider::CLIENT_ID.into()),
        auto_create: Some(true),
        ..oidc::Env::default()
    })
    .await;
    let result = sign_in(&app, &provider, |login| {
        provider::Claims::new(&provider, "account-1", "someone@example.com", &login.nonce)
    })
    .await;
    assert_eq!(result, Err("/login?error=unavailable".into()));
    assert_eq!(session_of(&app, None).await["needsSetup"], json!(true));
    let response = request(
        &app,
        Method::POST,
        "/api/auth/bootstrap",
        None,
        Some(json!({ "email": "admin@example.com", "password": "correct horse battery" })),
    )
    .await;
    assert_eq!(response.status_code(), 201);
}

/// Two admins saving different fields at once both keep their change, and
/// what is in effect is what is stored.
#[tokio::test]
async fn concurrent_saves_of_the_settings_keep_both_changes() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({})).await;
    for round in 0..5 {
        let label = format!("Label {round}");
        let scopes = format!("openid email scope{round}");
        let (a, b) = tokio::join!(
            save_oidc(&app, &admin, json!({ "label": label })),
            save_oidc(&app, &admin, json!({ "scopes": scopes })),
        );
        assert_eq!(
            (a.status_code().as_u16(), b.status_code().as_u16()),
            (200, 200)
        );
        let settings =
            json(request(&app, Method::GET, "/api/admin/oidc", Some(&admin), None).await).await;
        assert_eq!(
            (settings["label"].clone(), settings["scopes"].clone()),
            (json!(label), json!(scopes)),
            "round {round}"
        );
        assert_eq!(session_of(&app, None).await["oidc"]["label"], json!(label));
    }
}

/// A provider that is down is not asked again by every login: one failed
/// lookup answers the logins of the next half minute.
#[tokio::test]
async fn a_provider_that_is_down_is_not_asked_by_every_login() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let asked = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let counter = asked.clone();
    let down = axum::Router::new().fallback(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        async { axum::http::StatusCode::SERVICE_UNAVAILABLE }
    });
    tokio::spawn(async move { axum::serve(listener, down).await.unwrap() });

    let app = spawn_app_with(oidc::Env {
        issuer: Some(issuer),
        client_id: Some(provider::CLIENT_ID.into()),
        ..oidc::Env::default()
    })
    .await;
    admin_session(&app).await;
    for _ in 0..3 {
        let response = app.server.get("/api/auth/oidc/login").await;
        assert_eq!(location(&response), "/login?error=unavailable");
    }
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

/// The whole `set-cookie` header for a cookie, attributes included.
fn set_cookie_header(response: &Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(String::from)
}

/// The login cookie goes to the callback only, cannot be read by scripts,
/// survives the provider's cross-site redirect back, and is used up by the
/// callback. Over https it is a `__Host-` cookie, which a sibling subdomain
/// cannot plant.
#[tokio::test]
async fn the_login_cookie_is_scoped_and_used_up() {
    let provider = provider::spawn().await;
    let start = |app: &TestApp| app.server.get("/api/auth/oidc/login");

    // Over plain http (no PUBLIC_URL): a plain cookie, for the callback only.
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    configure(&app, &admin, &provider, json!({})).await;
    let response = start(&app).await;
    let cookie = set_cookie_header(&response, "potoo_oidc").expect("the login cookie");
    for attribute in [
        "HttpOnly",
        "SameSite=Lax",
        "Path=/api/auth/oidc/callback",
        "Max-Age=600",
    ] {
        assert!(cookie.contains(attribute), "{attribute} in {cookie}");
    }
    assert!(!cookie.contains("Secure"), "{cookie}");

    // Whatever the callback answers, the cookie is gone after it.
    let login = start_login(&app).await;
    for query in [
        format!("state={}&error=access_denied", login.state),
        format!("state={}&code=unknown", login.state),
    ] {
        let response = callback(&app, &query, Some(&login.cookie)).await;
        let cleared = set_cookie_header(&response, "potoo_oidc").expect("the cookie cleared");
        assert!(cleared.starts_with("potoo_oidc=;"), "{cleared}");
        assert!(cleared.contains("Max-Age=0"), "{cleared}");
    }

    // With an https PUBLIC_URL: `__Host-`, Secure, the whole site's path.
    let https = spawn(Options {
        public_url: Some("https://potoo.example.com".into()),
        ..Options::default()
    })
    .await;
    let admin = secure_admin_session(&https).await;
    configure(&https, &admin, &provider, json!({})).await;
    let response = start(&https).await;
    let header = set_cookie_header(&response, "__Host-potoo_oidc").expect("the login cookie");
    for attribute in ["HttpOnly", "SameSite=Lax", "Path=/;", "Secure"] {
        assert!(header.contains(attribute), "{attribute} in {header}");
    }

    // There, the removal is Secure too (a browser ignores a `__Host-`
    // removal without it); and a cookie this server did not sign (from
    // before a restart, say) is cleared as well.
    let cookie = set_cookie(&response, "__Host-potoo_oidc").expect("the login cookie");
    for cookie in [cookie.as_str(), "__Host-potoo_oidc=not-signed-here"] {
        let response = https
            .server
            .get("/api/auth/oidc/callback?state=x&code=y")
            .add_header("cookie", cookie)
            .await;
        let cleared =
            set_cookie_header(&response, "__Host-potoo_oidc").expect("the cookie cleared");
        for attribute in [
            "__Host-potoo_oidc=;",
            "Max-Age=0",
            "Secure",
            "Path=/;",
            "HttpOnly",
        ] {
            assert!(cleared.contains(attribute), "{attribute} in {cleared}");
        }
    }
}

/// The first admin of an app with an https PUBLIC_URL: its session cookie
/// is `__Host-potoo_session` (as a `cookie` header value).
async fn secure_admin_session(app: &TestApp) -> String {
    let response = request(
        app,
        Method::POST,
        "/api/auth/bootstrap",
        None,
        Some(json!({ "email": "admin@example.com", "password": "correct horse battery" })),
    )
    .await;
    let header = set_cookie_header(&response, "__Host-potoo_session").expect("the session cookie");
    for attribute in ["HttpOnly", "SameSite=Strict", "Path=/", "Secure"] {
        assert!(header.contains(attribute), "{attribute} in {header}");
    }
    set_cookie(&response, "__Host-potoo_session").unwrap()
}

/// A second account at the provider with the email of a user who is
/// linked already (a recycled address, say) does not get that user.
#[tokio::test]
async fn a_linked_user_is_not_taken_by_a_second_account() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({})).await;
    let as_b = |subject: &'static str| {
        let provider = &provider;
        move |login: &Login| {
            provider::Claims::new(provider, subject, "admin@example.com", &login.nonce)
        }
    };
    assert!(sign_in(&app, &provider, as_b("account-1")).await.is_ok());
    let result = sign_in(&app, &provider, as_b("account-2")).await;
    assert_eq!(result, Err("/login?error=linked".into()));
    assert!(sign_in(&app, &provider, as_b("account-1")).await.is_ok());
}

/// The client secret goes to one provider only: a new issuer without a new
/// secret drops the old one, and a secret from the environment pins the
/// issuer.
#[tokio::test]
async fn a_new_issuer_does_not_get_the_old_secret() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let first = provider::spawn().await;
    let second = provider::spawn().await;
    configure(&app, &admin, &first, json!({})).await;

    let settings = json(save_oidc(&app, &admin, json!({ "issuer": second.issuer })).await).await;
    assert_eq!(settings["client_secret_set"], json!(false));
    let settings = json(
        save_oidc(
            &app,
            &admin,
            json!({ "issuer": first.issuer, "client_secret": "another secret" }),
        )
        .await,
    )
    .await;
    assert_eq!(settings["client_secret_set"], json!(true));
    // Other changes keep it.
    let settings = json(save_oidc(&app, &admin, json!({ "label": "Acme" })).await).await;
    assert_eq!(settings["client_secret_set"], json!(true));

    let app = spawn_app_with(oidc::Env {
        client_secret: Some("from the environment".into()),
        ..oidc::Env::default()
    })
    .await;
    let admin = admin_session(&app).await;
    let response = save_oidc(
        &app,
        &admin,
        json!({ "issuer": first.issuer, "client_id": provider::CLIENT_ID }),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    assert!(
        json(response).await["error"]
            .as_str()
            .unwrap()
            .contains("OIDC_ISSUER")
    );
}

/// A provider whose discovery document says this, at an address of its
/// own on this machine.
async fn spawn_discovery(document: impl Fn(&str) -> String + Send + 'static) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let body = document(&issuer);
    let app = axum::Router::new()
        .route(
            "/.well-known/openid-configuration",
            axum::routing::get(
                move || async move { ([("content-type", "application/json")], body) },
            ),
        )
        .route(
            "/jwks",
            axum::routing::get(|| async { axum::Json(json!({ "keys": [] })) }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    issuer
}

/// The provider's own endpoints must use TLS too, and its answers are
/// small: one that is not is refused.
#[tokio::test]
async fn a_provider_must_use_tls_and_answer_briefly() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let plain_token_endpoint = spawn_discovery(|issuer| {
        json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": "http://id.example.com/token",
            "jwks_uri": format!("{issuer}/jwks"),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
        })
        .to_string()
    })
    .await;
    let response = save_oidc(
        &app,
        &admin,
        json!({ "issuer": plain_token_endpoint, "client_id": "potoo" }),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    let error = json(response).await["error"].as_str().unwrap().to_string();
    assert!(error.contains("http://id.example.com/token"), "{error}");

    let huge = spawn_discovery(|_| format!("{{\"padding\": \"{}\"}}", "x".repeat(2 << 20))).await;
    let response = save_oidc(
        &app,
        &admin,
        json!({ "issuer": huge, "client_id": "potoo" }),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    let error = json(response).await["error"].as_str().unwrap().to_string();
    assert!(error.contains("larger than"), "{error}");
}

/// A provider that rotated its signing key is fetched again at once, not
/// after the hour its keys are kept.
#[tokio::test]
async fn a_rotated_signing_key_is_fetched_at_once() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({})).await;
    let valid = |login: &Login| {
        provider::Claims::new(&provider, "account-1", "admin@example.com", &login.nonce)
    };
    assert!(sign_in(&app, &provider, valid).await.is_ok());
    provider.rotate();
    assert!(sign_in(&app, &provider, valid).await.is_ok());
}

/// The same when the provider replaced its key but kept its key id: the
/// cached key then fails the signature instead of being missing.
#[tokio::test]
async fn a_signing_key_replaced_under_the_same_id_is_fetched_at_once() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({})).await;
    let valid = |login: &Login| {
        provider::Claims::new(&provider, "account-1", "admin@example.com", &login.nonce)
    };
    assert!(sign_in(&app, &provider, valid).await.is_ok());
    provider.rotate_keeping_id();
    assert!(sign_in(&app, &provider, valid).await.is_ok());
    // A token the fresh keys do not check either is refused.
    let foreign = |login: &Login| provider::Claims {
        foreign_key: true,
        ..valid(login)
    };
    assert_eq!(
        sign_in(&app, &provider, foreign).await,
        Err("/login?error=token".into())
    );
}

/// A login whose callback is on another scheme than the page (http and
/// https) could never finish: it is refused at once, with a reason that
/// points at the setup rather than at the user.
#[tokio::test]
async fn a_callback_on_another_scheme_is_a_misconfiguration() {
    // An https callback, but no https PUBLIC_URL: the login cookie would
    // be a plain one the callback could not be trusted with.
    let provider = provider::spawn().await;
    let callback = json!({ "redirect_url": "https://potoo.example.com/api/auth/oidc/callback" });
    let plain = spawn_app().await;
    let admin = admin_session(&plain).await;
    configure(&plain, &admin, &provider, callback.clone()).await;
    let start = |app: &TestApp| app.server.get("/api/auth/oidc/login");
    let response = start(&plain).await;
    assert_eq!(location(&response), "/login?error=misconfigured");
    assert_eq!(set_cookie(&response, "potoo_oidc"), None);

    // With PUBLIC_URL on the same scheme it goes to the provider.
    let https = spawn(Options {
        public_url: Some("https://potoo.example.com".into()),
        ..Options::default()
    })
    .await;
    let admin = secure_admin_session(&https).await;
    configure(&https, &admin, &provider, callback).await;
    let response = start(&https).await;
    assert!(location(&response).starts_with(&provider.issuer));
}

/// The code an authenticator app shows now, `offset` steps away.
fn totp_code(secret: &str, offset: i64) -> String {
    let step = (potoo::totp::current_step() as i64 + offset) as u64;
    potoo::totp::code_at(secret, step).unwrap()
}

/// Sign in with a password: the response, and the cookie it set (the session,
/// or the challenge of the second step).
async fn password_login(app: &TestApp, email: &str) -> Response {
    request(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": email, "password": "correct horse battery" })),
    )
    .await
}

async fn second_step(app: &TestApp, challenge: Option<&str>, code: &str) -> Response {
    request(
        app,
        Method::POST,
        "/api/auth/login/totp",
        challenge,
        Some(json!({ "code": code })),
    )
    .await
}

async fn post(app: &TestApp, path: &str, session: &str, body: Option<Value>) -> Response {
    request(app, Method::POST, path, Some(session), body).await
}

/// Turn two-factor authentication on for the user of a session: the secret.
async fn enable(app: &TestApp, session: &str) -> String {
    let setup = json(post(app, "/api/auth/totp/setup", session, None).await).await;
    let secret = setup["secret"].as_str().unwrap().to_string();
    let body = json!({ "code": totp_code(&secret, 0) });
    let response = post(app, "/api/auth/totp/enable", session, Some(body)).await;
    assert_eq!(response.status_code(), 200);
    secret
}

async fn reset_totp(app: &TestApp, session: &str, id: &str) -> Response {
    let path = format!("/api/admin/users/{id}/totp");
    request(app, Method::DELETE, &path, Some(session), None).await
}

#[tokio::test]
async fn two_factor_authentication() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;

    // Off at first: a password signs in.
    assert_eq!(
        session_of(&app, Some(&admin)).await["user"]["totp_enabled"],
        json!(false)
    );
    assert_eq!(
        password_login(&app, "admin@example.com")
            .await
            .status_code(),
        204
    );

    // Enabling needs a setup first, then a code of its secret.
    let response = post(
        &app,
        "/api/auth/totp/enable",
        &admin,
        Some(json!({ "code": "123456" })),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    let setup = json(post(&app, "/api/auth/totp/setup", &admin, None).await).await;
    let secret = setup["secret"].as_str().unwrap().to_string();
    let uri = setup["uri"].as_str().unwrap();
    assert!(
        uri.starts_with("otpauth://totp/Potoo:admin%40example.com?"),
        "{uri}"
    );
    let wrong = if totp_code(&secret, 0) == "000000" {
        "111111"
    } else {
        "000000"
    };
    let response = post(
        &app,
        "/api/auth/totp/enable",
        &admin,
        Some(json!({ "code": wrong })),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    let response = post(
        &app,
        "/api/auth/totp/enable",
        &admin,
        Some(json!({ "code": totp_code(&secret, 0) })),
    )
    .await;
    assert_eq!(response.status_code(), 200);
    let codes: Vec<String> = serde_json::from_value(json(response).await["codes"].clone()).unwrap();
    assert_eq!(codes.len(), 10);
    assert_eq!(
        session_of(&app, Some(&admin)).await["user"]["totp_enabled"],
        json!(true)
    );
    // On already: no second setup.
    assert_eq!(
        post(&app, "/api/auth/totp/setup", &admin, None)
            .await
            .status_code(),
        409
    );

    // The password alone does not sign in any more.
    let response = password_login(&app, "admin@example.com").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(set_cookie(&response, sessions::SESSION_COOKIE), None);
    let challenge = set_cookie(&response, "potoo_2fa").expect("the challenge cookie");
    assert_eq!(
        json(response).await,
        json!({ "totp_required": true, "totp_setup_required": false })
    );

    // No challenge, or a forged one: sign in again.
    assert_eq!(
        second_step(&app, None, &totp_code(&secret, 0))
            .await
            .status_code(),
        401
    );
    assert_eq!(
        second_step(
            &app,
            Some("potoo_2fa=1.99999999999"),
            &totp_code(&secret, 0)
        )
        .await
        .status_code(),
        401
    );
    // The code used to enable it (this step) is used up; the next step's
    // code is in the allowed skew and works, once.
    assert_eq!(
        second_step(&app, Some(&challenge), wrong)
            .await
            .status_code(),
        400
    );
    let next = totp_code(&secret, 1);
    let response = second_step(&app, Some(&challenge), &next).await;
    assert_eq!(response.status_code(), 204);
    let session = set_cookie(&response, sessions::SESSION_COOKIE).expect("the session cookie");
    assert_eq!(
        session_of(&app, Some(&session)).await["user"]["email"],
        json!("admin@example.com")
    );
    let response = password_login(&app, "admin@example.com").await;
    let challenge = set_cookie(&response, "potoo_2fa").unwrap();
    assert_eq!(
        second_step(&app, Some(&challenge), &next)
            .await
            .status_code(),
        400
    );
    assert_eq!(
        second_step(&app, Some(&challenge), &totp_code(&secret, -1))
            .await
            .status_code(),
        400
    );

    // A recovery code works once, typed loosely.
    let typed = codes[0].to_uppercase().replace('-', " ");
    assert_eq!(
        second_step(&app, Some(&challenge), &typed)
            .await
            .status_code(),
        204
    );
    assert_eq!(
        second_step(&app, Some(&challenge), &codes[0])
            .await
            .status_code(),
        400
    );

    // New recovery codes replace the old ones.
    let response = post(
        &app,
        "/api/auth/totp/recovery-codes",
        &admin,
        Some(json!({ "code": codes[1] })),
    )
    .await;
    assert_eq!(response.status_code(), 200);
    let new_codes: Vec<String> =
        serde_json::from_value(json(response).await["codes"].clone()).unwrap();
    assert_eq!(
        second_step(&app, Some(&challenge), &codes[2])
            .await
            .status_code(),
        400
    );

    // The new codes work (the limits have their own test).
    assert_eq!(
        second_step(&app, Some(&challenge), &new_codes[0])
            .await
            .status_code(),
        204
    );
}

#[tokio::test]
async fn attempts_are_limited_per_email_address_and_user() {
    // The real limits: 5 per email, 30 per address, 5 codes per user.
    let app = spawn(Options {
        limits: potoo::limits::Limits::default(),
        ..Options::default()
    })
    .await;
    let admin = admin_session(&app).await;
    let attempt = |password: &'static str| {
        request(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "admin@example.com", "password": password })),
        )
    };
    // Right or wrong, every attempt counts: the sixth is refused.
    assert_eq!(attempt("wrong password").await.status_code(), 401);
    for _ in 0..3 {
        assert_eq!(attempt("correct horse battery").await.status_code(), 204);
    }
    assert_eq!(attempt("wrong password").await.status_code(), 401);
    let response = attempt("correct horse battery").await;
    assert_eq!(response.status_code(), 429);
    let wait: u64 = response.headers()["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&wait), "{wait}");
    assert_eq!(
        json(response).await,
        json!({ "error": "Too many attempts, try again later" })
    );
    // Nobody is locked out: the session goes on, and another email can
    // still try (from the same address, under its 30).
    assert!(session_of(&app, Some(&admin)).await["user"].is_object());
    let other = request(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "b@example.com", "password": "wrong password" })),
    )
    .await;
    assert_eq!(other.status_code(), 401);

    // Codes: five per user, then 429 even for a right one.
    let secret = enable(&app, &admin).await;
    for _ in 0..4 {
        let body = json!({ "code": "000000" });
        let response = post(&app, "/api/auth/totp/recovery-codes", &admin, Some(body)).await;
        assert_eq!(response.status_code(), 400);
    }
    let body = json!({ "code": totp_code(&secret, 1) });
    let response = post(&app, "/api/auth/totp/recovery-codes", &admin, Some(body)).await;
    assert_eq!(response.status_code(), 429);
}

#[tokio::test]
async fn two_factor_authentication_is_turned_off_by_the_user_or_an_admin() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let response = request(
        &app,
        Method::POST,
        "/api/admin/users",
        Some(&admin),
        Some(json!({ "email": "b@example.com", "password": "correct horse battery" })),
    )
    .await;
    assert_eq!(response.status_code(), 201);
    let user = cookie(&password_login(&app, "b@example.com").await);

    // The user turns it off with a code; a wrong one changes nothing.
    let secret = enable(&app, &user).await;
    let response = post(
        &app,
        "/api/auth/totp/disable",
        &user,
        Some(json!({ "code": "abc" })),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    let response = post(
        &app,
        "/api/auth/totp/disable",
        &user,
        Some(json!({ "code": totp_code(&secret, 1) })),
    )
    .await;
    assert_eq!(response.status_code(), 204);
    assert_eq!(
        password_login(&app, "b@example.com").await.status_code(),
        204
    );

    // An admin turns it off for someone who lost their app, not for
    // themselves; regular users cannot.
    enable(&app, &user).await;
    assert_eq!(
        password_login(&app, "b@example.com").await.status_code(),
        200
    );
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    let id = users[1]["id"].as_str().unwrap().to_string();
    assert_eq!(users[1]["totp_enabled"], json!(true));
    let own = users[0]["id"].as_str().unwrap().to_string();
    assert_eq!(reset_totp(&app, &user, &id).await.status_code(), 403);
    assert_eq!(reset_totp(&app, &admin, &own).await.status_code(), 400);
    assert_eq!(
        reset_totp(&app, &admin, "0190f5b2-7c00-7000-8000-000000000000")
            .await
            .status_code(),
        404
    );
    assert_eq!(reset_totp(&app, &admin, &id).await.status_code(), 204);
    assert_eq!(
        password_login(&app, "b@example.com").await.status_code(),
        204
    );
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    assert_eq!(users[1]["totp_enabled"], json!(false));
}

#[tokio::test]
async fn single_sign_on_only_refuses_passwords() {
    let provider = provider::spawn().await;
    let app = spawn_app_with(oidc::Env {
        issuer: Some(provider.issuer.clone()),
        client_id: Some(provider::CLIENT_ID.into()),
        only: true,
        ..oidc::Env::default()
    })
    .await;
    // The first admin is still made on /setup, with a password: nobody signs
    // in through the provider before there is one.
    let admin = admin_session(&app).await;
    let session = session_of(&app, None).await;
    assert_eq!(session["password_login"], json!(false));
    assert!(session["oidc"].is_object());

    // No password signs in, not even the right one.
    let response = password_login(&app, "admin@example.com").await;
    assert_eq!(response.status_code(), 403);
    assert_eq!(set_cookie(&response, sessions::SESSION_COOKIE), None);
    // The code step is for single sign-on too: without its cookie, a 401.
    assert_eq!(second_step(&app, None, "123456").await.status_code(), 401);

    // Passwords cannot be set; users are made without one.
    let body = json!({ "new_password": "another horse battery", "current_password": "correct horse battery" });
    assert_eq!(
        post(&app, "/api/auth/password", &admin, Some(body))
            .await
            .status_code(),
        403
    );
    let body = json!({ "email": "b@example.com", "password": "correct horse battery" });
    assert_eq!(
        post(&app, "/api/admin/users", &admin, Some(body))
            .await
            .status_code(),
        403
    );
    let body = json!({ "email": "b@example.com" });
    assert_eq!(
        post(&app, "/api/admin/users", &admin, Some(body))
            .await
            .status_code(),
        201
    );
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    let id = users[1]["id"].as_str().unwrap().to_string();
    let body = json!({ "new_password": "correct horse battery" });
    let path = format!("/api/admin/users/{id}/password");
    assert_eq!(
        post(&app, &path, &admin, Some(body)).await.status_code(),
        403
    );

    // Single sign-on signs both in.
    for email in ["admin@example.com", "b@example.com"] {
        let session = sign_in(&app, &provider, |login| {
            provider::Claims::new(&provider, email, email, &login.nonce)
        })
        .await
        .expect("signed in");
        assert_eq!(
            session_of(&app, Some(&session)).await["user"]["email"],
            json!(email)
        );
    }
}

/// A request with an API key instead of the session.
async fn with_key(
    app: &TestApp,
    method: Method,
    path: &str,
    key: &str,
    body: Option<Value>,
) -> Response {
    let mut builder = app
        .server
        .method(method, path)
        .add_header("authorization", format!("Bearer {key}"));
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    builder.await
}

async fn create_key(app: &TestApp, session: &str, scope: &str) -> Value {
    let body = json!({ "name": format!("{scope} key"), "scope": scope });
    let response = post(app, "/api/auth/api-keys", session, Some(body)).await;
    assert_eq!(response.status_code(), 201);
    json(response).await
}

#[tokio::test]
async fn api_keys_are_capped_by_their_scope() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;

    let read = create_key(&app, &admin, "read").await;
    let token = read["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("potoo_") && token.len() == 70);
    assert_eq!(read["key"]["prefix"], json!(&token[..12]));
    assert_eq!(read["key"]["scope"], json!("read"));
    let write = create_key(&app, &admin, "write").await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let admin_key = create_key(&app, &admin, "admin").await["token"]
        .as_str()
        .unwrap()
        .to_string();

    // Read: GET only.
    let site = json!({ "hostname": "example.com" });
    assert_eq!(
        with_key(&app, Method::GET, "/api/sites", &token, None)
            .await
            .status_code(),
        200
    );
    assert_eq!(
        with_key(&app, Method::POST, "/api/sites", &token, Some(site.clone()))
            .await
            .status_code(),
        403
    );
    // Write: changes, but no admin endpoints.
    assert_eq!(
        with_key(&app, Method::POST, "/api/sites", &write, Some(site))
            .await
            .status_code(),
        201
    );
    assert_eq!(
        with_key(&app, Method::GET, "/api/admin/users", &write, None)
            .await
            .status_code(),
        403
    );
    // Admin: what the admin may.
    assert_eq!(
        with_key(&app, Method::GET, "/api/admin/users", &admin_key, None)
            .await
            .status_code(),
        200
    );

    // No key reaches the account: not its keys, password or 2FA.
    for key in [&token, &write, &admin_key] {
        assert_eq!(
            with_key(&app, Method::GET, "/api/auth/api-keys", key, None)
                .await
                .status_code(),
            403
        );
        let body = json!({ "name": "more", "scope": "read" });
        assert_eq!(
            with_key(&app, Method::POST, "/api/auth/api-keys", key, Some(body))
                .await
                .status_code(),
            403
        );
        assert_eq!(
            with_key(&app, Method::POST, "/api/auth/totp/setup", key, None)
                .await
                .status_code(),
            403
        );
    }

    // Used: the list says when; the token is never listed.
    let keys =
        json(request(&app, Method::GET, "/api/auth/api-keys", Some(&admin), None).await).await;
    assert_eq!(keys.as_array().unwrap().len(), 3);
    assert!(keys[2]["last_used_at"].is_string());
    assert!(!keys.to_string().contains(&token));

    // Revoked, unknown or malformed: 401.
    let id = read["key"]["id"].as_str().unwrap().to_string();
    let path = format!("/api/auth/api-keys/{id}");
    assert_eq!(
        request(&app, Method::DELETE, &path, Some(&admin), None)
            .await
            .status_code(),
        204
    );
    assert_eq!(
        request(&app, Method::DELETE, &path, Some(&admin), None)
            .await
            .status_code(),
        404
    );
    assert_eq!(
        with_key(&app, Method::GET, "/api/sites", &token, None)
            .await
            .status_code(),
        401
    );
    assert_eq!(
        with_key(&app, Method::GET, "/api/sites", "potoo_nope", None)
            .await
            .status_code(),
        401
    );
    assert_eq!(
        with_key(&app, Method::GET, "/api/sites", "nope", None)
            .await
            .status_code(),
        401
    );

    // Bad requests.
    for body in [
        json!({ "name": "", "scope": "read" }),
        json!({ "name": "x", "scope": "owner" }),
        json!({ "name": "x", "scope": "read", "expires_in_days": 0 }),
    ] {
        assert_eq!(
            post(&app, "/api/auth/api-keys", &admin, Some(body))
                .await
                .status_code(),
            400
        );
    }
    let body = json!({ "name": "x", "scope": "read", "expires_in_days": 30 });
    let key = json(post(&app, "/api/auth/api-keys", &admin, Some(body)).await).await;
    assert!(key["key"]["expires_at"].is_string());

    // A regular user: no admin keys, and an admin key would not help.
    let body = json!({ "email": "b@example.com", "password": "correct horse battery" });
    post(&app, "/api/admin/users", &admin, Some(body)).await;
    let user = cookie(&password_login(&app, "b@example.com").await);
    let body = json!({ "name": "x", "scope": "admin" });
    assert_eq!(
        post(&app, "/api/auth/api-keys", &user, Some(body))
            .await
            .status_code(),
        403
    );
    let user_key = create_key(&app, &user, "write").await["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        with_key(&app, Method::GET, "/api/sites", &user_key, None)
            .await
            .status_code(),
        200
    );
    // A user's keys go with them.
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    let id = users[1]["id"].as_str().unwrap().to_string();
    let path = format!("/api/admin/users/{id}");
    assert_eq!(
        request(&app, Method::DELETE, &path, Some(&admin), None)
            .await
            .status_code(),
        204
    );
    assert_eq!(
        with_key(&app, Method::GET, "/api/sites", &user_key, None)
            .await
            .status_code(),
        401
    );
}

/// A single sign-on login that stops at a step: where it sent the browser,
/// and the challenge cookie.
async fn sso_step(app: &TestApp, provider: &provider::Provider, email: &str) -> (String, String) {
    let login = start_login(app).await;
    let code = provider.code_for(provider::Claims::new(provider, email, email, &login.nonce));
    let response = callback(
        app,
        &format!("code={code}&state={}", login.state),
        Some(&login.cookie),
    )
    .await;
    assert_eq!(response.status_code(), 303);
    assert_eq!(set_cookie(&response, sessions::SESSION_COOKIE), None);
    let challenge = set_cookie(&response, "potoo_2fa").expect("the challenge cookie");
    (location(&response), challenge)
}

#[tokio::test]
async fn single_sign_on_asks_for_the_code_too() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let provider = provider::spawn().await;
    configure(&app, &admin, &provider, json!({})).await;
    let secret = enable(&app, &admin).await;

    let (page, challenge) = sso_step(&app, &provider, "admin@example.com").await;
    assert_eq!(page, "/login?step=totp");
    // Not the enrolment step: that is for a user without the app.
    let response = request(
        &app,
        Method::POST,
        "/api/auth/login/totp/setup",
        Some(&challenge),
        None,
    )
    .await;
    assert_eq!(response.status_code(), 401);
    let response = second_step(&app, Some(&challenge), &totp_code(&secret, 1)).await;
    assert_eq!(response.status_code(), 204);
    let session = set_cookie(&response, sessions::SESSION_COOKIE).expect("the session cookie");
    assert_eq!(
        session_of(&app, Some(&session)).await["user"]["email"],
        json!("admin@example.com")
    );
}

#[tokio::test]
async fn totp_required_makes_everyone_set_it_up() {
    let provider = provider::spawn().await;
    let app = spawn_app_full(
        oidc::Env {
            issuer: Some(provider.issuer.clone()),
            client_id: Some(provider::CLIENT_ID.into()),
            ..oidc::Env::default()
        },
        true,
    )
    .await;
    // The session /setup made is not touched.
    let admin = admin_session(&app).await;
    assert_eq!(session_of(&app, None).await["totp_required"], json!(true));

    // The password leads to setting the app up, not to a session.
    let response = password_login(&app, "admin@example.com").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(set_cookie(&response, sessions::SESSION_COOKIE), None);
    let challenge = set_cookie(&response, "potoo_2fa").unwrap();
    assert_eq!(
        json(response).await,
        json!({ "totp_required": false, "totp_setup_required": true })
    );
    // Not the code step: there is no app yet.
    assert_eq!(
        second_step(&app, Some(&challenge), "123456")
            .await
            .status_code(),
        401
    );
    let enroll = |path: &'static str, body: Option<Value>| {
        request(&app, Method::POST, path, Some(&challenge), body)
    };
    let response = enroll(
        "/api/auth/login/totp/enable",
        Some(json!({ "code": "123456" })),
    )
    .await;
    assert_eq!(response.status_code(), 400);
    let setup = json(enroll("/api/auth/login/totp/setup", None).await).await;
    let secret = setup["secret"].as_str().unwrap().to_string();
    let body = json!({ "code": totp_code(&secret, 0) });
    let response = enroll("/api/auth/login/totp/enable", Some(body)).await;
    assert_eq!(response.status_code(), 200);
    let session = set_cookie(&response, sessions::SESSION_COOKIE).expect("the session cookie");
    assert_eq!(json(response).await["codes"].as_array().unwrap().len(), 10);
    assert_eq!(
        session_of(&app, Some(&session)).await["user"]["totp_enabled"],
        json!(true)
    );
    // Turning it on changed the user's credentials: the session /setup
    // made is signed out.
    assert_eq!(session_of(&app, Some(&admin)).await["user"], Value::Null);
    let admin = session;
    // It cannot be turned off.
    let body = json!({ "code": totp_code(&secret, 1) });
    assert_eq!(
        post(&app, "/api/auth/totp/disable", &admin, Some(body))
            .await
            .status_code(),
        403
    );

    // Single sign-on: a new user sets it up too; after an admin's reset,
    // they set it up again.
    let body = json!({ "email": "b@example.com" });
    assert_eq!(
        post(&app, "/api/admin/users", &admin, Some(body))
            .await
            .status_code(),
        201
    );
    let (page, challenge) = sso_step(&app, &provider, "b@example.com").await;
    assert_eq!(page, "/login?step=totp-setup");
    let response = request(
        &app,
        Method::POST,
        "/api/auth/login/totp/setup",
        Some(&challenge),
        None,
    )
    .await;
    let secret = json(response).await["secret"].as_str().unwrap().to_string();
    let response = request(
        &app,
        Method::POST,
        "/api/auth/login/totp/enable",
        Some(&challenge),
        Some(json!({ "code": totp_code(&secret, 0) })),
    )
    .await;
    assert_eq!(response.status_code(), 200);
    let (page, _) = sso_step(&app, &provider, "b@example.com").await;
    assert_eq!(page, "/login?step=totp");
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    let id = users[1]["id"].as_str().unwrap().to_string();
    assert_eq!(reset_totp(&app, &admin, &id).await.status_code(), 204);
    let (page, _) = sso_step(&app, &provider, "b@example.com").await;
    assert_eq!(page, "/login?step=totp-setup");
}

#[tokio::test]
async fn the_authenticator_app_can_be_replaced() {
    let app = spawn_app_full(oidc::Env::default(), true).await;
    let admin = admin_session(&app).await;
    // With TOTP_REQUIRED it cannot be turned off, but it can be replaced.
    let old = enable(&app, &admin).await;
    let key = create_key(&app, &admin, "admin").await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let body = json!({ "code": totp_code(&old, 1) });
    let response = with_key(
        &app,
        Method::POST,
        "/api/auth/totp/replace",
        &key,
        Some(body),
    )
    .await;
    assert_eq!(response.status_code(), 403);

    // Confirming needs a replacement started, which needs a current code.
    let body = json!({ "code": "123456" });
    assert_eq!(
        post(&app, "/api/auth/totp/replace/confirm", &admin, Some(body))
            .await
            .status_code(),
        400
    );
    let body = json!({ "code": "abc" });
    assert_eq!(
        post(&app, "/api/auth/totp/replace", &admin, Some(body))
            .await
            .status_code(),
        400
    );
    let body = json!({ "code": totp_code(&old, 1) });
    let response = post(&app, "/api/auth/totp/replace", &admin, Some(body)).await;
    assert_eq!(response.status_code(), 200);
    let new = json(response).await["secret"].as_str().unwrap().to_string();
    assert_ne!(new, old);

    // A wrong code from the new app changes nothing.
    let wrong = if totp_code(&new, 0) == "000000" {
        "111111"
    } else {
        "000000"
    };
    let body = json!({ "code": wrong });
    assert_eq!(
        post(&app, "/api/auth/totp/replace/confirm", &admin, Some(body))
            .await
            .status_code(),
        400
    );
    // Confirmed: the new app works, the old one does not.
    let body = json!({ "code": totp_code(&new, 0) });
    assert_eq!(
        post(&app, "/api/auth/totp/replace/confirm", &admin, Some(body))
            .await
            .status_code(),
        204
    );
    let response = password_login(&app, "admin@example.com").await;
    let challenge = set_cookie(&response, "potoo_2fa").unwrap();
    let old_code = totp_code(&old, 1);
    if old_code != totp_code(&new, 1) {
        assert_eq!(
            second_step(&app, Some(&challenge), &old_code)
                .await
                .status_code(),
            400
        );
    }
    let response = second_step(&app, Some(&challenge), &totp_code(&new, 1)).await;
    assert_eq!(response.status_code(), 204);
}

#[tokio::test]
async fn sessions_end_with_a_password_change_a_deletion_or_a_logout() {
    let app = spawn_app().await;
    let admin = admin_session(&app).await;
    let body = json!({ "email": "b@example.com", "password": "correct horse battery" });
    assert_eq!(
        post(&app, "/api/admin/users", &admin, Some(body))
            .await
            .status_code(),
        201
    );
    let laptop = cookie(&password_login(&app, "b@example.com").await);
    let phone = cookie(&password_login(&app, "b@example.com").await);
    assert_ne!(laptop, phone);
    let signed_in = |session: String| {
        let app = &app;
        async move { session_of(app, Some(&session)).await["user"] != Value::Null }
    };
    assert!(signed_in(laptop.clone()).await && signed_in(phone.clone()).await);

    // A new password signs the other sessions out; this one stays.
    let body = json!({ "current_password": "correct horse battery", "new_password": "another horse battery" });
    assert_eq!(
        post(&app, "/api/auth/password", &laptop, Some(body))
            .await
            .status_code(),
        204
    );
    assert!(signed_in(laptop.clone()).await);
    assert!(!signed_in(phone.clone()).await);

    // Signing out ends the session.
    assert_eq!(
        post(&app, "/api/auth/logout", &laptop, None)
            .await
            .status_code(),
        204
    );
    assert!(!signed_in(laptop).await);

    // A deleted user is signed out everywhere.
    let body = json!({ "email": "b@example.com", "password": "another horse battery" });
    let session = cookie(&request(&app, Method::POST, "/api/auth/login", None, Some(body)).await);
    assert!(signed_in(session.clone()).await);
    let users =
        json(request(&app, Method::GET, "/api/admin/users", Some(&admin), None).await).await;
    let path = format!("/api/admin/users/{}", users[1]["id"].as_str().unwrap());
    assert_eq!(
        request(&app, Method::DELETE, &path, Some(&admin), None)
            .await
            .status_code(),
        204
    );
    assert!(!signed_in(session).await);
}

/// A POST as a browser on another origin would send it.
async fn from_site(app: &TestApp, path: &str, session: Option<&str>, site: &str) -> Response {
    let mut builder = app
        .server
        .post(path)
        .add_header("sec-fetch-site", site)
        .json(&json!({}));
    if let Some(session) = session {
        builder = builder.add_header("cookie", session);
    }
    builder.await
}

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
