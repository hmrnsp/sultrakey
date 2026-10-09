//! Downloads release files over HTTPS only. Adapted from lopi `src/update/http.rs`.
//! Certificates are checked against the operating system's list, so an office
//! certificate authority trusted by the server is trusted here too (`SSL_CERT_FILE`
//! overrides on Linux). Proxies come from `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`.
//!
//! Only this module touches the network. Moving releases elsewhere (the office GitLab)
//! means changing [`DEFAULT_BASE`] (or building with `SULTRAKEY_RELEASE_BASE`) and the URL
//! shapes below.

use std::env;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

use super::version::Version;

/// Overrides where releases are looked up (tests, moving to another server). Plain
/// `http://` is accepted for this machine only (127.0.0.1, localhost, [::1]).
pub const URL_ENV: &str = "SULTRAKEY_UPDATE_URL";

/// Set at build time with `SULTRAKEY_RELEASE_BASE`, GitHub otherwise.
pub const DEFAULT_BASE: &str = match option_env!("SULTRAKEY_RELEASE_BASE") {
    Some(base) => base,
    None => "https://github.com/hmrnsp/sultrakey/releases",
};

/// Upper bounds for each download, far above the real sizes.
pub const MANIFEST_LIMIT: u64 = 1024 * 1024;
pub const BINARY_LIMIT: u64 = 64 * 1024 * 1024;
pub const SUMS_LIMIT: u64 = 64 * 1024;

/// The releases: `<base>/latest/download/<file>`, `<base>/download/v<version>/<file>` and
/// `<base>/tag/v<version>`, as on GitHub.
pub struct Releases {
    base: String,
    agent: Agent,
}

impl Releases {
    pub fn from_env() -> Result<Self> {
        match env::var(URL_ENV) {
            Ok(base) if !base.is_empty() => Self::new(&base),
            _ => Self::new(DEFAULT_BASE),
        }
    }

    pub fn new(base: &str) -> Result<Self> {
        let base = base.trim_end_matches('/');
        let local_http = match base.strip_prefix("http://") {
            Some(rest) if is_loopback(rest) => true,
            Some(_) => {
                bail!("{URL_ENV} harus alamat https:// (http:// hanya untuk komputer ini)")
            }
            None if base.starts_with("https://") => false,
            None => bail!("{URL_ENV} harus alamat https://"),
        };
        let mut config = Agent::config_builder()
            .https_only(!local_http)
            .user_agent(format!("sultrakey/{}", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(300)))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            );
        if local_http {
            // A test server on this machine; never send that through a proxy.
            config = config.proxy(None);
        }
        Ok(Self {
            base: base.to_string(),
            agent: config.build().new_agent(),
        })
    }

    /// The manifest of the latest release.
    pub fn latest_manifest(&self) -> Result<Vec<u8>> {
        let url = format!("{}/latest/download/manifest.json", self.base);
        self.get(&url, MANIFEST_LIMIT).map_err(|err| match err {
            Failure::NotFound => anyhow!("belum ada rilis yang diterbitkan di {url}"),
            Failure::Other(err) => err,
        })
    }

    /// A file of the release `version`. Its tag is fixed, so a release published meanwhile
    /// cannot mix into this download.
    pub fn asset(&self, version: Version, name: &str, limit: u64) -> Result<Vec<u8>> {
        let url = format!("{}/download/v{version}/{name}", self.base);
        self.get(&url, limit).map_err(|err| match err {
            Failure::NotFound => anyhow!("rilis v{version} tidak punya file {name}"),
            Failure::Other(err) => err,
        })
    }

    /// The release notes page.
    pub fn page(&self, version: Version) -> String {
        format!("{}/tag/v{version}", self.base)
    }

    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, Failure> {
        let fail = |err: ureq::Error| match err {
            ureq::Error::StatusCode(404) => Failure::NotFound,
            err => Failure::Other(explain(err).context(format!("tidak bisa mengunduh {url}"))),
        };
        let mut response = self.agent.get(url).call().map_err(fail)?;
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(fail)
    }
}

enum Failure {
    NotFound,
    Other(anyhow::Error),
}

/// Adds what to try to the errors people can do something about.
fn explain(err: ureq::Error) -> anyhow::Error {
    let hint = match &err {
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => {
            "koneksi aman gagal; bila jaringan kantor memeriksa HTTPS dengan sertifikatnya sendiri, \
             pasang sertifikat itu di sistem operasi, atau unduh binary manual lalu jalankan \
             `sudo ./sultrakey install`"
        }
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Timeout(_)
        | ureq::Error::Io(_)
        | ureq::Error::ConnectProxyFailed(_) => {
            "periksa koneksi internet, atau set HTTPS_PROXY bila server memakai proxy"
        }
        _ => return err.into(),
    };
    anyhow::Error::new(err).context(hint)
}

/// `host[:port][/path]` names this machine.
fn is_loopback(rest: &str) -> bool {
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().map(|h| format!("[{h}]"))
    } else {
        authority.split(':').next().map(str::to_string)
    };
    matches!(host.as_deref(), Some("127.0.0.1" | "localhost" | "[::1]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_only_for_this_machine() {
        for ok in [
            "https://github.com/hmrnsp/sultrakey/releases",
            "https://example.com/r/",
            "http://127.0.0.1:8080/r",
            "http://localhost:1/",
            "http://[::1]:9/r",
            "http://127.0.0.1",
        ] {
            assert!(Releases::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://example.com/r",
            "http://127.0.0.1.example.com/r",
            "http://localhost.evil:80/",
            "http://[::2]/",
            "ftp://127.0.0.1/",
            "github.com/hmrnsp/sultrakey",
            "",
        ] {
            assert!(Releases::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn builds_release_urls() {
        let releases = Releases::new("https://example.com/r/").unwrap();
        assert_eq!(
            releases.page(Version(0, 5, 0)),
            "https://example.com/r/tag/v0.5.0"
        );
        assert!(DEFAULT_BASE.starts_with("https://"));
    }
}
