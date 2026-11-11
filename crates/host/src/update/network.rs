//! The downloads `floti update` makes: `update.json`, each release file and
//! each signature, over the host's one HTTP client (reqwest on rustls with
//! ring, as `floti.fetch` uses it).
//!
//! Only `https` is fetched, with certificates the OS trusts and the system
//! proxy. A build with the testkit also fetches `http` from this computer's
//! loopback address, where a test serves a release; a release build never
//! does. A redirect is followed (a GitHub release download is one) only to
//! an address that rule allows. A body is passed on piece by piece as it
//! arrives, so a desktop installer is never held whole in memory, and one
//! that ends before the length its response announced is cut off, not done.

use std::io;

use reqwest::{Client, redirect};
use url::{Host, Url};

/// Why a download did not finish.
#[derive(Debug)]
pub enum FetchError {
    /// The server answered 404.
    NotFound,
    /// The server answered another status that is not success.
    Status(u16),
    /// Nothing arrived: the address could not be reached or refused us.
    Failed(String),
    /// The body stopped before its end.
    Cut(String),
    /// The body is longer than the limit.
    TooLarge(u64),
    /// Whatever the body was being written to refused it.
    Sink(io::Error),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("it is not there (404)"),
            Self::Status(status) => write!(f, "the server answered {status}"),
            Self::Failed(reason) | Self::Cut(reason) => f.write_str(reason),
            Self::TooLarge(limit) => write!(f, "it is over {limit} bytes"),
            Self::Sink(err) => write!(f, "it could not be written: {err}"),
        }
    }
}

/// What can fetch an address. The host's is [`Https`]; tests fake it.
pub trait Network {
    /// Fetches `url`, handing each piece of the body to `sink` in order,
    /// and refusing a body past `limit` bytes.
    fn get(
        &self,
        url: &Url,
        limit: u64,
        sink: &mut dyn FnMut(&[u8]) -> io::Result<()>,
    ) -> Result<(), FetchError>;
}

/// `url`'s whole body, up to `limit` bytes.
pub fn get_whole(network: &dyn Network, url: &Url, limit: u64) -> Result<Vec<u8>, FetchError> {
    let mut body = Vec::new();
    network.get(url, limit, &mut |piece| {
        body.extend_from_slice(piece);
        Ok(())
    })?;
    Ok(body)
}

/// Whether this build fetches `http` from this computer: only one with the
/// testkit, whose tests serve releases there.
const LOOPBACK_HTTP: bool = cfg!(any(test, feature = "testkit"));

/// Whether `url` may be fetched: `https`, or, in a build with the testkit,
/// `http` to a loopback address.
pub fn allowed_url(url: &Url) -> bool {
    allowed(url, LOOPBACK_HTTP)
}

fn allowed(url: &Url, loopback_http: bool) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => loopback_http && is_loopback(url),
        _ => false,
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

/// The error and every cause under it, as one line.
fn described(err: &(dyn std::error::Error + 'static)) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

/// The network, over reqwest, on a runtime of its own.
pub struct Https {
    runtime: tokio::runtime::Runtime,
    verified: Client,
    loopback: Client,
}

impl Https {
    pub fn new() -> Result<Self, String> {
        // Every rustls client in the process uses ring; a provider installed
        // already is kept.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("floti could not start its network runtime: {err}"))?;
        let policy = || {
            redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("too many redirects")
                } else if allowed_url(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("a redirect to an address that is not https")
                }
            })
        };
        let build = |loopback: bool| {
            let builder = Client::builder()
                .tls_backend_rustls()
                .redirect(policy())
                .user_agent(concat!("floti/", env!("FLOTI_VERSION")));
            let builder = if loopback {
                builder.no_proxy()
            } else {
                builder
            };
            builder.build().map_err(|err| {
                format!(
                    "floti could not set up its network client: {}",
                    described(&err)
                )
            })
        };
        Ok(Self {
            runtime,
            verified: build(false)?,
            loopback: build(true)?,
        })
    }
}

impl Network for Https {
    fn get(
        &self,
        url: &Url,
        limit: u64,
        sink: &mut dyn FnMut(&[u8]) -> io::Result<()>,
    ) -> Result<(), FetchError> {
        if !allowed_url(url) {
            return Err(FetchError::Failed(format!("{url} is not an https address")));
        }
        let client = if is_loopback(url) {
            &self.loopback
        } else {
            &self.verified
        };
        self.runtime.block_on(async {
            let mut response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|err| FetchError::Failed(described(&err)))?;
            let status = response.status();
            if status.as_u16() == 404 {
                return Err(FetchError::NotFound);
            }
            if !status.is_success() {
                return Err(FetchError::Status(status.as_u16()));
            }
            let announced = response.content_length();
            if announced.is_some_and(|length| length > limit) {
                return Err(FetchError::TooLarge(limit));
            }
            let mut received: u64 = 0;
            while let Some(piece) = response
                .chunk()
                .await
                .map_err(|err| FetchError::Cut(described(&err)))?
            {
                received += piece.len() as u64;
                if received > limit {
                    return Err(FetchError::TooLarge(limit));
                }
                sink(&piece).map_err(FetchError::Sink)?;
            }
            match announced {
                Some(length) if received < length => Err(FetchError::Cut(format!(
                    "{received} of its {length} bytes arrived"
                ))),
                _ => Ok(()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_build_fetches_https_only() {
        let release = |url: &str| allowed(&Url::parse(url).unwrap(), false);
        assert!(release("https://floti.dev/update.json"));
        assert!(release("https://127.0.0.1/update.json"));
        assert!(!release("http://127.0.0.1:8080/update.json"));
        assert!(!release("http://localhost/update.json"));
        assert!(!release("http://floti.dev/update.json"));
        assert!(!release("file:///update.json"));
    }

    #[test]
    fn a_test_build_also_fetches_http_from_this_computer() {
        let test = |url: &str| allowed_url(&Url::parse(url).unwrap());
        assert!(test("https://floti.dev/update.json"));
        assert!(test("http://127.0.0.1:8080/update.json"));
        assert!(test("http://localhost/update.json"));
        assert!(test("http://[::1]/update.json"));
        assert!(!test("http://floti.dev/update.json"));
        assert!(!test("http://localhost.example.com/"));
        assert!(!test("file:///update.json"));
        assert!(!test("ftp://floti.dev/update.json"));
    }
}
