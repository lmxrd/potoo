//! The app end to end: setup, users, sites, collection, stats and exports; the tracker, bots, and the API's routes and bodies.

use crate::common::*;

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
