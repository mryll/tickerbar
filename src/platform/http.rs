use std::time::Duration;

use crate::platform::model::FetchError;

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
        resp.text()
            .map_err(|e| FetchError::Other(format!("read body failed: {e}")))
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

fn build_client(timeout_secs: u64) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent(concat!("tickerbar/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::model::FetchError;

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
    fn a_500_maps_to_other() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/e").with_status(500).create();
        let http = Http::with_base_url(&server.url(), 2);
        assert!(matches!(http.get("/e"), Err(FetchError::Other(_))));
    }
}
