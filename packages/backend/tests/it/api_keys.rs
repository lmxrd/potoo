//! API keys and their scopes.

use crate::common::*;

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
