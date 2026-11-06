//! Single sign-on.

use crate::common::*;

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
