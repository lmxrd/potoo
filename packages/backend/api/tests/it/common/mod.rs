//! What the integration tests share: the app on axum-test's TestServer, over a real local
//! socket (the app reads the client's address from the connection), against
//! an in-memory database.

pub use api::{db, geo, oidc, sessions, state::AppState};
pub use axum::http::Method;
pub use axum_test::{TestResponse as Response, TestServer};
pub use serde_json::{Value, json};
pub use std::sync::Arc;

pub const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

pub struct TestApp {
    pub server: TestServer,
    /// Where the server listens, e.g. `http://127.0.0.1:41234`.
    pub base: String,
}

pub async fn spawn_app() -> TestApp {
    spawn_app_with(oidc::Env::default()).await
}

/// The app, with OIDC_* environment variables as given.
pub async fn spawn_app_with(oidc_env: oidc::Env) -> TestApp {
    spawn_app_full(oidc_env, false).await
}

/// The app, with OIDC_* environment variables and TOTP_REQUIRED as given.
pub async fn spawn_app_full(oidc_env: oidc::Env, totp_required: bool) -> TestApp {
    spawn(Options {
        oidc: oidc_env,
        totp_required,
        ..Options::default()
    })
    .await
}

/// What a test app is started with.
pub struct Options {
    pub oidc: oidc::Env,
    pub totp_required: bool,
    /// `PUBLIC_URL`; https makes every cookie Secure and `__Host-`.
    pub public_url: Option<String>,
    /// The login limits: large ones, but for the tests of the limits.
    pub limits: api::limits::Limits,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            oidc: oidc::Env::default(),
            totp_required: false,
            public_url: None,
            limits: api::limits::Limits::generous(),
        }
    }
}

pub async fn spawn(options: Options) -> TestApp {
    api::install_tls_provider();
    let db = db::open(":memory:").await;
    let state = AppState {
        ingest: api::ingest::Ingest::spawn(db.clone()).0,
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
    };
    let server = TestServer::builder().http_transport().build(
        api::routes::api(state.clone())
            .with_state(state)
            .split_for_parts()
            .0
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    );
    let base = server
        .server_address()
        .expect("an http transport")
        .to_string()
        .trim_end_matches('/')
        .to_string();
    TestApp { server, base }
}

/// Now, give or take some hours, as the API takes times.
pub fn iso_from_now(hours: i64) -> String {
    use jiff::ToSpan;
    format!("{:.3}", jiff::Timestamp::now() + hours.hours())
}

/// The session cookie of a response.
pub fn cookie(response: &Response) -> String {
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

pub async fn request(
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

pub async fn json(response: Response) -> Value {
    response.json()
}

/// The pageviews of the test site in a window, under a filter.
pub async fn pageviews(app: &TestApp, session: &str, window: &(String, String), q: &str) -> Value {
    let url = format!(
        "/api/stats?sites=blog.example.com&from={}&to={}&q={q}",
        window.0, window.1
    );
    json(request(app, Method::GET, &url, Some(session), None).await).await["pageviews"].clone()
}

pub async fn post_raw(app: &TestApp, path: &str, content_type: &str, body: &str) -> Response {
    app.server
        .post(path)
        .add_header("user-agent", UA)
        .add_header("content-type", content_type)
        .bytes(body.to_string().into())
        .await
}

pub async fn post_csv(app: &TestApp, session: &str, content_type: &str, body: &str) -> Response {
    app.server
        .post("/api/import")
        .add_header("cookie", session)
        .add_header("content-type", content_type)
        .bytes(body.to_string().into())
        .await
}

// Single sign-on. The provider is a stand-in served by these tests: its
// discovery document, its keys (JWKS) and its token endpoint, which answers
// a code with the ID token the test signed for it.

pub mod provider {
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
        let key = Arc::new(signing_key(
            include_str!("../fixtures/oidc-key.pem"),
            "first",
        ));
        let other_key = Arc::new(signing_key(
            include_str!("../fixtures/oidc-other-key.pem"),
            "second",
        ));
        let other_key_same_id = Arc::new(signing_key(
            include_str!("../fixtures/oidc-other-key.pem"),
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
            let code = api::random::hex::<8>();
            self.codes
                .lock()
                .unwrap()
                .insert(code.clone(), id_token.to_string());
            code
        }
    }
}

/// A cookie value as a browser sends it.
pub fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The value of a cookie a response sets, as a `cookie` header value.
pub fn set_cookie(response: &Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .find(|pair| pair.starts_with(&format!("{name}=")) && pair.len() > name.len() + 1)
        .map(String::from)
}

pub fn location(response: &Response) -> String {
    response.headers()["location"].to_str().unwrap().to_string()
}

/// A login in progress: where the app sent the browser.
pub struct Login {
    pub url: reqwest::Url,
    pub state: String,
    pub nonce: String,
    /// The login cookie, as a `cookie` header value.
    pub cookie: String,
}

pub async fn start_login(app: &TestApp) -> Login {
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
pub async fn callback(app: &TestApp, query: &str, cookie: Option<&str>) -> Response {
    let mut request = app.server.get(&format!("/api/auth/oidc/callback?{query}"));
    if let Some(cookie) = cookie {
        request = request.add_header("cookie", cookie);
    }
    request.await
}

/// A whole login with these claims (the nonce is the login's): the
/// session cookie, or where the browser was sent instead.
pub async fn sign_in(
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
pub async fn admin_session(app: &TestApp) -> String {
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

pub async fn save_oidc(app: &TestApp, session: &str, body: Value) -> Response {
    request(
        app,
        Method::PUT,
        "/api/admin/oidc",
        Some(session),
        Some(body),
    )
    .await
}

pub async fn configure(app: &TestApp, session: &str, provider: &provider::Provider, extra: Value) {
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
pub async fn session_of(app: &TestApp, session: Option<&str>) -> Value {
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

/// The whole `set-cookie` header for a cookie, attributes included.
pub fn set_cookie_header(response: &Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(String::from)
}

/// The first admin of an app with an https PUBLIC_URL: its session cookie
/// is `__Host-potoo_session` (as a `cookie` header value).
pub async fn secure_admin_session(app: &TestApp) -> String {
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

/// A provider whose discovery document says this, at an address of its
/// own on this machine.
pub async fn spawn_discovery(document: impl Fn(&str) -> String + Send + 'static) -> String {
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

/// The code an authenticator app shows now, `offset` steps away.
pub fn totp_code(secret: &str, offset: i64) -> String {
    let step = (api::totp::current_step() as i64 + offset) as u64;
    api::totp::code_at(secret, step).unwrap()
}

/// Sign in with a password: the response, and the cookie it set (the session,
/// or the challenge of the second step).
pub async fn password_login(app: &TestApp, email: &str) -> Response {
    request(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": email, "password": "correct horse battery" })),
    )
    .await
}

pub async fn second_step(app: &TestApp, challenge: Option<&str>, code: &str) -> Response {
    request(
        app,
        Method::POST,
        "/api/auth/login/totp",
        challenge,
        Some(json!({ "code": code })),
    )
    .await
}

pub async fn post(app: &TestApp, path: &str, session: &str, body: Option<Value>) -> Response {
    request(app, Method::POST, path, Some(session), body).await
}

/// Turn two-factor authentication on for the user of a session: the secret.
pub async fn enable(app: &TestApp, session: &str) -> String {
    let setup = json(post(app, "/api/auth/totp/setup", session, None).await).await;
    let secret = setup["secret"].as_str().unwrap().to_string();
    let body = json!({ "code": totp_code(&secret, 0) });
    let response = post(app, "/api/auth/totp/enable", session, Some(body)).await;
    assert_eq!(response.status_code(), 200);
    secret
}

pub async fn reset_totp(app: &TestApp, session: &str, id: &str) -> Response {
    let path = format!("/api/admin/users/{id}/totp");
    request(app, Method::DELETE, &path, Some(session), None).await
}

/// A request with an API key instead of the session.
pub async fn with_key(
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

pub async fn create_key(app: &TestApp, session: &str, scope: &str) -> Value {
    let body = json!({ "name": format!("{scope} key"), "scope": scope });
    let response = post(app, "/api/auth/api-keys", session, Some(body)).await;
    assert_eq!(response.status_code(), 201);
    json(response).await
}

/// A single sign-on login that stops at a step: where it sent the browser,
/// and the challenge cookie.
pub async fn sso_step(
    app: &TestApp,
    provider: &provider::Provider,
    email: &str,
) -> (String, String) {
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

/// A POST as a browser on another origin would send it.
pub async fn from_site(app: &TestApp, path: &str, session: Option<&str>, site: &str) -> Response {
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
