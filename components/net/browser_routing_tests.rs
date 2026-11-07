use super::*;
use http_body_util::{BodyExt, Empty};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static PREF_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn browser_routing_tls_indicator_requires_verified_handshake() {
    use net_traits::TlsSecurityState;
    let mut handshake = TlsHandshakeInfo {
        protocol_version: None, cipher_suite: None, kea_group_name: None,
        signature_scheme_name: None, alpn_protocol: None, certificate_chain_der: vec![], used_ech: false,
    };
    assert_eq!(crate::http_loader::build_tls_security_info(&handshake, false, false).state, TlsSecurityState::Insecure);
    handshake.protocol_version = Some(ProtocolVersion::TLSv1_3);
    handshake.cipher_suite = Some(CipherSuite::TLS13_AES_128_GCM_SHA256);
    assert_eq!(crate::http_loader::build_tls_security_info(&handshake, false, false).state, TlsSecurityState::Secure);
    assert_eq!(crate::http_loader::build_tls_security_info(&handshake, false, true).state, TlsSecurityState::Broken);
}

#[test]
fn browser_routing_certificate_exception_is_explicit() {
    let manager = CertificateErrorOverrideManager::new();
    assert!(!manager.has_override(b"test-certificate"));
    manager.add_override(&CertificateDer::from(b"test-certificate".to_vec()));
    assert!(manager.has_override(b"test-certificate"));
    assert!(!manager.has_override(b"different-certificate"));
}
async fn header(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while bytes.len() < 16384 {
        let Ok(byte) = socket.read_u8().await else {
            break;
        };
        bytes.push(byte);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
async fn origin_server() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                loop {
                    if header(&mut socket).await.is_empty() {
                        break;
                    }
                    if socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    });
    (address, hits)
}
async fn proxy_server(
    target: Option<std::net::SocketAddr>,
    reject: bool,
) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let count = count.clone();
            tokio::spawn(async move {
                let request = header(&mut socket).await;
                count.fetch_add(1, Ordering::SeqCst);
                assert!(
                    request.starts_with("CONNECT "),
                    "Expected an HTTP CONNECT tunnel"
                );
                if reject {
                    let _ = socket
                        .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                        .await;
                    return;
                }
                let destination = target
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| request.split_whitespace().nth(1).unwrap().to_owned());
                let mut upstream = TcpStream::connect(destination).await.unwrap();
                socket
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
            });
        }
    });
    (format!("http://{address}"), hits)
}
fn preferences(mode: &str, proxy: &str) {
    let mut p = servo_config::prefs::get().clone();
    p.network_browser_route_mode = mode.into();
    p.network_http_proxy_uri = proxy.into();
    p.network_https_proxy_uri = proxy.into();
    p.network_http_no_proxy = "*".into();
    p.network_browser_direct_domains = String::new();
    p.network_browser_proxy_domains = String::new();
    servo_config::prefs::set(p);
}
fn tls() -> TlsConfig {
    rustls::ClientConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth()
}
async fn get(client: &ServoClient, url: &str) -> Result<(), BoxError> {
    let body: BoxedBody = Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed();
    let response = client
        .request(http::Request::builder().uri(url).body(body).unwrap())
        .await?;
    assert_eq!(response.status(), 200);
    response.into_body().collect().await?;
    Ok(())
}
fn runtime() -> tokio::runtime::Runtime {
    let _ = crate::test_util::create_embedder_proxy();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn browser_routing_proxy_ignores_no_proxy_and_replaces_existing_pool() {
    let _guard = PREF_LOCK.lock().unwrap();
    runtime().block_on(async {
        let (target, _) = origin_server().await;
        let (proxy, hits) = proxy_server(None, false).await;
        preferences("direct", &proxy);
        let client = create_http_client(tls());
        let url = format!("http://{target}/");
        get(&client, &url).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        preferences("proxy", &proxy);
        get(&client, &url).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    });
}
#[test]
fn browser_routing_auto_retries_failed_direct_and_proxy_without_endpoint_fails_closed() {
    let _guard = PREF_LOCK.lock().unwrap();
    runtime().block_on(async {
        let (target, direct_hits) = origin_server().await;
        let (proxy, hits) = proxy_server(Some(target), false).await;
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        preferences("auto", &proxy);
        let client = create_http_client(tls());
        get(&client, &format!("http://{address}/")).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let previous = direct_hits.load(Ordering::SeqCst);
        preferences("proxy", "");
        assert!(get(&client, &format!("http://{target}/")).await.is_err());
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(direct_hits.load(Ordering::SeqCst), previous);
    });
}
#[test]
fn browser_routing_websocket_tls_failure_retries_proxy_before_http_handshake() {
    let _guard = PREF_LOCK.lock().unwrap();
    runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut hello = [0; 1024];
            let _ = socket.read(&mut hello).await;
        });
        let (proxy, hits) = proxy_server(None, true).await;
        preferences("auto", &proxy);
        let result = browser_websocket_transport(
            "127.0.0.1",
            address.port(),
            Some((tls(), "localhost".into())),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn browser_routing_http2_accepts_large_response_headers() {
    use rustls_pki_types::{PrivateKeyDer, pem::PemObject};
    let _guard = PREF_LOCK.lock().unwrap();
    let _ = aws_lc_rs::default_provider().install_default();
    runtime().block_on(async {
        let certificates = vec![
            CertificateDer::from_pem_slice(include_bytes!(
                "tests/self_signed_certificate_for_testing.crt"
            ))
            .unwrap(),
        ];
        let key = PrivateKeyDer::from_pem_slice(include_bytes!("tests/privatekey_for_testing.key"))
            .unwrap();
        let mut server_tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates, key)
            .unwrap();
        server_tls.alpn_protocols = vec![b"h2".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_tls));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let stream = acceptor.accept(socket).await.unwrap();
                    let service = hyper::service::service_fn(
                        |_: http::Request<hyper::body::Incoming>| async {
                            Ok::<_, std::convert::Infallible>(
                                http::Response::builder()
                                    .header("x-large-fixture", "a".repeat(48 * 1024))
                                    .body(http_body_util::Full::new(Bytes::from_static(b"OK")))
                                    .unwrap(),
                            )
                        },
                    );
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor {})
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        preferences("direct", "");
        // Only the isolated loopback test uses this fixture certificate override.
        let client = create_http_client(create_tls_config(
            CACertificates::Default,
            true,
            CertificateErrorOverrideManager::new(),
        ));
        let body: BoxedBody = Empty::<Bytes>::new()
            .map_err(|never| match never {})
            .boxed();
        let request = http::Request::builder()
            .uri(format!("https://{address}/"))
            .body(body)
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), client.request(request))
            .await
            .unwrap();
        assert!(
            response.is_ok(),
            "valid large HTTP/2 response was rejected: {response:?}"
        );
        let response = response.unwrap();
        assert_eq!(response.version(), http::Version::HTTP_2);
        assert_eq!(
            response.headers()["x-large-fixture"].as_bytes().len(),
            48 * 1024
        );
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "OK"
        );
        server.abort();
    });
}

// Explicit developer check against a public endpoint, never part of offline CI.
// The optional proxy must be loopback and contain no credentials.
#[test]
#[ignore = "requires live access to youtube.com; run explicitly with --ignored --nocapture"]
fn browser_routing_live_youtube() {
    let _guard = PREF_LOCK.lock().unwrap();
    let _ = aws_lc_rs::default_provider().install_default();
    let proxy = std::env::var("RUST_BROWSER_DIAGNOSTIC_PROXY").unwrap_or_default();
    if !proxy.is_empty() {
        let uri: Destination = proxy.parse().unwrap();
        assert_eq!(uri.scheme_str(), Some("http"));
        assert_eq!(uri.host(), Some("127.0.0.1"));
        assert!(!uri.authority().unwrap().as_str().contains('@'));
    }
    runtime().block_on(async {
        for mode in ["direct", "auto", "proxy"] {
            if proxy.is_empty() && mode != "direct" {
                continue;
            }
            preferences(mode, &proxy);
            let client = create_http_client(create_tls_config(
                CACertificates::Default,
                false,
                CertificateErrorOverrideManager::new(),
            ));
            let body: BoxedBody = Empty::<Bytes>::new()
                .map_err(|never| match never {})
                .boxed();
            let request = http::Request::builder()
                .uri("https://www.youtube.com/")
                .header(
                    http::header::USER_AGENT,
                    "Mozilla/5.0 Servo/0.7 RustBrowser/0.1",
                )
                .body(body)
                .unwrap();
            let result =
                tokio::time::timeout(Duration::from_secs(25), client.request(request)).await;
            match result {
                Ok(Ok(response)) => {
                    println!(
                        "YouTube {mode}: HTTP {} {:?}",
                        response.status(),
                        response.version()
                    );
                    assert!(response.status().is_success() || response.status().is_redirection());
                },
                Ok(Err(error)) => panic!("YouTube {mode} transport: {error:?}"),
                Err(_) => panic!("YouTube {mode} transport timed out"),
            }
        }
    });
}
