/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::RefCell;
use std::collections::hash_map::HashMap;
use std::convert::TryFrom;
use std::error::Error;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use std::{fmt, io};

use futures::task::{Context, Poll};
use futures::{Future, TryFutureExt};
use http::uri::{Authority, Uri as Destination};
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::rt::Executor;
use hyper_rustls::{HttpsConnector as HyperRustlsHttpsConnector, MaybeHttpsStream};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::proxy::Tunnel;
use hyper_util::client::legacy::connect::{
    Connected, Connection, HttpConnector as HyperHttpConnector,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use log::warn;
use parking_lot::Mutex;
use rustls::client::danger::ServerCertVerifier;
use rustls::client::{ClientConnection, EchStatus};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use rustls::{CipherSuite, ClientConfig, NamedGroup, ProtocolVersion};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use servo_config::pref;
use tokio::net::TcpStream;
use tower::Service;

use crate::async_runtime::spawn_task;
use crate::hosts::replace_host;

#[cfg(test)]
#[path = "browser_routing_tests.rs"]
mod browser_routing_tests;

pub const BUF_SIZE: usize = 32768;

/// ALPN identifier for HTTP/2 (RFC 7540 §3.1).
pub const ALPN_H2: &str = "h2";

#[derive(Clone)]
pub struct ServoHttpConnector {
    inner: HyperHttpConnector,
}

impl ServoHttpConnector {
    fn new() -> ServoHttpConnector {
        let mut inner = HyperHttpConnector::new();
        inner.enforce_http(false);
        inner.set_happy_eyeballs_timeout(None);
        inner.set_connect_timeout(Some(Duration::from_secs(pref!(network_connection_timeout))));
        ServoHttpConnector { inner }
    }
}

impl Service<Destination> for ServoHttpConnector {
    type Response = TokioIo<TcpStream>;
    type Error = ConnectionError;
    type Future =
        std::pin::Pin<Box<dyn Future<Output = Result<TokioIo<TcpStream>, ConnectionError>> + Send>>;

    fn call(&mut self, dest: Destination) -> Self::Future {
        // Perform host replacement when making the actual TCP connection.
        let mut new_dest = dest.clone();
        let mut parts = dest.into_parts();

        if let Some(auth) = parts.authority {
            let host = auth.host();
            let host = replace_host(host);

            let authority = if let Some(port) = auth.port() {
                format!("{}:{}", host, port.as_str())
            } else {
                (*host).to_string()
            };

            if let Ok(authority) = Authority::from_maybe_shared(authority) {
                parts.authority = Some(authority);
                if let Ok(dest) = Destination::from_parts(parts) {
                    new_dest = dest
                }
            }
        }

        Box::pin(
            self.inner
                .call(new_dest)
                .map_err(|error| ConnectionError::HttpError(BoxError::from(error))),
        )
    }

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Ok(()).into()
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone)]
pub struct InstrumentedConnector<T> {
    inner: HyperRustlsHttpsConnector<T>,
}

impl<T> InstrumentedConnector<T> {
    fn new(inner: HyperRustlsHttpsConnector<T>) -> Self {
        Self { inner }
    }
}

impl<T> From<HyperRustlsHttpsConnector<T>> for InstrumentedConnector<T> {
    fn from(inner: HyperRustlsHttpsConnector<T>) -> Self {
        Self::new(inner)
    }
}

pub struct InstrumentedStream<T> {
    inner: MaybeHttpsStream<T>,
    tls_info: RefCell<Option<TlsHandshakeInfo>>,
}

impl<T: Unpin> Unpin for InstrumentedStream<T> {}

impl<T> fmt::Debug for InstrumentedStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstrumentedStream")
            .field("tls_info", &self.tls_info)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TlsHandshakeInfo {
    pub protocol_version: Option<ProtocolVersion>,
    pub cipher_suite: Option<CipherSuite>,
    pub kea_group_name: Option<NamedGroup>,
    pub signature_scheme_name: Option<String>,
    pub alpn_protocol: Option<String>,
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub used_ech: bool,
}

impl TlsHandshakeInfo {
    fn from_connection(conn: &ClientConnection) -> Self {
        let protocol_version = conn.protocol_version();
        let cipher_suite = conn.negotiated_cipher_suite().map(|suite| suite.suite());
        let kea_group_name = conn
            .negotiated_key_exchange_group()
            .map(|group| group.name());
        let certificate_chain_der = conn
            .peer_certificates()
            .map(|certs| certs.iter().map(|cert| cert.as_ref().to_vec()).collect())
            .unwrap_or_default();
        let alpn_protocol = conn
            .alpn_protocol()
            .map(|proto| String::from_utf8_lossy(proto).into_owned());
        let used_ech = matches!(conn.ech_status(), EchStatus::Accepted);

        Self {
            protocol_version,
            cipher_suite,
            kea_group_name,
            signature_scheme_name: None,
            alpn_protocol,
            certificate_chain_der,
            used_ech,
        }
    }
}

impl<T> InstrumentedStream<T>
where
    T: Connection + hyper::rt::Read + hyper::rt::Write + Unpin,
{
    fn from_maybe_https_stream(stream: MaybeHttpsStream<T>) -> Self {
        match stream {
            MaybeHttpsStream::Http(inner) => Self {
                inner: MaybeHttpsStream::Http(inner),
                tls_info: RefCell::new(None),
            },
            MaybeHttpsStream::Https(tls_stream) => {
                let (_tcp, tls) = tls_stream.inner().get_ref();
                let tls_info = TlsHandshakeInfo::from_connection(tls);

                Self {
                    inner: MaybeHttpsStream::Https(tls_stream),
                    tls_info: RefCell::new(Some(tls_info)),
                }
            },
        }
    }
}

impl<T> Connection for InstrumentedStream<T>
where
    T: Connection + hyper::rt::Read + hyper::rt::Write + Unpin,
{
    fn connected(&self) -> Connected {
        let connected = match &self.inner {
            MaybeHttpsStream::Http(stream) => stream.connected(),
            MaybeHttpsStream::Https(stream) => {
                let (tcp, tls) = stream.inner().get_ref();
                if tls.alpn_protocol() == Some(ALPN_H2.as_bytes()) {
                    tcp.inner().connected().negotiated_h2()
                } else {
                    tcp.inner().connected()
                }
            },
        };
        if let Some(info) = self.tls_info.borrow_mut().take() {
            connected.extra(info)
        } else {
            connected
        }
    }
}

impl<T> hyper::rt::Read for InstrumentedStream<T>
where
    T: Connection + hyper::rt::Read + hyper::rt::Write + Unpin,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T> hyper::rt::Write for InstrumentedStream<T>
where
    T: Connection + hyper::rt::Read + hyper::rt::Write + Unpin,
{
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }
}

impl<T> Service<Destination> for InstrumentedConnector<T>
where
    T: Service<Destination>,
    T::Response: Connection + hyper::rt::Read + hyper::rt::Write + Send + Unpin + 'static,
    T::Future: Send + 'static,
    T::Error: Into<BoxError>,
{
    type Response = InstrumentedStream<T::Response>;
    type Error = BoxError;
    type Future = std::pin::Pin<
        Box<dyn Future<Output = Result<InstrumentedStream<T::Response>, BoxError>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, dst: Destination) -> Self::Future {
        let future = self.inner.call(dst);
        Box::pin(async move {
            let stream = future.await.map_err(|error| -> BoxError { error })?;
            Ok(InstrumentedStream::from_maybe_https_stream(stream))
        })
    }
}

pub type Connector = InstrumentedConnector<ServoHttpConnector>;
pub type TlsConfig = ClientConfig;

#[derive(Clone, Debug, Default)]
struct CertificateErrorOverrideManagerInternal {
    /// A mapping of certificates and their hosts, which have seen certificate errors.
    /// This is used to later create an override in this [CertificateErrorOverrideManager].
    certificates_failing_to_verify: HashMap<ServerName<'static>, CertificateDer<'static>>,
    /// A list of certificates that should be accepted despite encountering verification
    /// errors.
    overrides: Vec<CertificateDer<'static>>,
}

/// This data structure is used to track certificate verification errors and overrides.
/// It tracks:
///  - A list of [Certificate]s with verification errors mapped by their [ServerName]
///  - A list of [Certificate]s for which to ignore verification errors.
#[derive(Clone, Debug, Default)]
pub struct CertificateErrorOverrideManager(Arc<Mutex<CertificateErrorOverrideManagerInternal>>);

impl CertificateErrorOverrideManager {
    pub fn new() -> Self {
        Self(Default::default())
    }

    /// Add a certificate to this manager's list of certificates for which to ignore
    /// validation errors.
    pub fn add_override(&self, certificate: &CertificateDer<'static>) {
        self.0.lock().overrides.push(certificate.clone());
    }

    /// An accepted certificate exception must not be displayed as verified TLS.
    pub(crate) fn has_override(&self, certificate: &[u8]) -> bool {
        self.0.lock().overrides.iter().any(|cert| cert.as_ref() == certificate)
    }

    /// Given the a string representation of a sever host name, remove information about
    /// a [Certificate] with verification errors. If a certificate with
    /// verification errors was found, return it, otherwise None.
    pub(crate) fn remove_certificate_failing_verification(
        &self,
        host: &str,
    ) -> Option<CertificateDer<'static>> {
        let server_name = match ServerName::try_from(host) {
            Ok(name) => name.to_owned(),
            Err(error) => {
                warn!("Could not convert host string into RustTLS ServerName: {error:?}");
                return None;
            },
        };
        self.0
            .lock()
            .certificates_failing_to_verify
            .remove(&server_name)
    }
}

#[derive(Clone, Debug, Default)]
pub enum CACertificates<'de> {
    #[default]
    Default,
    Override(Vec<CertificateDer<'de>>),
}

/// Create a [TlsConfig] to use for managing a HTTP connection. This currently creates
/// a rustls [ClientConfig].
///
/// FIXME: The `ignore_certificate_errors` argument ignores all certificate errors. This
/// is used when running the WPT tests, because rustls currently rejects the WPT certificiate.
/// See <https://github.com/servo/servo/issues/30080>
#[servo_tracing::instrument(skip_all)]
pub fn create_tls_config(
    ca_certificates: CACertificates<'static>,
    ignore_certificate_errors: bool,
    override_manager: CertificateErrorOverrideManager,
) -> TlsConfig {
    let verifier = CertificateVerificationOverrideVerifier::new(
        ca_certificates,
        ignore_certificate_errors,
        override_manager,
    );
    // TODO: After <https://github.com/rustls/rustls-platform-verifier/pull/204> is merged,
    // `dangerous` can be removed.
    rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth()
}

#[derive(Clone)]
struct TokioExecutor {}

impl<F> Executor<F> for TokioExecutor
where
    F: Future<Output = ()> + 'static + std::marker::Send,
{
    fn execute(&self, fut: F) {
        spawn_task(fut);
    }
}

static CRYPTO_PROVIDER_CACHE: LazyLock<Arc<CryptoProvider>> = LazyLock::new(|| {
    CryptoProvider::get_default()
        .cloned()
        // The embedder should have initialized the default crypto provider before
        // initializing servo, so this should never fail.
        .unwrap_or_else(|| {
            warn!("Default crypto provider not initialized before first access in connector.");
            Arc::new(aws_lc_rs::default_provider())
        })
});

/// A cache for the default rustls platform verifier.
///
/// Instantiating a new verifier can be expensive, since it can read through all certificates:
/// <https://github.com/rustls/rustls-platform-verifier/blob/996b1c903491641b17b3c9afb65d1352f6fc6b76/rustls-platform-verifier/src/verification/others.rs#L92>
static RUSTLS_PLATFORM_VERIFIER_CACHE: LazyLock<Arc<rustls_platform_verifier::Verifier>> =
    LazyLock::new(|| {
        Arc::new(
            rustls_platform_verifier::Verifier::new(CRYPTO_PROVIDER_CACHE.clone())
                .expect("Could not initialize platform certificate verifier"),
        )
    });

/// Prewarm the TLS stack to speed up the first connection
///
/// Currently, this force-seeds the crypto provider (from aws_lc_rs),
/// which on my system takes around 30-50ms according to samply, spent in
/// `tree_jitter_initialize_once`. If we don't call this function, then
/// the initialization will happen much later, on a tokio runtime thread.
#[inline]
pub fn prewarm_tls() {
    #[servo_tracing::instrument]
    fn prewarm_tls_impl() {
        let mut sink = [0u8; 32];
        // The first access can be slow, if the provider needs to gather entropy.
        let _ = CRYPTO_PROVIDER_CACHE.secure_random.fill(&mut sink);
        // Note: We don't need to explicitly force initialize RUSTLS_PLATFORM_VERIFIER_CACHE,
        // since the resource manager thread will do that during startup.
    }

    if let Err(error) = std::thread::Builder::new()
        .name("Net-TLS-prewarm".into())
        .spawn(prewarm_tls_impl)
    {
        warn!("Failed to spawn thread to prewarm TLS: {error:?}");
    }
}

#[derive(Debug)]
struct CertificateVerificationOverrideVerifier {
    main_verifier: Arc<dyn ServerCertVerifier>,
    ignore_certificate_errors: bool,
    override_manager: CertificateErrorOverrideManager,
}

impl CertificateVerificationOverrideVerifier {
    fn new(
        ca_certficates: CACertificates<'static>,
        ignore_certificate_errors: bool,
        override_manager: CertificateErrorOverrideManager,
    ) -> Self {
        // From <https://github.com/rustls/rustls-platform-verifier/blob/main/README.md>:
        // > Some manual setup is required, outside of cargo, to use this crate on
        // > Android. In order to use Android's certificate verifier, the crate needs to
        // > call into the JVM. A small Kotlin component must be included in your app's
        // > build to support rustls-platform-verifier.
        //
        // Since we cannot count on embedders to do this setup, just stick with webpki roots
        // on Android.
        let use_webpki_roots = cfg!(target_os = "android") || pref!(network_use_webpki_roots);
        let main_verifier = if !use_webpki_roots {
            let verifier = match ca_certficates {
                CACertificates::Default => RUSTLS_PLATFORM_VERIFIER_CACHE.clone(),
                // Android doesn't support `Verifier::new_with_extra_roots`, but currently Android
                // never uses the platform verifier at all.
                CACertificates::Override(_certificates) => {
                    #[cfg(target_os = "android")]
                    unreachable!("Android should always use the WebPKI verifier.");
                    #[cfg(not(target_os = "android"))]
                    {
                        let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
                            _certificates,
                            CRYPTO_PROVIDER_CACHE.clone(),
                        )
                        .expect("Could not initialize platform certificate verifier");
                        Arc::new(verifier)
                    }
                },
            };
            verifier as Arc<dyn ServerCertVerifier>
        } else {
            let mut root_store =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            match ca_certficates {
                CACertificates::Default => {},
                CACertificates::Override(certificates) => {
                    for certificate in certificates {
                        if root_store.add(certificate).is_err() {
                            log::error!("Could not add an override certificate.");
                        }
                    }
                },
            }
            rustls::client::WebPkiServerVerifier::builder(root_store.into())
                .build()
                .expect("Could not initialize platform certificate verifier.")
                as Arc<dyn ServerCertVerifier>
        };

        Self {
            main_verifier,
            ignore_certificate_errors,
            override_manager,
        }
    }
}

impl rustls::client::danger::ServerCertVerifier for CertificateVerificationOverrideVerifier {
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.main_verifier
            .verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.main_verifier
            .verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.main_verifier.supported_verify_schemes()
    }

    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let error = match self.main_verifier.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(result) => return Ok(result),
            Err(error) => error,
        };

        if self.ignore_certificate_errors {
            warn!("Ignoring certficate error: {error:?}");
            return Ok(rustls::client::danger::ServerCertVerified::assertion());
        }

        // If there's an override for this certificate, just accept it.
        for cert_with_exception in &*self.override_manager.0.lock().overrides {
            if *end_entity == *cert_with_exception {
                return Ok(rustls::client::danger::ServerCertVerified::assertion());
            }
        }
        self.override_manager
            .0
            .lock()
            .certificates_failing_to_verify
            .insert(server_name.to_owned(), end_entity.clone().into_owned());
        Err(error)
    }
}

pub type BoxedBody = BoxBody<Bytes, hyper::Error>;

#[derive(Debug)]
/// The error type for the MaybeProxyConnector
pub enum ConnectionError {
    HttpError(BoxError),
    // It looks like currently the type is not exported.
    ProxyError(BoxError),
}

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ConnectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ConnectionError::HttpError(error) => error.source(),
            ConnectionError::ProxyError(error) => error.source(),
        }
    }
}

#[derive(Clone)]
/// A proxy connector. This will automatically open a proxy connection if the uri matches the proxy uri.
/// Also respects 'no_proxy'.
pub struct ProxyConnector {
    /// A client without proxy for `no_proxy` matches.
    client: ServoHttpConnector,
    /// Matcher to see if we should forward to the proxy or not.
    use_proxy: bool,
}

impl ProxyConnector {
    fn new(use_proxy: bool) -> Self {
        ProxyConnector {
            client: ServoHttpConnector::new(),
            use_proxy,
        }
    }
}

// Just forward everything to the inner type except that we modify the errors returned.
impl Service<Destination> for ProxyConnector {
    type Response = TokioIo<TcpStream>;
    type Error = ConnectionError;
    type Future =
        std::pin::Pin<Box<dyn Future<Output = Result<TokioIo<TcpStream>, ConnectionError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.client
            .poll_ready(cx)
            .map_err(|error| ConnectionError::ProxyError(BoxError::from(error)))
    }

    fn call(&mut self, req: Destination) -> Self::Future {
        if self.use_proxy && !pref!(network_browser_route_mode).is_empty() {
            let endpoint = if req.scheme_str() == Some("https") {
                pref!(network_https_proxy_uri)
            } else {
                pref!(network_http_proxy_uri)
            };
            if endpoint.is_empty() {
                return Box::pin(async {
                    Err(ConnectionError::ProxyError(
                        "Browser proxy is not configured".into(),
                    ))
                });
            }
        }
        // Rebuild from preferences so switching the subscription does not require restarting Servo.
        let matcher = hyper_util::client::proxy::matcher::Matcher::builder()
            .http(if self.use_proxy {
                pref!(network_http_proxy_uri)
            } else {
                String::new()
            })
            .https(if self.use_proxy {
                pref!(network_https_proxy_uri)
            } else {
                String::new()
            })
            .no(if pref!(network_browser_route_mode).is_empty() {
                pref!(network_http_no_proxy)
            } else {
                String::new()
            })
            .build();
        match matcher.intercept(&req) {
            Some(intercept) => {
                let mut tunnel = Tunnel::new(intercept.uri().clone(), self.client.clone());
                let final_tunnel = if let Some(auth) = intercept.basic_auth() {
                    tunnel.with_auth(auth.clone())
                } else {
                    tunnel
                }
                .call(req)
                .map_err(|error| ConnectionError::ProxyError(BoxError::from(error)));
                Box::pin(final_tunnel)
            },
            None => Box::pin(
                self.client
                    .call(req)
                    .map_err(|error| ConnectionError::ProxyError(BoxError::from(error))),
            ),
        }
    }
}

type BrowserHyperClient = Client<BrowserConnector, BoxedBody>;

#[derive(Clone)]
pub struct ServoClient {
    current: Arc<Mutex<(String, BrowserHyperClient)>>,
    tls_config: TlsConfig,
}
fn route_signature() -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        pref!(network_browser_route_mode),
        pref!(network_http_proxy_uri),
        pref!(network_https_proxy_uri),
        pref!(network_http_no_proxy),
        pref!(network_browser_direct_domains),
        pref!(network_browser_proxy_domains)
    )
}
impl ServoClient {
    pub fn request(
        &self,
        request: http::Request<BoxedBody>,
    ) -> hyper_util::client::legacy::ResponseFuture {
        let signature = route_signature();
        let mut current = self.current.lock();
        if current.0 != signature {
            // A route change also replaces the connection pool. New requests cannot reuse
            // a direct HTTP/2 connection after the user selects the proxy.
            *current = (signature, build_browser_client(self.tls_config.clone()));
        }
        current.1.request(request)
    }
}

static PROXY_ROUTE_CACHE: LazyLock<Arc<Mutex<HashMap<String, std::time::Instant>>>> =
    LazyLock::new(Default::default);
fn route_cache_key(endpoint: &str, host: &str, port: u16) -> String {
    format!("{endpoint}|{host}:{port}")
}

/// Chooses a route before the HTTP request is sent. A failed TLS/TCP attempt can be
/// retried safely here: no form, upload, or other HTTP request has been transmitted.
#[derive(Clone)]
pub struct BrowserConnector {
    direct: InstrumentedConnector<ProxyConnector>,
    proxy: InstrumentedConnector<ProxyConnector>,
    proxy_hosts: Arc<Mutex<HashMap<String, std::time::Instant>>>,
}

fn domain_in_rules(host: &str, rules: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    rules
        .split([',', ';', '\n', ' '])
        .filter(|s| !s.is_empty())
        .any(|rule| {
            let rule = rule
                .trim_start_matches("*.")
                .trim_end_matches('.')
                .to_ascii_lowercase();
            host == rule || host.ends_with(&format!(".{rule}"))
        })
}

impl Service<Destination> for BrowserConnector {
    type Response = InstrumentedStream<TokioIo<TcpStream>>;
    type Error = BoxError;
    type Future = std::pin::Pin<Box<dyn Future<Output = Result<Self::Response, BoxError>> + Send>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, dst: Destination) -> Self::Future {
        let mode = pref!(network_browser_route_mode);
        if mode.is_empty() {
            return self.proxy.call(dst);
        }
        let host = dst.host().unwrap_or("").to_owned();
        let endpoint = pref!(network_https_proxy_uri);
        let direct =
            mode == "direct" || domain_in_rules(&host, &pref!(network_browser_direct_domains));
        if direct {
            return self.direct.call(dst);
        }
        let cache_key = route_cache_key(
            &endpoint,
            &host,
            dst.port_u16()
                .unwrap_or(if dst.scheme_str() == Some("https") {
                    443
                } else {
                    80
                }),
        );
        let cached = self
            .proxy_hosts
            .lock()
            .get(&cache_key)
            .is_some_and(|time| time.elapsed() < Duration::from_secs(600));
        if mode == "proxy"
            || domain_in_rules(&host, &pref!(network_browser_proxy_domains))
            || cached
        {
            return self.proxy.call(dst);
        }
        let first = self.direct.call(dst.clone());
        let mut proxy = self.proxy.clone();
        let cache = self.proxy_hosts.clone();
        Box::pin(async move {
            if let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(3), first).await {
                return Ok(stream);
            }
            let stream = proxy.call(dst).await?;
            let mut hosts = cache.lock();
            if hosts.len() >= 1024 {
                hosts.clear();
            }
            hosts.insert(cache_key, std::time::Instant::now());
            Ok(stream)
        })
    }
}

pub fn create_http_client(tls_config: TlsConfig) -> ServoClient {
    let client = build_browser_client(tls_config.clone());
    ServoClient {
        current: Arc::new(Mutex::new((route_signature(), client))),
        tls_config,
    }
}

fn build_browser_client(tls_config: TlsConfig) -> BrowserHyperClient {
    let direct = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config.clone())
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(ProxyConnector::new(false));
    let proxy = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(ProxyConnector::new(true));

    Client::builder(TokioExecutor {})
        .http1_title_case_headers(true)
        // This is necessary to allow hyper to perform various background tasks for HTTP/2.
        .timer(TokioTimer::new())
        // This is necessary for hyper to reap unused idle keep-alive connections.
        .pool_timer(TokioTimer::new())
        // Other browsers control the maximum connections per host, tyipcally set to 6, but hyper
        // does not allow controlling this directly. Until we have code to manually do that, this
        // limit controls the maximum idle connections per host, which we still don't want to grow
        // without bound.
        .pool_max_idle_per_host(6)
        // Large CSP/reporting headers on sites such as YouTube exceed Hyper's
        // small default. Retain a finite decoded-header budget per HTTP/2 stream.
        .http2_max_header_list_size(256 * 1024)
        .build(BrowserConnector {
            direct: direct.into(),
            proxy: proxy.into(),
            proxy_hosts: PROXY_ROUTE_CACHE.clone(),
        })
}

pub(crate) trait BrowserSocket:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send
{
}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> BrowserSocket for T {}
pub(crate) type WebSocketTransport = Box<dyn BrowserSocket>;

/// Completes TCP *and TLS* before choosing a route. No WebSocket HTTP request has
/// been sent during the direct attempt, so a retry cannot repeat that request.
pub(crate) async fn browser_websocket_transport(
    host: &str,
    port: u16,
    tls: Option<(TlsConfig, String)>,
) -> Result<WebSocketTransport, BoxError> {
    let mode = pref!(network_browser_route_mode);
    let endpoint = pref!(network_https_proxy_uri);
    let direct = mode == "direct"
        || (mode.is_empty() && endpoint.is_empty())
        || domain_in_rules(host, &pref!(network_browser_direct_domains));
    let key = route_cache_key(&endpoint, host, port);
    let cached = PROXY_ROUTE_CACHE
        .lock()
        .get(&key)
        .is_some_and(|t| t.elapsed() < Duration::from_secs(600));
    let forced = mode == "proxy"
        || mode.is_empty()
        || domain_in_rules(host, &pref!(network_browser_proxy_domains))
        || cached;
    if direct || !forced {
        let attempt = tokio::time::timeout(
            Duration::from_secs(if direct { 15 } else { 3 }),
            open_websocket_transport(host, port, tls.clone(), false),
        )
        .await;
        match attempt {
            Ok(Ok(socket)) => return Ok(socket),
            _ if direct => {
                return Err(io::Error::other("WebSocket direct connection failed").into());
            },
            _ => {},
        }
    }
    let socket = tokio::time::timeout(
        Duration::from_secs(15),
        open_websocket_transport(host, port, tls, true),
    )
    .await
    .map_err(|_| io::Error::other("WebSocket proxy connection timed out"))??;
    let mut cache = PROXY_ROUTE_CACHE.lock();
    if cache.len() >= 1024 {
        cache.clear();
    }
    cache.insert(key, std::time::Instant::now());
    Ok(socket)
}

async fn open_websocket_transport(
    host: &str,
    port: u16,
    tls: Option<(TlsConfig, String)>,
    proxy: bool,
) -> Result<WebSocketTransport, BoxError> {
    let socket = if proxy {
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let target: Destination = format!("https://{host}:{port}/").parse()?;
        ProxyConnector::new(true).call(target).await?.into_inner()
    } else {
        TcpStream::connect((host, port)).await?
    };
    if let Some((config, name)) = tls {
        let name = ServerName::try_from(name)?;
        let stream = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, socket)
            .await?;
        Ok(Box::new(stream))
    } else {
        Ok(Box::new(socket))
    }
}
