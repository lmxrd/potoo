use crate::stats::{SiteStats, StatsRange};
use axum::http::header;
use axum::response::Response;
use chrono::{DateTime, Utc};

/// A CSV writer with the same conventions as the old export: RFC 4180
/// quoting, CRLF line endings, UTF-8.
pub fn write_csv(rows: &[Vec<String>]) -> String {
    let mut out = Vec::new();
    {
        let mut writer = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .flexible(true)
            .from_writer(&mut out);
        for row in rows {
            writer.write_record(row).expect("in-memory CSV cannot fail");
        }
        writer.flush().expect("in-memory CSV cannot fail");
    }
    String::from_utf8(out).expect("CSV is UTF-8")
}

/// The range of a stats view, as one CSV header line.
pub fn range_line(range: &StatsRange) -> String {
    match range {
        StatsRange::Days(1) => "last 24 hours".into(),
        StatsRange::Days(n) => format!("last {n} days"),
        StatsRange::FromTo { from, to } => format!("{} to {}", minute(from), minute(to)),
    }
}

fn minute(d: &DateTime<Utc>) -> String {
    format!(
        "{}Z",
        &d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)[..16]
    )
}

/// One dashboard view's aggregated numbers, as one CSV document: a header
/// with the range, the filter and the totals, then one section per
/// breakdown. The columns match the breakdowns exactly.
pub fn stats_csv(stats: &SiteStats, range: &str, filter: Option<&str>) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let line = |rows: &mut Vec<Vec<String>>, cells: Vec<String>| rows.push(cells);
    let cell = |v: &dyn std::fmt::Display| v.to_string();

    line(&mut rows, vec!["# potoo stats".into(), range.into()]);
    if let Some(filter) = filter {
        line(&mut rows, vec!["# filter".into(), filter.into()]);
    }
    line(
        &mut rows,
        vec!["# pageviews".into(), cell(&stats.pageviews)],
    );
    line(&mut rows, vec!["# visitors".into(), cell(&stats.visitors)]);

    let sections = [
        (
            "Page views per period",
            vec!["period", "pageviews", "visitors"],
        ),
        ("Paths", vec!["path", "pageviews", "visitors"]),
        ("Referrers", vec!["referrer", "pageviews"]),
        ("Countries", vec!["country", "visitors"]),
        ("Devices", vec!["device", "visitors"]),
        ("Browsers", vec!["browser", "visitors"]),
        ("Operating systems", vec!["os", "visitors"]),
        ("Clicks", vec!["name", "clicks"]),
    ]
    .into_iter()
    .peekable();
    let mut data = [
        stats
            .by_day
            .iter()
            .map(|d| vec![d.day.clone(), cell(&d.pageviews), cell(&d.visitors)])
            .collect::<Vec<_>>(),
        stats
            .paths
            .iter()
            .map(|p| vec![p.path.clone(), cell(&p.pageviews), cell(&p.visitors)])
            .collect::<Vec<_>>(),
        stats
            .referrers
            .iter()
            .map(|r| vec![r.referrer.clone(), cell(&r.pageviews)])
            .collect::<Vec<_>>(),
        stats
            .countries
            .iter()
            .map(|c| vec![c.country.clone(), cell(&c.visitors)])
            .collect::<Vec<_>>(),
        stats
            .devices
            .iter()
            .map(|d| vec![d.device.clone(), cell(&d.visitors)])
            .collect::<Vec<_>>(),
        stats
            .browsers
            .iter()
            .map(|b| vec![b.browser.clone(), cell(&b.visitors)])
            .collect::<Vec<_>>(),
        stats
            .os
            .iter()
            .map(|o| vec![o.os.clone(), cell(&o.visitors)])
            .collect::<Vec<_>>(),
        stats
            .clicks
            .iter()
            .map(|c| vec![c.name.clone(), cell(&c.clicks)])
            .collect::<Vec<_>>(),
    ]
    .into_iter();

    let mut first = true;
    for (title, columns) in sections {
        if !first {
            line(&mut rows, vec![]);
        }
        first = false;
        line(&mut rows, vec![format!("# {title}")]);
        line(
            &mut rows,
            columns.into_iter().map(|c| c.to_string()).collect(),
        );
        for row in data.next().unwrap_or_default() {
            rows.push(row);
        }
    }
    write_csv(&rows)
}

/// A CSV document as a download.
pub fn download(body: String, filename: &str) -> Response {
    let mut response = Response::new(axum::body::Body::from(body));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .expect("a filename header value"),
    );
    response
}
