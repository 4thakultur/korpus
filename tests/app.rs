//! Route-level tests against a fresh in-memory database with real migrations.
//! External providers are replaced with a fake; no test touches the network.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use korpus::config::Config;
use korpus::datasets::{self, Dataset, DatasetFood};
use korpus::i18n::Lang;
use korpus::nutrition::{Basis, Nutrients};
use korpus::providers::{
    BoxFuture, FoodProvider, ProviderError, ProviderFood, ProviderPage, Providers,
};
use korpus::web::{AppState, Guard, router};
use sqlx::SqlitePool;
use tower::ServiceExt;

struct FakeProvider {
    fail: Mutex<bool>,
    /// Language of the most recent request, to check what Korpus asks for.
    last_lang: Mutex<Option<Lang>>,
}

impl FakeProvider {
    fn record(&self, lang: Lang) {
        *self.last_lang.lock().unwrap() = Some(lang);
    }
}

/// The product name the fake database holds for each language, like Open
/// Food Facts' `product_name_xx` fields.
fn granola_name(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Remote granola",
        Lang::Fr => "Granola croustillant",
        Lang::Es => "Granola crujiente",
    }
}

fn fake_food(code: &str, name: &str) -> ProviderFood {
    ProviderFood {
        source: "openfoodfacts",
        provider_id: code.to_string(),
        name: name.to_string(),
        names: Vec::new(),
        brand: Some("Fake Brand".into()),
        barcode: Some(code.to_string()),
        basis: Basis::Per100g,
        basis_inferred: true,
        nutrients: Nutrients {
            energy_kcal: Some(400.0),
            protein_g: Some(8.0),
            carbs_g: Some(70.0),
            fat_g: None,
            ..Default::default()
        },
        energy_from_kj: false,
        serving_amount: Some(30.0),
        serving_label: Some("30 g".into()),
        source_updated_at: Some("2025-05-01".into()),
        rejected: vec![],
        salt_from_sodium: false,
        carbs_include_fiber: false,
        trace_values: vec![],
    }
}

impl FoodProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "openfoodfacts"
    }
    fn name(&self) -> &'static str {
        "Fake Food Facts"
    }
    fn attribution(&self, _lang: Lang) -> &'static str {
        "Fake attribution."
    }
    fn attribution_url(&self) -> &'static str {
        "https://example.invalid"
    }
    fn search<'a>(
        &'a self,
        q: &'a str,
        _page: usize,
        lang: Lang,
    ) -> BoxFuture<'a, Result<ProviderPage, ProviderError>> {
        Box::pin(async move {
            self.record(lang);
            if *self.fail.lock().unwrap() {
                return Err(ProviderError::Timeout);
            }
            Ok(ProviderPage {
                foods: vec![
                    fake_food("4000000000017", &format!("Remote {q}")),
                    fake_food("4000000000024", "Remote other"),
                ],
                has_more: false,
            })
        })
    }
    fn get<'a>(
        &'a self,
        id: &'a str,
        lang: Lang,
    ) -> BoxFuture<'a, Result<Option<ProviderFood>, ProviderError>> {
        Box::pin(async move {
            self.record(lang);
            if *self.fail.lock().unwrap() {
                return Err(ProviderError::Unavailable("down".into()));
            }
            Ok((id == "4000000000017").then(|| ProviderFood {
                names: Lang::ALL
                    .into_iter()
                    .map(|l| (l, granola_name(l).to_string()))
                    .collect(),
                ..fake_food(id, granola_name(lang))
            }))
        })
    }
    fn lookup_barcode<'a>(
        &'a self,
        code: &'a str,
        lang: Lang,
    ) -> BoxFuture<'a, Result<Option<ProviderFood>, ProviderError>> {
        self.get(code, lang)
    }
}

struct App {
    router: Router,
    db: SqlitePool,
    provider: Arc<FakeProvider>,
    auth: Arc<Guard>,
    /// `Cookie` value of a logged-in session, sent with every request
    /// unless the test clears it.
    session: Option<String>,
}

const USERNAME: &str = "tester";
const PASSWORD: &str = "correct horse battery";

async fn app() -> App {
    let db = korpus::db::connect_memory().await.unwrap();
    let provider = Arc::new(FakeProvider {
        fail: Mutex::new(false),
        last_lang: Mutex::new(None),
    });
    let config = Config::from_lookup(|_| None).unwrap();
    let auth = Arc::new(Guard::default());
    let router = router(AppState {
        db: db.clone(),
        providers: Arc::new(Providers::new(vec![provider.clone()])),
        config: Arc::new(config),
        auth: auth.clone(),
    });
    korpus::auth::create_account(&db, USERNAME, PASSWORD)
        .await
        .unwrap();
    let token = korpus::auth::create_session(&db, false).await.unwrap();
    App {
        router,
        db,
        provider,
        auth,
        session: Some(format!("korpus_session={token}")),
    }
}

/// An installation nobody has logged into yet.
async fn app_without_login() -> App {
    let a = app().await;
    korpus::auth::reset(&a.db).await.unwrap();
    a.as_visitor(None)
}

struct Resp {
    status: StatusCode,
    location: Option<String>,
    set_cookie: Vec<String>,
    body: String,
}

impl App {
    async fn send(&self, mut req: Request<Body>) -> Resp {
        if let Some(session) = &self.session {
            let cookie = match req.headers().get(header::COOKIE) {
                Some(c) => format!("{}; {session}", c.to_str().unwrap()),
                None => session.clone(),
            };
            req.headers_mut()
                .insert(header::COOKIE, cookie.parse().unwrap());
        }
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let location = res
            .headers()
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string());
        let set_cookie = res
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        let bytes = to_bytes(res.into_body(), 10 * 1024 * 1024).await.unwrap();
        Resp {
            status,
            location,
            set_cookie,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&self, uri: &str) -> Resp {
        self.send(
            Request::get(uri)
                .header(header::HOST, "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    async fn post(&self, uri: &str, form: &str) -> Resp {
        self.send(
            Request::post(uri)
                .header(header::HOST, "localhost")
                .header(header::ORIGIN, "http://localhost")
                .header("sec-fetch-site", "same-origin")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
    }

    /// Posts files as `multipart/form-data`, like the Settings upload forms.
    async fn upload(&self, uri: &str, files: &[(&str, &[u8])]) -> Resp {
        let boundary = "korpus-test-boundary";
        let mut body = Vec::new();
        for (name, bytes) in files {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        self.send(
            Request::post(uri)
                .header(header::HOST, "localhost")
                .header(header::ORIGIN, "http://localhost")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
    }

    async fn setup(&self) {
        let r = self.post("/setup", "unit_system=metric&timezone=UTC").await;
        assert_eq!(r.status, StatusCode::SEE_OTHER);
    }

    async fn create_oats(&self) -> i64 {
        let r = self
            .post(
                "/foods",
                "name=Rolled+oats&brand=&barcode=&basis=g&energy=370&energy_unit=kcal&protein=13&carbs=60&fat=7&fiber=10",
            )
            .await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
        let loc = r.location.unwrap();
        loc.trim_start_matches("/foods/")
            .split('?')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    async fn day_kcal(&self, date: &str) -> String {
        let r = self.get(&format!("/diary?date={date}")).await;
        assert_eq!(r.status, StatusCode::OK);
        let marker = "<p class=\"big-number\"><strong>";
        let start = r.body.find(marker).expect("summary present") + marker.len();
        r.body[start..].split('<').next().unwrap().to_string()
    }
}

fn entry_id(db_rows: &[(i64,)]) -> i64 {
    db_rows[0].0
}

#[tokio::test]
async fn health_endpoints() {
    let a = app().await;
    assert_eq!(a.get("/healthz").await.status, StatusCode::OK);
    let r = a.get("/readyz").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, "ready");
}

#[tokio::test]
async fn first_run_goes_through_setup() {
    let a = app().await;
    let r = a.get("/").await;
    assert_eq!(r.location.as_deref(), Some("/setup"));
    assert_eq!(a.get("/diary").await.location.as_deref(), Some("/setup"));
    assert_eq!(a.get("/setup").await.status, StatusCode::OK);

    let bad = a
        .post("/setup", "unit_system=metric&timezone=Nowhere%2FLand")
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(bad.body.contains("time zone name"));

    let r = a
        .post("/setup", "unit_system=imperial&timezone=Europe%2FBerlin")
        .await;
    assert_eq!(r.location.as_deref(), Some("/goals?setup=1"));
    assert_eq!(a.get("/").await.location.as_deref(), Some("/diary"));
    let diary = a.get("/diary").await;
    assert_eq!(diary.status, StatusCode::OK);
    assert!(diary.body.contains("Add food"));
    assert!(diary.body.contains("No daily target set"));
}

#[tokio::test]
async fn security_headers_present() {
    let a = app().await;
    let res = a
        .router
        .clone()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let h = res.headers();
    assert!(h.get(header::CONTENT_SECURITY_POLICY).is_some());
    assert_eq!(h.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
    assert_eq!(h.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");
    assert_eq!(h.get("cross-origin-opener-policy").unwrap(), "same-origin");
    assert_eq!(
        h.get("cross-origin-resource-policy").unwrap(),
        "same-origin"
    );
}

#[tokio::test]
async fn cross_site_posts_are_rejected() {
    let a = app().await;
    let cross_site = a
        .send(
            Request::post("/setup")
                .header(header::HOST, "localhost")
                .header("sec-fetch-site", "cross-site")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("unit_system=metric&timezone=UTC"))
                .unwrap(),
        )
        .await;
    assert_eq!(cross_site.status, StatusCode::FORBIDDEN);

    let wrong_origin = a
        .send(
            Request::post("/setup")
                .header(header::HOST, "localhost")
                .header(header::ORIGIN, "https://evil.example")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("unit_system=metric&timezone=UTC"))
                .unwrap(),
        )
        .await;
    assert_eq!(wrong_origin.status, StatusCode::FORBIDDEN);

    let p = korpus::profile::load(&a.db).await.unwrap();
    assert!(!p.setup_complete, "rejected requests must not change data");
}

/// DNS rebinding: a page on evil.example whose name now resolves to this
/// server is "same origin" for the browser, so only the host name gives it
/// away.
#[tokio::test]
async fn requests_for_unknown_host_names_are_refused() {
    let a = app().await;
    a.setup().await;
    let req = |host: &str, path: &str| {
        Request::get(path)
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap()
    };
    for path in ["/diary", "/settings/export.json", "/settings/backup.sqlite"] {
        let r = a.send(req("evil.example:9874", path)).await;
        assert_eq!(r.status, StatusCode::MISDIRECTED_REQUEST, "{path}");
        assert!(r.body.contains("KORPUS_ALLOWED_HOSTS"));
    }
    let rebound_post = a
        .send(
            Request::post("/weight")
                .header(header::HOST, "evil.example")
                .header(header::ORIGIN, "http://evil.example")
                .header("sec-fetch-site", "same-origin")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("date=2026-01-01&weight=70"))
                .unwrap(),
        )
        .await;
    assert_eq!(rebound_post.status, StatusCode::MISDIRECTED_REQUEST);
    assert!(korpus::weight::latest(&a.db).await.unwrap().is_none());

    // A forged forwarded host does not get past the check either.
    let forwarded = a
        .send(
            Request::get("/diary")
                .header(header::HOST, "localhost")
                .header("x-forwarded-host", "evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(forwarded.status, StatusCode::MISDIRECTED_REQUEST);

    for host in [
        "localhost:9874",
        "127.0.0.1:9874",
        "192.168.1.20",
        "[::1]:9874",
    ] {
        assert_eq!(
            a.send(req(host, "/diary")).await.status,
            StatusCode::OK,
            "{host}"
        );
    }
}

#[tokio::test]
async fn configured_host_names_are_answered() {
    let db = korpus::db::connect_memory().await.unwrap();
    let config =
        Config::from_lookup(|k| (k == "KORPUS_ALLOWED_HOSTS").then(|| "korpus.lan".into()))
            .unwrap();
    let router = router(AppState {
        db,
        providers: Arc::new(Providers::new(vec![])),
        config: Arc::new(config),
        auth: Default::default(),
    });
    let status = |host: &'static str| {
        let router = router.clone();
        async move {
            router
                .oneshot(
                    Request::get("/healthz")
                        .header(header::HOST, host)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
    };
    assert_eq!(status("korpus.lan:9874").await, StatusCode::OK);
    assert_eq!(status("Korpus.LAN").await, StatusCode::OK);
    assert_eq!(status("other.lan").await, StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn manual_food_validation_and_creation() {
    let a = app().await;
    a.setup().await;
    let bad = a
        .post("/foods", "name=&basis=g&energy=-5&protein=abc&carbs=&fat=1")
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(bad.body.contains("Enter a name."));
    assert!(bad.body.contains("aria-invalid=\"true\""));
    assert!(bad.body.contains("Please correct the following"));

    let id = a.create_oats().await;
    let detail = a.get(&format!("/foods/{id}")).await;
    assert_eq!(detail.status, StatusCode::OK);
    assert!(detail.body.contains("Rolled oats"));
    assert!(detail.body.contains("Created by you"));
    assert!(detail.body.contains("370 kcal"));

    // Barcodes are unique when supplied.
    let first = a
        .post(
            "/foods",
            "name=A&basis=g&barcode=12345678&energy=1&protein=1&carbs=1&fat=1",
        )
        .await;
    assert_eq!(first.status, StatusCode::SEE_OTHER);
    let dup = a
        .post(
            "/foods",
            "name=B&basis=g&barcode=12345678&energy=1&protein=1&carbs=1&fat=1",
        )
        .await;
    assert_eq!(dup.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(dup.body.contains("already uses this barcode"));
}

#[tokio::test]
async fn log_change_and_delete_entries() {
    let a = app().await;
    a.setup().await;
    let food = a.create_oats().await;

    let r = a
        .post(
            "/diary/entries",
            &format!("food_id={food}&date=2026-03-10&meal=breakfast&amount=50"),
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert!(r.location.unwrap().starts_with("/diary?date=2026-03-10"));
    assert_eq!(a.day_kcal("2026-03-10").await, "185");
    // Same food twice in one day is allowed.
    a.post(
        "/diary/entries",
        &format!("food_id={food}&date=2026-03-10&meal=snacks&amount=10,5"),
    )
    .await;
    assert_eq!(a.day_kcal("2026-03-10").await, "224");
    // Other dates are unaffected.
    assert_eq!(a.day_kcal("2026-03-11").await, "0");

    let rows: Vec<(i64,)> = sqlx::query_as("SELECT id FROM diary_entries WHERE meal = 'breakfast'")
        .fetch_all(&a.db)
        .await
        .unwrap();
    let id = entry_id(&rows);

    let invalid = a
        .post(
            &format!("/diary/entries/{id}"),
            "date=2026-03-10&meal=breakfast&amount=0",
        )
        .await;
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(invalid.body.contains("greater than zero"));

    let r = a
        .post(
            &format!("/diary/entries/{id}"),
            "date=2026-03-10&meal=lunch&amount=100",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(a.day_kcal("2026-03-10").await, "409");

    let r = a.post(&format!("/diary/entries/{id}/delete"), "").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(a.day_kcal("2026-03-10").await, "39");
    assert_eq!(
        a.post(&format!("/diary/entries/{id}/delete"), "")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn invalid_entry_input_is_rejected() {
    let a = app().await;
    a.setup().await;
    let food = a.create_oats().await;
    for body in [
        format!("food_id={food}&date=2026-02-30&meal=lunch&amount=10"),
        format!("food_id={food}&date=2026-02-01&meal=brunch&amount=10"),
        format!("food_id={food}&date=2026-02-01&meal=lunch&amount=-1"),
        format!("food_id={food}&date=2026-02-01&meal=lunch&amount=NaN"),
        format!("food_id={food}&date=2026-02-01&meal=lunch&amount=99999"),
    ] {
        let r = a.post("/diary/entries", &body).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    assert_eq!(
        a.post(
            "/diary/entries",
            "food_id=999&date=2026-02-01&meal=lunch&amount=1"
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        a.get("/diary?date=not-a-date").await.status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn history_is_stable_when_food_changes_or_is_deleted() {
    let a = app().await;
    a.setup().await;
    let food = a.create_oats().await;
    a.post(
        "/diary/entries",
        &format!("food_id={food}&date=2026-01-05&meal=dinner&amount=100"),
    )
    .await;
    assert_eq!(a.day_kcal("2026-01-05").await, "370");

    let r = a
        .post(
            &format!("/foods/{food}"),
            "name=Rolled+oats+v2&basis=g&energy=800&energy_unit=kcal&protein=1&carbs=1&fat=1",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert_eq!(
        a.day_kcal("2026-01-05").await,
        "370",
        "edit must not rewrite history"
    );
    let day = a.get("/diary?date=2026-01-05").await;
    assert!(
        day.body.contains("Rolled oats<"),
        "entry keeps the logged name"
    );

    let confirm = a.get(&format!("/foods/{food}/delete")).await;
    assert!(confirm.body.contains("used in 1 diary entry"));
    let r = a.post(&format!("/foods/{food}/delete"), "").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(
        a.get(&format!("/foods/{food}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        a.day_kcal("2026-01-05").await,
        "370",
        "delete must not rewrite history"
    );
}

#[tokio::test]
async fn weight_log_and_units() {
    let a = app().await;
    a.post("/setup", "unit_system=imperial&timezone=UTC").await;

    let r = a
        .post("/weight", "date=2026-01-01&weight=200&note=start")
        .await;
    assert_eq!(r.location.as_deref(), Some("/weight?msg=weight_saved"));
    let kg: f64 = sqlx::query_scalar("SELECT weight_kg FROM weight_entries")
        .fetch_one(&a.db)
        .await
        .unwrap();
    assert!((kg - 90.718474).abs() < 1e-6, "stored canonically in kg");

    let r = a.post("/weight", "date=2026-01-01&weight=199,5").await;
    assert_eq!(r.location.as_deref(), Some("/weight?msg=weight_replaced"));
    a.post("/weight", "date=2026-01-08&weight=198").await;

    let page = a.get("/weight").await;
    assert!(page.body.contains("199.5"));
    assert!(
        page.body.contains("<svg"),
        "chart rendered with two entries"
    );
    assert!(page.body.contains("-1.5"));

    for bad in [
        "date=2026-01-02&weight=0",
        "date=2026-01-02&weight=abc",
        "date=x&weight=150",
    ] {
        assert_eq!(
            a.post("/weight", bad).await.status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM weight_entries ORDER BY entry_date")
        .fetch_all(&a.db)
        .await
        .unwrap();
    let clash = a
        .post(&format!("/weight/{}", ids[0]), "date=2026-01-08&weight=150")
        .await;
    assert_eq!(clash.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(clash.body.contains("already a weight for this date"));

    let r = a.post(&format!("/weight/{}/delete", ids[0]), "").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM weight_entries")
        .fetch_one(&a.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn goals_estimate_custom_and_safety() {
    let a = app().await;
    a.setup().await;

    let r = a
        .post(
            "/goals",
            "target_mode=estimate&sex=male&birth_date=1990-01-01&height=180&weight=80&activity=sedentary&goal=maintain",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let g = korpus::goals::load(&a.db).await.unwrap();
    assert_eq!(g.target_source.as_deref(), Some("estimate"));
    assert!(g.daily_kcal_target.unwrap() > 1500.0);
    assert!(a.get("/diary").await.body.contains("remaining"));

    // Logging a new weight does not silently change the target.
    let before = g.daily_kcal_target;
    a.post("/weight", "date=2026-01-01&weight=70").await;
    assert_eq!(
        korpus::goals::load(&a.db).await.unwrap().daily_kcal_target,
        before
    );

    let missing = a.post("/goals", "target_mode=estimate&goal=lose").await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);

    let minor = a
        .post(
            "/goals",
            "target_mode=estimate&sex=female&birth_date=2015-01-01&height=150&weight=45&activity=light&goal=lose",
        )
        .await;
    assert_eq!(minor.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(minor.body.contains("under 18"));

    let unsafe_target = a
        .post("/goals", "target_mode=custom&custom_target=300")
        .await;
    assert_eq!(unsafe_target.status, StatusCode::UNPROCESSABLE_ENTITY);

    let low = a
        .post("/goals", "target_mode=custom&custom_target=1000")
        .await;
    assert_eq!(low.status, StatusCode::OK);
    assert!(low.body.contains("below 1200 kcal"));

    let none = a.post("/goals", "target_mode=none&protein_goal=120").await;
    assert_eq!(none.status, StatusCode::SEE_OTHER);
    let g = korpus::goals::load(&a.db).await.unwrap();
    assert_eq!(g.daily_kcal_target, None);
    assert_eq!(g.protein_goal_g, Some(120.0));
}

#[tokio::test]
async fn search_local_and_remote_with_import() {
    let a = app().await;
    a.setup().await;
    a.create_oats().await;

    let page = a.get("/search?q=oats").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Rolled oats"));
    assert!(page.body.contains("data-src=\"/search/remote?q=oats"));

    // Saved foods and database results form a single list.
    let both = a.get("/search?q=oats&remote=1").await;
    assert_eq!(both.body.matches("<ul class=\"results\"").count(), 1);
    let list = &both.body[both.body.find("id=\"result-list\"").unwrap()..];
    let list = &list[..list.find("</ul>").unwrap()];
    assert!(list.contains("Rolled oats") && list.contains("Remote oats"));
    assert!(
        list.find("Rolled oats") < list.find("Remote oats"),
        "saved foods first"
    );

    // Accent folding and partial matches.
    let page = a.get("/search?q=ROLL").await;
    assert!(page.body.contains("Rolled oats"));
    let none = a.get("/search?q=zzzz").await;
    assert!(none.body.contains("No results."));
    assert!(none.body.contains("/foods/new?name=zzzz"));

    // Without the fetch header the fragment URL falls back to the full page.
    let r = a.get("/search/remote?q=granola").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let inline = a
        .get("/search?q=granola&remote=1&date=2026-04-01&meal=lunch")
        .await;
    assert!(inline.body.contains("Remote granola"));
    assert!(inline.body.contains("Fake attribution."));
    assert!(
        inline.body.contains("Missing: Fat"),
        "missing values are flagged"
    );

    let r = a
        .post(
            "/foods/import",
            "source=openfoodfacts&provider_id=4000000000017&date=2026-04-01&meal=lunch",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let loc = r.location.unwrap();
    assert!(loc.contains("date=2026-04-01&meal=lunch"), "{loc}");
    let detail = a.get(&loc).await;
    assert!(detail.body.contains("Remote granola"));
    assert!(detail.body.contains("Missing values: Fat"));
    assert!(detail.body.contains("Open Food Facts"));

    // Importing again reuses the same local food.
    let again = a
        .post(
            "/foods/import",
            "source=openfoodfacts&provider_id=4000000000017",
        )
        .await;
    assert_eq!(
        again.location.unwrap().split('?').next(),
        loc.split('?').next()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods")
        .fetch_one(&a.db)
        .await
        .unwrap();
    assert_eq!(count, 2);

    // Already-imported products are listed once, as the saved food.
    let inline = a.get("/search?q=granola&remote=1").await;
    assert!(inline.body.contains("Remote granola"));
    assert!(!inline.body.contains("value=\"4000000000017\""));

    // Exact barcode search finds the local food.
    let by_code = a.get("/search?q=4000000000017").await;
    assert!(by_code.body.contains("Remote granola"));
}

#[tokio::test]
async fn provider_outage_degrades_gracefully() {
    let a = app().await;
    a.setup().await;
    a.create_oats().await;
    *a.provider.fail.lock().unwrap() = true;

    let page = a.get("/search?q=oats&remote=1").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(
        page.body.contains("Rolled oats"),
        "local results still shown"
    );
    assert!(page.body.contains("could not be searched"));
    assert!(page.body.contains("Your own foods still work."));

    let r = a
        .post(
            "/foods/import",
            "source=openfoodfacts&provider_id=4000000000017",
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY);
    assert!(r.body.contains("Create food manually"));

    let unknown = a.post("/foods/import", "source=nope&provider_id=1").await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn export_import_round_trip_and_backup() {
    let a = app().await;
    a.setup().await;
    let food = a.create_oats().await;
    a.post(
        "/diary/entries",
        &format!("food_id={food}&date=2026-05-01&meal=lunch&amount=80"),
    )
    .await;
    a.post("/weight", "date=2026-05-01&weight=72").await;
    a.post("/goals", "target_mode=custom&custom_target=2100")
        .await;

    let export = a.get("/settings/export.json").await;
    assert_eq!(export.status, StatusCode::OK);

    let csv = a.get("/settings/diary.csv").await;
    assert!(csv.body.starts_with("date,meal,food"));
    assert!(csv.body.contains("2026-05-01,lunch,Rolled oats"));

    let parsed: korpus::backup::Export = serde_json::from_str(&export.body).unwrap();
    let b = app().await;
    let summary = korpus::backup::import(&b.db, &parsed, Lang::En)
        .await
        .unwrap();
    assert_eq!(summary.foods, 1);
    assert_eq!(summary.diary_entries, 1);
    assert_eq!(summary.weight_entries, 1);
    assert_eq!(b.day_kcal("2026-05-01").await, "296");
    assert_eq!(
        korpus::goals::load(&b.db).await.unwrap().daily_kcal_target,
        Some(2100.0)
    );

    // Importing into a non-empty installation is refused and changes nothing.
    assert!(
        korpus::backup::import(&b.db, &parsed, Lang::En)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM diary_entries")
        .fetch_one(&b.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

/// `VACUUM INTO` from an in-memory database writes to memory, so the
/// backup is tested against a database file.
#[tokio::test]
async fn backup_is_a_consistent_sqlite_file_and_leaves_nothing_behind() {
    let dir = std::env::temp_dir().join(format!("korpus-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = korpus::db::connect(&format!("sqlite://{}/korpus.db", dir.display()))
        .await
        .unwrap();
    korpus::weight::record(&db, jiff::civil::date(2026, 5, 1), 72.0, None)
        .await
        .unwrap();
    let leftovers = || {
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("korpus-backup-{}-", std::process::id()))
            })
            .count()
    };
    let bytes = korpus::backup::sqlite_snapshot(&db).await.unwrap();
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    assert_eq!(leftovers(), 0, "the temporary copy is removed");
    db.close().await;
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn invalid_import_is_rejected_atomically() {
    let a = app().await;
    a.setup().await;
    let food = a.create_oats().await;
    a.post(
        "/diary/entries",
        &format!("food_id={food}&date=2026-05-01&meal=lunch&amount=80"),
    )
    .await;
    let mut export = korpus::backup::export(&a.db).await.unwrap();
    export.diary_entries[0].amount = -3.0;

    let b = app().await;
    let err = korpus::backup::import(&b.db, &export, Lang::En)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid"));
    let foods: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods")
        .fetch_one(&b.db)
        .await
        .unwrap();
    assert_eq!(foods, 0);

    // A file from elsewhere cannot install a target the goals page refuses,
    // nor oversized text.
    let mut export = korpus::backup::export(&a.db).await.unwrap();
    export.goals.daily_kcal_target = Some(400.0);
    let err = korpus::backup::import(&b.db, &export, Lang::En)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("goal settings"), "{err}");
    let mut export = korpus::backup::export(&a.db).await.unwrap();
    export.foods[0].notes = Some("x".repeat(10_000));
    assert!(
        korpus::backup::import(&b.db, &export, Lang::En)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn local_search_ranks_exact_matches_first() {
    let a = app().await;
    for name in ["Apple juice", "Pineapple", "Apple", "Apple pie"] {
        a.post(
            "/foods",
            &format!(
                "name={}&basis=g&energy=50&protein=0&carbs=12&fat=0",
                name.replace(' ', "+")
            ),
        )
        .await;
    }
    let page = korpus::foods::search(&a.db, "apple", 1, Lang::En)
        .await
        .unwrap();
    let names: Vec<&str> = page.foods.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names[0], "Apple");
    assert_eq!(names.last(), Some(&"Pineapple"));
    assert_eq!(names.len(), 4);
    // Punctuation never reaches LIKE: it is stripped by normalization.
    let partial = korpus::foods::search(&a.db, "pple_", 1, Lang::En)
        .await
        .unwrap();
    assert_eq!(partial.foods.len(), 4);
}

#[tokio::test]
async fn local_search_prefers_foods_logged_often_and_matches_plurals() {
    let a = app().await;
    a.setup().await;
    for name in ["Yaourt nature brebis", "Yaourt nature vache"] {
        a.post(
            "/foods",
            &format!(
                "name={}&basis=g&energy=60&protein=4&carbs=5&fat=3",
                name.replace(' ', "+")
            ),
        )
        .await;
    }
    let vache: i64 = sqlx::query_scalar("SELECT id FROM foods WHERE name LIKE '%vache'")
        .fetch_one(&a.db)
        .await
        .unwrap();
    for day in ["2026-03-10", "2026-03-11"] {
        a.post(
            "/diary/entries",
            &format!("food_id={vache}&date={day}&meal=breakfast&amount=125"),
        )
        .await;
    }
    let page = korpus::foods::search(&a.db, "yaourts nature", 1, Lang::Fr)
        .await
        .unwrap();
    let names: Vec<&str> = page.foods.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Yaourt nature vache", "Yaourt nature brebis"]);
}

#[tokio::test]
async fn unknown_routes_and_ids_return_not_found() {
    let a = app().await;
    a.setup().await;
    assert_eq!(a.get("/nope").await.status, StatusCode::NOT_FOUND);
    assert_eq!(a.get("/foods/abc").await.status, StatusCode::NOT_FOUND);
    assert_eq!(a.get("/foods/0").await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        a.get("/diary/entries/5/edit").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn sqlite_backup_of_file_database() {
    let dir = std::env::temp_dir().join(format!("korpus-test-{}", std::process::id()));
    let path = dir.join("nested").join("korpus.db");
    let url = format!("sqlite://{}", path.display());
    let db = korpus::db::connect(&url).await.unwrap();
    assert!(path.exists(), "parent directories and database are created");
    sqlx::query("INSERT INTO weight_entries (entry_date, weight_kg) VALUES ('2026-01-01', 70)")
        .execute(&db)
        .await
        .unwrap();

    let bytes = korpus::backup::sqlite_snapshot(&db).await.unwrap();
    assert!(bytes.starts_with(b"SQLite format 3\0"));

    // The snapshot is a complete database that opens with migrations already applied.
    let restored = dir.join("restored.db");
    std::fs::write(&restored, &bytes).unwrap();
    let db2 = korpus::db::connect(&format!("sqlite://{}", restored.display()))
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM weight_entries")
        .fetch_one(&db2)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Reopening an existing database never loses data.
    db.close().await;
    let db = korpus::db::connect(&url).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM weight_entries")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(n, 1);
    db.close().await;
    db2.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn interface_follows_browser_until_a_language_is_chosen() {
    let a = app().await;
    a.setup().await;
    let get_with_lang = |accept: &'static str| {
        Request::get("/diary?date=2026-10-22")
            .header(header::HOST, "localhost")
            .header(header::ACCEPT_LANGUAGE, accept)
            .body(Body::empty())
            .unwrap()
    };

    let es = a.send(get_with_lang("es-ES,es;q=0.9")).await;
    assert!(es.body.contains("<html lang=\"es\" "));
    assert!(es.body.contains("Desayuno"));
    assert!(es.body.contains("jueves, 22 de octubre de 2026"));
    let en = a.get("/diary?date=2026-10-22").await;
    assert!(en.body.contains("<html lang=\"en\" "));
    assert!(en.body.contains("Breakfast"));

    // A chosen language wins over the browser.
    let r = a
        .post("/settings", "unit_system=metric&timezone=UTC&language=fr")
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let fr = a.send(get_with_lang("es-ES,es;q=0.9")).await;
    assert!(fr.body.contains("<html lang=\"fr\" "));
    assert!(fr.body.contains("Petit-déjeuner"));
    assert!(fr.body.contains("jeudi 22 octobre 2026"));
    let settings = a.get("/settings?msg=settings_saved").await;
    assert!(settings.body.contains("Paramètres enregistrés."));
    assert!(
        settings
            .body
            .contains("<option value=\"fr\" lang=\"fr\" selected>")
    );

    // Back to following the browser.
    a.post("/settings", "unit_system=metric&timezone=UTC&language=")
        .await;
    let es = a.send(get_with_lang("es")).await;
    assert!(es.body.contains("<html lang=\"es\" "));
}

#[tokio::test]
async fn invalid_language_and_errors_are_localized() {
    let a = app().await;
    a.setup().await;
    let r = a
        .post("/settings", "unit_system=metric&timezone=UTC&language=de")
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("Choose a language."));

    a.post("/settings", "unit_system=metric&timezone=UTC&language=es")
        .await;
    let r = a.post("/foods", "name=&basis=g&energy=abc").await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("Introduzca un nombre."));
    assert!(r.body.contains("Energía: debe ser un número."));
    let missing = a.get("/foods/999999").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(missing.body.contains("No encontrado"));
}

#[tokio::test]
async fn food_is_searched_in_the_interface_language_not_translated() {
    let a = app().await;
    a.setup().await;
    a.post("/settings", "unit_system=metric&timezone=UTC&language=fr")
        .await;

    let page = a.get("/search?q=granola&remote=1").await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(*a.provider.last_lang.lock().unwrap(), Some(Lang::Fr));

    let r = a
        .post(
            "/foods/import",
            "source=openfoodfacts&provider_id=4000000000017",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let detail = a.get(&r.location.unwrap()).await;
    assert!(
        detail.body.contains("Granola croustillant"),
        "the provider's French name is stored, not a translation"
    );
    assert!(detail.body.contains("Valeurs manquantes"));

    // Names the user typed are never translated.
    let id = a.create_oats().await;
    a.post("/settings", "unit_system=metric&timezone=UTC&language=es")
        .await;
    let oats = a.get(&format!("/foods/{id}")).await;
    assert!(oats.body.contains("Rolled oats"));
    assert!(oats.body.contains("Añadir al diario"));
}

#[tokio::test]
async fn saved_products_follow_the_interface_language() {
    let a = app().await;
    a.setup().await;
    let set_lang = |code: &'static str| {
        let a = &a;
        async move {
            let r = a
                .post(
                    "/settings",
                    &format!("unit_system=metric&timezone=UTC&language={code}"),
                )
                .await;
            assert_eq!(r.status, StatusCode::SEE_OTHER);
        }
    };

    set_lang("fr").await;
    let r = a
        .post(
            "/foods/import",
            "source=openfoodfacts&provider_id=4000000000017",
        )
        .await;
    let loc = r.location.unwrap();
    let food_url = loc.split('?').next().unwrap().to_string();
    let id: i64 = food_url.trim_start_matches("/foods/").parse().unwrap();
    let r = a
        .post(
            "/diary/entries",
            &format!("food_id={id}&amount=40&meal=breakfast&date=2026-10-22"),
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);

    // Switching language shows the provider's own Spanish name everywhere.
    set_lang("es").await;
    assert!(a.get(&food_url).await.body.contains("Granola crujiente"));
    let diary = a.get("/diary?date=2026-10-22").await;
    assert!(diary.body.contains("Granola crujiente"));
    assert!(!diary.body.contains("Granola croustillant"));
    // Search matches every language's name and lists the Spanish one.
    let found = a.get("/search?q=croustillant").await;
    assert!(found.body.contains("Granola crujiente"));
    set_lang("en").await;
    assert!(a.get(&food_url).await.body.contains("Remote granola"));

    // Backups keep the names.
    let export = korpus::backup::export(&a.db).await.unwrap();
    assert_eq!(export.foods[0].names.len(), 3);

    // Saving without renaming keeps them; a new name is used in every language.
    let form = |name: &str| {
        format!("name={name}&basis=g&energy=400&energy_unit=kcal&protein=8&carbs=70&fat=1")
    };
    a.post(&food_url, &form("Remote+granola")).await;
    set_lang("fr").await;
    assert!(a.get(&food_url).await.body.contains("Granola croustillant"));
    let r = a.post(&food_url, &form("Mon+granola")).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    set_lang("es").await;
    assert!(a.get(&food_url).await.body.contains("Mon granola"));
    assert!(
        a.get("/search?q=granola")
            .await
            .body
            .contains("Mon granola")
    );
}

#[tokio::test]
async fn products_saved_before_names_existed_catch_up_in_search() {
    let a = app().await;
    a.setup().await;
    a.post(
        "/foods/import",
        "source=openfoodfacts&provider_id=4000000000017",
    )
    .await;
    sqlx::query("DELETE FROM food_names")
        .execute(&a.db)
        .await
        .unwrap();

    // The fake search does not return this product, but a barcode lookup does.
    a.post("/settings", "unit_system=metric&timezone=UTC&language=fr")
        .await;
    let page = a.get("/search?q=4000000000017&remote=1").await;
    assert_eq!(page.status, StatusCode::OK);
    let names: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM food_names")
        .fetch_one(&a.db)
        .await
        .unwrap();
    assert_eq!(names, 3);
    let again = a.get("/search?q=4000000000017").await;
    assert!(again.body.contains("Granola croustillant"));
}

#[tokio::test]
async fn appearance_is_stored_per_device_in_a_cookie() {
    let a = app().await;
    a.setup().await;
    let diary = a.get("/diary").await;
    assert!(diary.body.contains("data-theme=\"auto\""));
    assert!(diary.body.contains("content=\"light dark\""));

    let res = a
        .send(
            Request::post("/settings/appearance")
                .header(header::HOST, "localhost")
                .header(header::ORIGIN, "http://localhost")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("theme=dark"))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let cookie = res.set_cookie[0].as_str();
    assert!(cookie.starts_with("korpus_theme=dark;"), "{cookie}");
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));

    let dark = a
        .send(
            Request::get("/settings?msg=appearance_saved")
                .header(header::HOST, "localhost")
                .header(header::COOKIE, "korpus_theme=dark")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(dark.body.contains("data-theme=\"dark\""));
    assert!(
        dark.body
            .contains("<meta name=\"color-scheme\" content=\"dark\">")
    );
    assert!(dark.body.contains("value=\"dark\" checked"));
    assert!(dark.body.contains("Appearance saved on this device."));

    let bad = a.post("/settings/appearance", "theme=neon").await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert!(bad.body.contains("Choose an appearance."));

    // Cross-site requests cannot change it either.
    let cross = a
        .send(
            Request::post("/settings/appearance")
                .header(header::HOST, "localhost")
                .header("sec-fetch-site", "cross-site")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("theme=light"))
                .unwrap(),
        )
        .await;
    assert_eq!(cross.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn nutrients_keep_their_colour_classes() {
    let a = app().await;
    a.setup().await;
    let id = a.create_oats().await;
    let detail = a.get(&format!("/foods/{id}")).await;
    for key in ["c-protein", "c-carbs", "c-fat"] {
        assert!(detail.body.contains(key), "{key} missing on food page");
    }
    assert!(detail.body.contains("class=\"split-bar\""));

    a.post(
        "/diary/entries",
        &format!("food_id={id}&date=2026-05-01&meal=lunch&amount=50"),
    )
    .await;
    let day = a.get("/diary?date=2026-05-01").await;
    assert!(day.body.contains("<div class=\"c-protein\">"));
    assert!(day.body.contains("<b>P</b> 6.5 g"), "{}", day.body);
}

/// A table food for tests, with the given names (main language first).
fn table_food(code: &str, names: &[(Lang, &str)], carbs_include_fiber: bool) -> DatasetFood {
    DatasetFood {
        code: code.into(),
        names: names.iter().map(|(l, n)| (*l, n.to_string())).collect(),
        basis: Basis::Per100g,
        nutrients: Nutrients {
            energy_kcal: Some(52.0),
            protein_g: Some(0.3),
            carbs_g: Some(13.8),
            fat_g: Some(0.2),
            ..Default::default()
        },
        energy_from_kj: false,
        salt_from_sodium: false,
        carbs_include_fiber,
        trace: vec![],
        rejected: vec![],
    }
}

/// The hidden import fields of a result that is not saved yet.
fn import_field(source: &str, code: &str) -> String {
    format!(
        "name=\"source\" value=\"{source}\">\n    <input type=\"hidden\" name=\"provider_id\" value=\"{code}\">"
    )
}

#[tokio::test]
async fn country_picks_the_food_composition_table() {
    let a = app().await;
    a.setup().await;
    datasets::replace(
        &a.db,
        Dataset::Ciqual,
        &[table_food(
            "13039",
            &[(Lang::Fr, "Pomme, crue"), (Lang::En, "Apple, raw")],
            false,
        )],
        None,
    )
    .await
    .unwrap();
    datasets::replace(
        &a.db,
        Dataset::Cnf,
        &[table_food(
            "1",
            &[
                (Lang::En, "Apples, raw, with skin"),
                (Lang::Fr, "Pommes, crues, avec pelure"),
            ],
            true,
        )],
        None,
    )
    .await
    .unwrap();

    let r = a
        .post(
            "/settings",
            "unit_system=metric&timezone=UTC&language=&country=FR",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let page = a.get("/search?q=pomme").await;
    assert!(
        page.body.contains("Apple, raw"),
        "names follow the interface language"
    );
    assert!(page.body.contains(&import_field("ciqual", "13039")));
    assert!(
        !page.body.contains("Apples, raw, with skin"),
        "one table at a time"
    );
    assert!(
        page.body
            .contains("ANSES-CIQUAL French food composition table")
    );
    let settings = a.get("/settings").await;
    assert!(
        settings
            .body
            .contains("Search includes generic foods from ANSES-CIQUAL.")
    );
    assert!(
        settings
            .body
            .contains("<option value=\"FR\" selected>France</option>")
    );

    // Another country, another table.
    a.post(
        "/settings",
        "unit_system=metric&timezone=UTC&language=&country=CA",
    )
    .await;
    let page = a.get("/search?q=apple").await;
    assert!(page.body.contains("Apples, raw, with skin"));
    assert!(!page.body.contains("Apple, raw<"));
    // Barcodes are for packaged products only.
    let page = a.get("/search?q=40000000").await;
    assert!(!page.body.contains("Apples, raw"));

    // Picking a table food saves it like a product, with its conversions explained.
    let r = a
        .post(
            "/foods/import",
            "source=cnf&provider_id=1&date=2026-04-01&meal=lunch",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let detail = a.get(&r.location.unwrap()).await;
    assert!(detail.body.contains("Apples, raw, with skin"));
    assert!(detail.body.contains("Canadian Nutrient File"));
    assert!(detail.body.contains("includes fibre"));
    assert!(detail.body.contains("Health Canada"));
    // The saved copy is listed instead of the table row.
    let page = a.get("/search?q=apple").await;
    assert_eq!(page.body.matches("Apples, raw, with skin").count(), 1);
    assert!(!page.body.contains(&import_field("cnf", "1")));

    // Removing a table keeps saved foods; search falls back to the next table.
    let r = a.post("/settings/datasets/cnf/remove", "").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let page = a.get("/search?q=apple").await;
    assert!(
        page.body.contains("Apples, raw, with skin"),
        "saved food kept"
    );
    assert!(page.body.contains(&import_field("ciqual", "13039")));
    let r = a.post("/foods/import", "source=cnf&provider_id=2").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // The country travels with the JSON export.
    let export = a.get("/settings/export.json").await;
    assert!(export.body.contains("\"country\": \"CA\""));
}

#[tokio::test]
async fn country_is_validated_and_offered_at_setup() {
    let a = app().await;
    let setup = a.get("/setup").await;
    assert!(
        setup
            .body
            .contains("<select id=\"country\" name=\"country\"")
    );
    let r = a
        .post("/setup", "unit_system=metric&timezone=UTC&country=XX")
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("Choose a country from the list."));
    let r = a
        .post("/setup", "unit_system=metric&timezone=UTC&country=es")
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(korpus::profile::country(&a.db).await.unwrap(), Some("ES"));
}

#[tokio::test]
async fn food_tables_are_imported_from_settings_uploads() {
    use std::io::Write;

    let a = app().await;
    a.setup().await;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    w.start_file("FOOD NAME.csv", options).unwrap();
    w.write_all(
        b"FoodID,FoodCode,FoodGroupID,FoodSourceID,FoodDescription,FoodDescriptionF\r\n\
2,2,22,20,Cheese souffle,Souffl\xe9 au fromage\r\n\
4,4,22,20,\"Chop suey, with meat, canned\",\"Chop suey, avec viande, conserve\"\r\n",
    )
    .unwrap();
    w.start_file("NUTRIENT AMOUNT.csv", options).unwrap();
    w.write_all(b"FoodID,NutrientID,NutrientValue\r\n2,208,204\r\n2,203,9.54\r\n4,208,60\r\n")
        .unwrap();
    let zip = w.finish().unwrap().into_inner();

    let r = a
        .upload("/settings/datasets/cnf", &[("cnf-fcen-csv.zip", &zip)])
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert!(r.location.unwrap().contains("msg=dataset_imported"));
    let settings = a.get("/settings").await;
    assert!(settings.body.contains("2 foods, imported"));
    assert!(
        settings
            .body
            .contains("Search includes generic foods from Canadian Nutrient File.")
    );
    let page = a.get("/search?q=souffle").await;
    assert!(page.body.contains("Cheese souffle"));

    // A wrong file is explained next to its table and changes nothing.
    let r = a
        .upload("/settings/datasets/ciqual", &[("notes.txt", b"hello")])
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("id=\"dataset-ciqual-error\""));
    assert!(r.body.contains("The upload does not contain alim_"));
    let r = a
        .upload("/settings/datasets/ciqual", &[("XML_2020.zip", b"hello")])
        .await;
    assert!(r.body.contains("could not be opened"));
    let r = a.upload("/settings/datasets/cofid", &[]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("Choose the downloaded file to import."));
    assert_eq!(datasets::installed(&a.db).await.unwrap().len(), 1);

    let r = a
        .upload("/settings/datasets/nope", &[("x.zip", &zip)])
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn foods_page_lists_logged_foods_and_history_can_be_cleared() {
    let a = app().await;
    a.setup().await;
    let oats = a.create_oats().await;
    let r = a
        .post(
            "/foods",
            "name=Plain+yogurt&brand=&barcode=&basis=g&energy=60&energy_unit=kcal&protein=4&carbs=5&fat=3",
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);

    // Saved or opened foods are not history; only logged ones are.
    let page = a.get("/search").await;
    assert!(
        page.body
            .contains("Foods you add to your diary will appear here.")
    );
    assert!(!page.body.contains("Rolled oats"));
    assert!(!page.body.contains("/search/history/clear"));

    for _ in 0..2 {
        a.post(
            "/diary/entries",
            &format!("food_id={oats}&date=2026-03-10&meal=breakfast&amount=50"),
        )
        .await;
    }
    let page = a.get("/search").await;
    let list = &page.body[page.body.find("id=\"result-list\"").unwrap()..];
    let list = &list[..list.find("</ul>").unwrap()];
    assert_eq!(list.matches("Rolled oats").count(), 1, "listed once");
    assert!(!list.contains("Plain yogurt"));
    assert!(page.body.contains("action=\"/search/history/clear\""));
    assert!(page.body.contains("data-confirm=\"Clear history?"));

    // Make the existing entries older than the moment of clearing.
    sqlx::query("UPDATE diary_entries SET created_at = '2026-03-10T08:00:00Z'")
        .execute(&a.db)
        .await
        .unwrap();
    let r = a
        .post("/search/history/clear", "date=2026-03-10&meal=lunch")
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(
        r.location.as_deref(),
        Some("/search?date=2026-03-10&meal=lunch&msg=history_cleared")
    );
    let page = a.get("/search").await;
    assert!(!page.body.contains("Rolled oats"));
    // The diary itself is untouched.
    assert_eq!(a.day_kcal("2026-03-10").await, "370");

    // Logging again brings a food back.
    sqlx::query("UPDATE profile SET history_cleared_at = '2026-03-10T09:00:00Z'")
        .execute(&a.db)
        .await
        .unwrap();
    a.post(
        "/diary/entries",
        &format!("food_id={oats}&date=2026-03-11&meal=lunch&amount=10"),
    )
    .await;
    assert!(a.get("/search").await.body.contains("Rolled oats"));
}

#[tokio::test]
async fn bundled_table_can_be_installed_and_search_says_when_none_is() {
    let a = app().await;
    a.setup().await;
    let page = a.get("/search?q=oeuf").await;
    assert!(page.body.contains("no food composition table is installed"));
    assert!(page.body.contains("href=\"/settings#datasets-heading\""));
    assert!(
        !a.get("/search")
            .await
            .body
            .contains("no food composition table"),
        "not on the history list"
    );
    let settings = a.get("/settings").await;
    assert!(
        settings
            .body
            .contains("Use Ciqual 2025, included with Korpus")
    );

    let r = a.post("/settings/datasets/ciqual/bundled", "").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let settings = a.get("/settings").await;
    assert!(settings.body.contains("Ciqual 2025 included with Korpus"));
    assert!(
        !settings
            .body
            .contains("Use Ciqual 2025, included with Korpus")
    );
    let page = a.get("/search?q=oeuf").await;
    assert!(!page.body.contains("no food composition table is installed"));
    assert!(
        page.body.contains(&import_field("ciqual", "22000")),
        "Oeuf cru"
    );

    assert_eq!(
        a.post("/settings/datasets/cofid/bundled", "").await.status,
        StatusCode::NOT_FOUND,
        "only bundled tables"
    );
}

// --- Login ---------------------------------------------------------------------

/// The `name=value` part of the session cookie set by a response.
fn session_cookie(r: &Resp) -> String {
    r.set_cookie
        .iter()
        .find(|c| c.starts_with("korpus_session="))
        .and_then(|c| c.split(';').next())
        .expect("session cookie")
        .to_string()
}

fn current_code(secret: &str) -> String {
    let step = jiff::Timestamp::now().as_second() / 30;
    format!(
        "{:06}",
        korpus::auth::totp_code(&korpus::auth::base32_decode(secret).unwrap(), step)
    )
}

impl App {
    /// The same installation seen from another browser.
    fn as_visitor(&self, session: Option<String>) -> App {
        App {
            router: self.router.clone(),
            db: self.db.clone(),
            provider: self.provider.clone(),
            auth: self.auth.clone(),
            session,
        }
    }

    async fn login(&self, username: &str, password: &str, next: &str) -> Resp {
        self.post(
            "/login",
            &format!(
                "username={username}&password={}&next={next}",
                password.replace(' ', "+")
            ),
        )
        .await
    }
}

#[tokio::test]
async fn every_page_requires_a_login() {
    let a = app().await;
    a.setup().await;
    a.post("/weight", "date=2026-01-01&weight=70").await;
    let visitor = a.as_visitor(None);

    let r = visitor.get("/weight").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.location.as_deref(), Some("/login?next=%2Fweight"));
    for path in [
        "/diary",
        "/settings/export.json",
        "/settings/backup.sqlite",
        "/nope",
    ] {
        let r = visitor.get(path).await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{path}");
        assert!(r.location.unwrap().starts_with("/login"), "{path}");
        assert!(!r.body.contains("70"), "no data leaks: {path}");
    }
    let fragment = visitor
        .send(
            Request::get("/search/remote?q=oats")
                .header(header::HOST, "localhost")
                .header("x-requested-with", "fetch")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(fragment.status, StatusCode::UNAUTHORIZED);

    let r = visitor.post("/weight", "date=2026-01-02&weight=99").await;
    assert_eq!(r.location.as_deref(), Some("/login"));
    let weights: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM weight_entries")
        .fetch_one(&visitor.db)
        .await
        .unwrap();
    assert_eq!(weights, 1, "a logged-out request changes nothing");

    // Public: login pages, assets and health checks.
    assert_eq!(visitor.get("/login").await.status, StatusCode::OK);
    assert_eq!(visitor.get("/healthz").await.status, StatusCode::OK);
    assert_eq!(visitor.get("/static/app.css").await.status, StatusCode::OK);
    for icon in [
        "/static/favicon.svg",
        "/static/apple-touch-icon.png",
        "/apple-touch-icon.png",
    ] {
        assert_eq!(visitor.get(icon).await.status, StatusCode::OK, "{icon}");
    }
    // A forged or unknown cookie is no session.
    let forged = visitor.as_visitor(Some(format!("korpus_session={}", "a".repeat(64))));
    assert_eq!(forged.get("/diary").await.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn the_account_is_created_with_the_setup_code_from_the_log() {
    let a = app_without_login().await;
    let r = a.get("/diary").await;
    assert_eq!(r.location.as_deref(), Some("/account/setup"));
    assert_eq!(
        a.get("/login").await.location.as_deref(),
        Some("/account/setup")
    );
    let page = a.get("/account/setup").await;
    assert_eq!(page.status, StatusCode::OK);
    let code = a.auth.setup_code().unwrap();

    let form = |code: &str, password: &str, confirm: &str| {
        format!(
            "username=me&password={}&password_confirm={}&setup_code={code}",
            password.replace(' ', "+"),
            confirm.replace(' ', "+")
        )
    };
    let wrong = a
        .post("/account/setup", &form("WRONG-CODE1", PASSWORD, PASSWORD))
        .await;
    assert_eq!(wrong.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(wrong.body.contains("setup code is incorrect"));
    let short = a
        .post("/account/setup", &form(&code, "short", "short"))
        .await;
    assert!(short.body.contains("at least 10 characters"));
    let mismatch = a
        .post("/account/setup", &form(&code, PASSWORD, "something else"))
        .await;
    assert!(mismatch.body.contains("do not match"));
    assert!(!korpus::auth::has_account(&a.db).await.unwrap());

    let created = a
        .post("/account/setup", &form(&code, PASSWORD, PASSWORD))
        .await;
    assert_eq!(created.status, StatusCode::SEE_OTHER);
    let cookie = &created.set_cookie[0];
    assert!(
        cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"),
        "{cookie}"
    );
    assert!(!cookie.contains("Secure"), "plain HTTP behind no proxy");
    let me = a.as_visitor(Some(session_cookie(&created)));
    // Logged in, the profile set-up comes next.
    assert_eq!(me.get("/diary").await.location.as_deref(), Some("/setup"));

    // The code is spent; the page now sends visitors to the login.
    let again = me
        .as_visitor(None)
        .post("/account/setup", &form(&code, PASSWORD, PASSWORD))
        .await;
    assert_eq!(again.location.as_deref(), Some("/login"));
    assert_eq!(
        korpus::auth::account(&me.db)
            .await
            .unwrap()
            .unwrap()
            .username,
        "me"
    );
}

#[tokio::test]
async fn login_logout_and_attempt_limits() {
    let a = app().await;
    a.setup().await;
    let visitor = a.as_visitor(None);

    let wrong = visitor.login(USERNAME, "wrong password", "").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(wrong.body.contains("username or password is incorrect"));
    let unknown = visitor.login("someone", PASSWORD, "").await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert!(unknown.body.contains("username or password is incorrect"));

    let ok = visitor.login("TESTER", PASSWORD, "%2Fweight").await;
    assert_eq!(ok.status, StatusCode::SEE_OTHER);
    assert_eq!(ok.location.as_deref(), Some("/weight"));
    let me = visitor.as_visitor(Some(session_cookie(&ok)));
    assert_eq!(me.get("/weight").await.status, StatusCode::OK);
    // Only local paths are followed after logging in.
    let me2 = me.as_visitor(None);
    let evil = me2.login(USERNAME, PASSWORD, "%2F%2Fevil.example").await;
    assert_eq!(evil.location.as_deref(), Some("/"));

    // Behind an HTTPS proxy, the cookie is Secure.
    let https = me2
        .send(
            Request::post("/login")
                .header(header::HOST, "localhost")
                .header(header::ORIGIN, "http://localhost")
                .header("x-forwarded-proto", "https")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "username={USERNAME}&password={}",
                    PASSWORD.replace(' ', "+")
                )))
                .unwrap(),
        )
        .await;
    assert!(https.set_cookie[0].contains("; Secure"));

    let me = me2.as_visitor(Some(session_cookie(&ok)));
    let out = me.post("/logout", "").await;
    assert_eq!(out.location.as_deref(), Some("/login"));
    assert!(out.set_cookie[0].contains("Max-Age=0"));
    assert_eq!(
        me.get("/weight").await.status,
        StatusCode::SEE_OTHER,
        "session ended"
    );

    // After ten failures even the right password must wait. (The login
    // above cleared the earlier ones.)
    let visitor = me.as_visitor(None);
    for _ in 0..10 {
        visitor.login(USERNAME, "wrong password", "").await;
    }
    let blocked = visitor.login(USERNAME, PASSWORD, "").await;
    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(blocked.body.contains("Too many failed attempts"));
}

#[tokio::test]
async fn password_change_logs_out_other_devices() {
    let a = app().await;
    a.setup().await;
    let other = a.as_visitor(a_session(&a).await);

    let wrong = a
        .post(
            "/settings/password",
            "current=nope&password=new+password+42&password_confirm=new+password+42",
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(wrong.body.contains("current password is incorrect"));
    let ok = a
        .post(
            "/settings/password",
            "current=correct+horse+battery&password=new+password+42&password_confirm=new+password+42",
        )
        .await;
    assert_eq!(
        ok.location.as_deref(),
        Some("/settings?msg=password_changed#account-heading")
    );
    assert_eq!(
        a.get("/settings").await.status,
        StatusCode::OK,
        "this device stays"
    );
    let second = a.as_visitor(other.session.clone());
    assert_eq!(second.get("/settings").await.status, StatusCode::SEE_OTHER);
    let visitor = second.as_visitor(None);
    assert_eq!(
        visitor.login(USERNAME, PASSWORD, "").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        visitor.login(USERNAME, "new password 42", "").await.status,
        StatusCode::SEE_OTHER
    );
}

/// A second, independent session cookie.
async fn a_session(a: &App) -> Option<String> {
    let token = korpus::auth::create_session(&a.db, false).await.unwrap();
    Some(format!("korpus_session={token}"))
}

#[tokio::test]
async fn two_factor_login_with_an_authenticator_app() {
    let a = app().await;
    a.setup().await;
    let other = a.as_visitor(a_session(&a).await);

    // A session alone does not reveal a secret: the password comes first.
    let page = other.get("/settings/2fa").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(!page.body.contains("<svg class=\"qr\""));
    assert!(page.body.contains(r#"action="/settings/2fa/start""#));
    let wrong = other.post("/settings/2fa/start", "current=nope").await;
    assert_eq!(wrong.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!wrong.body.contains("<svg class=\"qr\""));
    let abandoned = other
        .post("/settings/2fa/start", "current=correct+horse+battery")
        .await;
    let page = other
        .post("/settings/2fa/start", "current=correct+horse+battery")
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("<svg class=\"qr\" role=\"img\""));
    let secret: String = sqlx::query_scalar("SELECT totp_pending_secret FROM account")
        .fetch_one(&other.db)
        .await
        .unwrap();
    assert!(
        page.body.contains(&secret[..4]),
        "key shown for manual entry"
    );
    assert!(
        !abandoned.body.contains(&secret[..8]),
        "starting again makes a new secret"
    );
    assert!(
        other
            .get("/settings")
            .await
            .body
            .contains(r#"<span class="row-value">Off</span>"#)
    );

    let bad = other.post("/settings/2fa", "code=12345").await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        !bad.body.contains(&secret[..4]) && !bad.body.contains("<svg class=\"qr\""),
        "a wrong code does not show the secret again"
    );
    let code = current_code(&secret);
    let on = other.post("/settings/2fa", &format!("code={code}")).await;
    assert_eq!(
        on.location.as_deref(),
        Some("/settings?msg=twofa_enabled#account-heading")
    );
    let settings = other.get("/settings").await.body;
    assert!(
        settings.contains(r#"<span class="row-value">On</span>"#),
        "{settings}"
    );
    assert_eq!(
        a.get("/diary").await.status,
        StatusCode::SEE_OTHER,
        "other device logged out"
    );

    // Logging in now takes the password, then a code.
    let visitor = a.as_visitor(None);
    let step1 = visitor.login(USERNAME, PASSWORD, "%2Fweight").await;
    assert_eq!(
        step1.location.as_deref(),
        Some("/login/code?next=%2Fweight")
    );
    assert!(step1.set_cookie[0].contains("Max-Age=300"));
    let pending = visitor.as_visitor(Some(session_cookie(&step1)));
    // The password alone opens nothing.
    assert_eq!(
        pending.get("/diary").await.location.as_deref(),
        Some("/login/code")
    );
    assert_eq!(pending.get("/login/code").await.status, StatusCode::OK);
    // The code that turned the feature on cannot be used again.
    let replay = pending
        .post("/login/code", &format!("code={code}&next=%2Fweight"))
        .await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    assert!(replay.body.contains("incorrect or has expired"));

    // The next time step's code is accepted (clocks may differ by one step).
    let next_step = jiff::Timestamp::now().as_second() / 30 + 1;
    let next_code = format!(
        "{:06}",
        korpus::auth::totp_code(&korpus::auth::base32_decode(&secret).unwrap(), next_step)
    );
    let done = pending
        .post("/login/code", &format!("code={next_code}&next=%2Fweight"))
        .await;
    assert_eq!(done.location.as_deref(), Some("/weight"));
    let me = pending.as_visitor(Some(session_cookie(&done)));
    assert_eq!(me.get("/weight").await.status, StatusCode::OK);
    assert_ne!(me.session, pending.session, "a new session after the code");
    assert_eq!(pending.get("/diary").await.status, StatusCode::SEE_OTHER);

    // Turning it off needs the password.
    let wrong = me.post("/settings/2fa/disable", "current=nope").await;
    assert_eq!(wrong.status, StatusCode::UNPROCESSABLE_ENTITY);
    let off = me
        .post("/settings/2fa/disable", "current=correct+horse+battery")
        .await;
    assert_eq!(
        off.location.as_deref(),
        Some("/settings?msg=twofa_disabled#account-heading")
    );
    let visitor = me.as_visitor(None);
    assert_eq!(
        visitor
            .login(USERNAME, PASSWORD, "")
            .await
            .location
            .as_deref(),
        Some("/")
    );
}

#[tokio::test]
async fn guesses_sent_at_once_are_limited_too() {
    let a = app().await;
    let visitor = Arc::new(a.as_visitor(None));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..40 {
        let v = visitor.clone();
        tasks.spawn(async move { v.login(USERNAME, "wrong password", "").await.status });
    }
    let mut judged = 0;
    while let Some(status) = tasks.join_next().await {
        match status.unwrap() {
            StatusCode::UNAUTHORIZED => judged += 1,
            StatusCode::TOO_MANY_REQUESTS => {}
            other => panic!("{other}"),
        }
    }
    assert_eq!(judged, 10, "only ten guesses were checked");
}
