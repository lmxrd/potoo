//! A write-load test for the tracker: `POST /api/collect` at a series of
//! fixed request rates (open loop), reporting throughput and latency per
//! step. Latency is measured from when a request was *due*, not when it
//! was sent, so a server that falls behind shows up as latency instead of
//! the generator quietly slowing down.
//!
//! Every hit looks like a different visitor (random IP via X-Forwarded-For,
//! one of several user agents, one of a few hundred paths), so run the
//! server with TRUST_PROXY=1 and with a site registered for HOST.
//!
//!   cargo run --release --example loadtest -- \
//!     --url http://127.0.0.1:8000 --host load.example.com \
//!     --rates 200,500,1000,2000,4000 --seconds 15
//!
//! `--mode goatcounter` sends `GET /count?p=..&r=..` with the site's Host
//! header instead, the hit of GoatCounter's tracking script, to compare the
//! two tools with the same generator (the site is then HOST).
//!
//! `--close` sends `Connection: close`, so every hit opens a new TCP
//! connection, as with GoatCounter, which closes every connection itself.
//!
//! `--rates health:2000` style entries hit /api/health instead, to measure
//! the ceiling of the generator and the network path themselves.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36",
    "Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0",
];
const REFERRERS: &[&str] = &[
    "",
    "",
    "https://www.google.com/",
    "https://news.ycombinator.com/item?id=1",
    "https://twitter.com/",
    "https://duckduckgo.com/",
    "https://www.reddit.com/r/rust/",
];

/// SplitMix64: a small, fast, decent random number generator, so the
/// example needs no extra dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

struct Args {
    url: String,
    host: String,
    rates: Vec<(bool, u64)>, // (health?, requests per second)
    seconds: u64,
    max_in_flight: usize,
    goatcounter: bool,
    close: bool,
}

fn args() -> Args {
    let mut args = Args {
        url: "http://127.0.0.1:8000".into(),
        host: "load.example.com".into(),
        rates: vec![(false, 500), (false, 1000), (false, 2000)],
        seconds: 15,
        max_in_flight: 2000,
        goatcounter: false,
        close: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        if flag == "--close" {
            args.close = true;
            continue;
        }
        let value = it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--url" => args.url = value.trim_end_matches('/').into(),
            "--host" => args.host = value,
            "--seconds" => args.seconds = value.parse().expect("--seconds is a number"),
            "--max-in-flight" => args.max_in_flight = value.parse().expect("a number"),
            "--mode" => match value.as_str() {
                "potoo" => args.goatcounter = false,
                "goatcounter" => args.goatcounter = true,
                other => panic!("--mode is potoo or goatcounter, not {other}"),
            },
            "--rates" => {
                args.rates = value
                    .split(',')
                    .map(|r| match r.strip_prefix("health:") {
                        Some(r) => (true, r.parse().expect("a rate")),
                        None => (false, r.parse().expect("a rate")),
                    })
                    .collect()
            }
            other => panic!("unknown flag {other}"),
        }
    }
    args
}

struct Step {
    latencies: Mutex<Vec<u32>>, // microseconds, from when the request was due
    ok: std::sync::atomic::AtomicU64,
    failed: std::sync::atomic::AtomicU64,
}

#[tokio::main]
async fn main() {
    potoo::install_tls_provider();
    let args = args();
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(args.max_in_flight)
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    println!(
        "target {}  host {}  {}s per step  max {} in flight\n",
        args.url, args.host, args.seconds, args.max_in_flight
    );
    println!(
        "{:>10} {:>10} {:>8} {:>8} {:>9} {:>9} {:>9} {:>9}",
        "offered/s", "done/s", "failed", "dropped", "p50 ms", "p95 ms", "p99 ms", "max ms"
    );

    for (health, rate) in &args.rates {
        run_step(&client, &args, *health, *rate).await;
        // Let the server drain before the next step.
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn run_step(client: &reqwest::Client, args: &Args, health: bool, rate: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    let step = Arc::new(Step {
        latencies: Mutex::new(Vec::with_capacity((rate * args.seconds) as usize)),
        ok: 0.into(),
        failed: 0.into(),
    });
    let permits = Arc::new(Semaphore::new(args.max_in_flight));
    let mut dropped = 0u64;
    let total = rate * args.seconds;
    let start = Instant::now();
    let mut rng = Rng(rate ^ 0xDEAD_BEEF);

    for i in 0..total {
        let due = start + Duration::from_secs_f64(i as f64 / rate as f64);
        tokio::time::sleep_until(due.into()).await;

        let Ok(permit) = permits.clone().try_acquire_owned() else {
            dropped += 1; // the server (or the network) cannot keep up
            continue;
        };
        let request = if health {
            client.get(format!("{}/api/health", args.url))
        } else if args.goatcounter {
            let ua = USER_AGENTS[rng.below(USER_AGENTS.len())];
            let referrer = REFERRERS[rng.below(REFERRERS.len())];
            let ip = format!(
                "{}.{}.{}.{}",
                11 + rng.below(89),
                rng.below(256),
                rng.below(256),
                1 + rng.below(254)
            );
            let query = format!(
                "p=%2Fpage%2F{}&r={}",
                rng.below(300),
                referrer
                    .replace(':', "%3A")
                    .replace('/', "%2F")
                    .replace('?', "%3F")
                    .replace('=', "%3D")
            );
            client
                .get(format!("{}/count?{query}", args.url))
                .header("host", &args.host)
                .header("x-forwarded-for", ip)
                .header("user-agent", ua)
        } else {
            let ua = USER_AGENTS[rng.below(USER_AGENTS.len())];
            let ip = format!(
                "{}.{}.{}.{}",
                11 + rng.below(89), // public, not 10/100/127/169/172/192
                rng.below(256),
                rng.below(256),
                1 + rng.below(254)
            );
            let referrer = REFERRERS[rng.below(REFERRERS.len())];
            let path = format!("/page/{}", rng.below(300));
            let body = if rng.below(100) < 85 {
                format!(
                    r#"{{"type":"pageview","host":"{}","path":"{path}","referrer":"{referrer}"}}"#,
                    args.host
                )
            } else {
                format!(
                    r#"{{"type":"click","host":"{}","path":"{path}","name":"button-{}"}}"#,
                    args.host,
                    rng.below(20)
                )
            };
            client
                .post(format!("{}/api/collect", args.url))
                .header("user-agent", ua)
                .header("x-forwarded-for", ip)
                .header("content-type", "text/plain")
                .body(body)
        };
        let request = if args.close {
            request.header("connection", "close")
        } else {
            request
        };
        let step = step.clone();
        tokio::spawn(async move {
            let result = request.send().await;
            let micros = due.elapsed().as_micros().min(u32::MAX as u128) as u32;
            match result {
                Ok(r) if r.status().is_success() => {
                    step.ok.fetch_add(1, Relaxed);
                    step.latencies.lock().unwrap().push(micros);
                }
                _ => {
                    step.failed.fetch_add(1, Relaxed);
                }
            }
            drop(permit);
        });
    }

    // Wait for the stragglers (they have a 10 s timeout at most).
    let _ = tokio::time::timeout(
        Duration::from_secs(12),
        permits.acquire_many(args.max_in_flight as u32),
    )
    .await;
    let elapsed = start.elapsed().as_secs_f64();

    let mut latencies = step.latencies.lock().unwrap().clone();
    latencies.sort_unstable();
    let pct = |p: f64| {
        latencies
            .get(((latencies.len() as f64 * p) as usize).min(latencies.len().saturating_sub(1)))
            .map(|&us| us as f64 / 1000.0)
            .unwrap_or(f64::NAN)
    };
    println!(
        "{:>10} {:>10.0} {:>8} {:>8} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
        format!("{}{}", if health { "h" } else { "" }, rate),
        step.ok.load(Relaxed) as f64 / elapsed,
        step.failed.load(Relaxed),
        dropped,
        pct(0.50),
        pct(0.95),
        pct(0.99),
        latencies
            .last()
            .map(|&us| us as f64 / 1000.0)
            .unwrap_or(f64::NAN),
    );
}
