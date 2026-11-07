//! Two-factor authentication.

use crate::common::*;

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
