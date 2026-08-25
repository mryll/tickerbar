use std::io::Read;
use std::time::Duration;

use crate::platform::model::FetchError;

/// Cap on a response body. Every byte here comes from one of six third-party
/// APIs, and the whole document is materialized in memory before it is parsed:
/// without a cap, one endpoint answering with a stream instead of a quote grows
/// the omarchy-shell process it runs in until the machine gives up. The largest
/// legitimate document any provider returns is a few tens of KiB, so 2 MiB is a
/// wall a real answer never reaches.
const BODY_LIMIT: u64 = 2 * 1024 * 1024;

pub struct Http {
    client: reqwest::blocking::Client,
    base_url: Option<String>,
}

impl Http {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            client: build_client(timeout_secs),
            base_url: None,
        }
    }

    /// Test constructor: every request is rewritten to `base` (the mock server host),
    /// preserving the original path + query so provider URL construction is exercised.
    pub fn with_base_url(base: &str, timeout_secs: u64) -> Self {
        Self {
            client: build_client(timeout_secs),
            base_url: Some(base.trim_end_matches('/').to_string()),
        }
    }

    pub fn get(&self, url: &str) -> Result<String, FetchError> {
        self.get_with_header(url, None)
    }

    /// GET with an optional `(header_name, header_value)`. Used to pass secret tokens via a
    /// header instead of the URL, so the value never lands in error strings or logs.
    pub fn get_with_header(
        &self,
        url: &str,
        header: Option<(&str, &str)>,
    ) -> Result<String, FetchError> {
        let target = self.resolve(url);
        let mut req = self.client.get(&target);
        if let Some((name, value)) = header {
            req = req.header(name, value);
        }
        let resp = req
            .send()
            .map_err(|e| FetchError::Other(format!("request failed: {e}")))?;
        let status = resp.status();
        if status.as_u16() == 429 {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            return Err(FetchError::RateLimited { retry_after });
        }
        if !status.is_success() {
            return Err(FetchError::Other(format!("http {status}")));
        }
        read_body(resp)
    }

    /// In production, use the URL as-is. With a base set (tests), rewrite scheme+host to the
    /// base host while keeping the path and query.
    fn resolve(&self, url: &str) -> String {
        match &self.base_url {
            None => url.to_string(),
            Some(base) => match reqwest::Url::parse(url) {
                Ok(u) => {
                    let q = u.query().map(|q| format!("?{q}")).unwrap_or_default();
                    format!("{base}{}{}", u.path(), q)
                }
                Err(_) => format!("{base}{url}"),
            },
        }
    }
}

/// Read a response body under `BODY_LIMIT`, refusing anything above it.
///
/// `Response::text` has no bound: it buffers whatever the server sends. Reading
/// one byte PAST the limit is what distinguishes a body that exactly fits from
/// one the cap truncated — with `take(BODY_LIMIT)` the two are identical, and a
/// truncated document would reach serde as a parse error about nothing.
/// Decoding is strict and comes second, for the same reason: all six providers
/// answer JSON, which RFC 8259 requires to be UTF-8, so lossy decoding would
/// only move a broken response's failure to a worse message downstream.
fn read_body(resp: reqwest::blocking::Response) -> Result<String, FetchError> {
    let mut buf: Vec<u8> = Vec::new();
    resp.take(BODY_LIMIT + 1)
        .read_to_end(&mut buf)
        .map_err(|e| FetchError::Other(format!("read body failed: {e}")))?;
    if buf.len() as u64 > BODY_LIMIT {
        return Err(FetchError::Other(format!(
            "response body is larger than {BODY_LIMIT} bytes"
        )));
    }
    String::from_utf8(buf).map_err(|e| FetchError::Other(format!("body is not valid UTF-8: {e}")))
}

fn build_client(timeout_secs: u64) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        // None of the six provider endpoints redirects legitimately, so every
        // redirect is either a hijacked hostname or an operator mistake; the
        // default policy would chase up to ten of them, and nothing stops the
        // chain from stepping down to plain http on the way. Refusing turns a
        // 3xx into an ordinary `http 3xx` failure at the first hop.
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("tickerbar/", env!("CARGO_PKG_VERSION")))
        .build()
        // The former fallback here was `Client::new()`, which has NO timeout and
        // follows redirects — precisely the client the two settings above exist
        // to forbid, reintroduced on the one path nobody tests. `main` wraps the
        // run in `catch_unwind` and answers with fallback JSON, so failing loudly
        // costs a run, not the never-crash invariant.
        .expect("blocking client with a timeout and no redirects")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::model::FetchError;

    /// The cap, written out by hand. Reading `BODY_LIMIT` here would move the
    /// expectation with the code, so raising the cap could never turn a test
    /// red: the body would grow with it and the message would still match.
    /// Changing a safety limit has to cost editing this number on purpose.
    const DOCUMENTED_BODY_LIMIT: u64 = 2 * 1024 * 1024;

    #[test]
    fn the_body_cap_is_still_the_two_mebibytes_the_module_documents() {
        assert_eq!(
            BODY_LIMIT, DOCUMENTED_BODY_LIMIT,
            "the response body cap changed; update the doc comment and this number together"
        );
    }

    #[test]
    fn a_200_response_returns_the_body() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/ok")
            .with_status(200)
            .with_body("hi")
            .create();
        let http = Http::with_base_url(&server.url(), 2);
        let body = http.get("/ok").unwrap();
        assert_eq!(body, "hi");
        m.assert();
    }

    #[test]
    fn an_absolute_url_is_rewritten_to_the_base_host_preserving_path_and_query() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/api/v3/simple/price")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("ok")
            .create();
        let http = Http::with_base_url(&server.url(), 2);
        let body = http
            .get("https://api.coingecko.com/api/v3/simple/price?ids=bitcoin")
            .unwrap();
        assert_eq!(body, "ok");
        m.assert();
    }

    #[test]
    fn a_429_with_retry_after_maps_to_rate_limited() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/x")
            .with_status(429)
            .with_header("retry-after", "30")
            .create();
        let http = Http::with_base_url(&server.url(), 2);
        match http.get("/x") {
            Err(FetchError::RateLimited { retry_after }) => assert_eq!(retry_after, Some(30)),
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn a_redirect_is_refused_at_the_first_hop_instead_of_being_followed() {
        let mut server = mockito::Server::new();
        let target = server
            .mock("GET", "/moved")
            .with_status(200)
            .with_body("followed")
            .expect(0)
            .create();
        server
            .mock("GET", "/r")
            .with_status(301)
            .with_header("location", "/moved")
            .create();
        let http = Http::with_base_url(&server.url(), 2);
        match http.get("/r") {
            Err(FetchError::Other(m)) => assert!(m.contains("301"), "got: {m}"),
            other => panic!("expected the 301 itself, got {other:?}"),
        }
        target.assert();
    }

    #[test]
    fn an_oversize_body_is_refused_by_size_instead_of_parsed() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/big")
            .with_status(200)
            .with_body(vec![b'x'; (DOCUMENTED_BODY_LIMIT + 16) as usize])
            .create();
        let http = Http::with_base_url(&server.url(), 30);
        match http.get("/big") {
            Err(FetchError::Other(m)) => {
                assert!(m.contains("larger than 2097152 bytes"), "got: {m}")
            }
            other => panic!("expected an oversize error, got {other:?}"),
        }
    }

    #[test]
    fn a_500_maps_to_other() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/e").with_status(500).create();
        let http = Http::with_base_url(&server.url(), 2);
        assert!(matches!(http.get("/e"), Err(FetchError::Other(_))));
    }
}
