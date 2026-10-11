use axum::http::header;
use axum::response::Response;

/// A CSV document: RFC 4180 quoting, CRLF line endings, UTF-8.
pub fn write_csv(rows: &[Vec<String>]) -> String {
    let mut out = Vec::new();
    {
        let mut writer = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .from_writer(&mut out);
        for row in rows {
            writer.write_record(row).expect("in-memory CSV cannot fail");
        }
        writer.flush().expect("in-memory CSV cannot fail");
    }
    String::from_utf8(out).expect("CSV is UTF-8")
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
