//! Importing hits.

use crate::common::*;

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
