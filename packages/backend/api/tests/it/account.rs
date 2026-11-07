//! Sessions: when they end.

use crate::common::*;

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
