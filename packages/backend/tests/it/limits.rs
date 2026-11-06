//! How often sign-ins and codes may be tried.

use crate::common::*;

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
