/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};

use http::Method;
use http::header::{self, HeaderMap, HeaderValue};
use http_body_util::combinators::BoxBody;
use hyper::body::{Bytes, Incoming};
use hyper::{Request as HyperRequest, Response as HyperResponse};
use log::{LevelFilter, Log, Metadata, Record};
use net::cookie::ServoCookie;
use net::cookie_storage::CookieStorage;
use net_traits::blob_url_store::UrlWithBlobClaim;
use net_traits::request::{CredentialsMode, Referrer, RequestBuilder, RequestMode};
use net_traits::{CookieSource, NetworkError};
use servo_url::ServoUrl;

use crate::{fetch_with_context, make_server, new_fetch_context};

struct MemoryLogger(Mutex<Vec<String>>);
static LOGGER: MemoryLogger = MemoryLogger(Mutex::new(Vec::new()));
static INIT: Once = Once::new();
static TEST_LOCK: Mutex<()> = Mutex::new(());
static CAPTURING: AtomicBool = AtomicBool::new(false);

struct Capture(LevelFilter);
impl Drop for Capture {
    fn drop(&mut self) {
        CAPTURING.store(false, Ordering::Release);
        log::set_max_level(self.0);
        LOGGER.0.lock().unwrap().clear();
    }
}

impl Log for MemoryLogger {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        CAPTURING.load(Ordering::Acquire)
    }
    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{} {}", record.target(), record.args()));
        }
    }
    fn flush(&self) {}
}

fn begin_capture() -> Capture {
    INIT.call_once(|| log::set_logger(&LOGGER).unwrap());
    let previous = log::max_level();
    log::set_max_level(LevelFilter::Debug);
    LOGGER.0.lock().unwrap().clear();
    CAPTURING.store(true, Ordering::Release);
    Capture(previous)
}

fn assert_no_markers(markers: &[&str]) {
    let messages = std::mem::take(&mut *LOGGER.0.lock().unwrap());
    // A disabled logger is not evidence of redaction.
    assert!(
        !messages.is_empty(),
        "Expected real network diagnostics at DEBUG"
    );
    for marker in markers {
        assert!(
            messages.iter().all(|message| !message.contains(marker)),
            "Synthetic secret leaked to network diagnostics"
        );
    }
}

#[test]
fn browser_log_privacy_cookie_selection_keeps_values_out_of_logs() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let _capture = begin_capture();
    let url = ServoUrl::parse("https://example.invalid/private?token=COOKIE_QUERY_MARKER").unwrap();
    let mut jar = CookieStorage::new(10);
    let cookie = ServoCookie::from_cookie_string(
        "COOKIE_NAME_MARKER=COOKIE_VALUE_MARKER; Path=/; Secure; HttpOnly",
        &url,
        CookieSource::HTTP,
    )
    .unwrap();
    jar.push(cookie, &url, CookieSource::HTTP);
    assert_eq!(
        jar.cookies_for_url(&url, CookieSource::HTTP).as_deref(),
        Some("COOKIE_NAME_MARKER=COOKIE_VALUE_MARKER")
    );
    assert!(jar.cookies_for_url(&url, CookieSource::NonHTTP).is_none());
    let insecure = ServoUrl::parse("http://example.invalid/").unwrap();
    assert!(jar.cookies_for_url(&insecure, CookieSource::HTTP).is_none());
    assert_no_markers(&[
        "COOKIE_NAME_MARKER",
        "COOKIE_VALUE_MARKER",
        "COOKIE_QUERY_MARKER",
    ]);
}

#[test]
fn browser_log_privacy_http_headers_and_token_url_are_not_logged() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let _capture = begin_capture();
    let handler = |request: HyperRequest<Incoming>,
                   response: &mut HyperResponse<BoxBody<Bytes, hyper::Error>>| {
        assert_eq!(
            request.headers()[header::AUTHORIZATION],
            "Bearer AUTH_VALUE_MARKER"
        );
        assert_eq!(
            request.headers()[header::COOKIE],
            "session=REQUEST_COOKIE_MARKER"
        );
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_static("session=RESPONSE_COOKIE_MARKER; Path=/; HttpOnly"),
        );
        response.headers_mut().insert(
            "x-private-diagnostic",
            HeaderValue::from_static("CUSTOM_HEADER_MARKER"),
        );
    };
    let (server, base) = make_server(handler);
    let url = ServoUrl::parse(&format!(
        "{}PRIVATE_PATH_MARKER?token=QUERY_VALUE_MARKER",
        base.as_str()
    ))
    .unwrap();
    let mut context = new_fetch_context(None, None);
    let cookie = ServoCookie::from_cookie_string(
        "session=REQUEST_COOKIE_MARKER; Path=/; HttpOnly",
        &url,
        CookieSource::HTTP,
    )
    .unwrap();
    context
        .state
        .cookie_jar
        .write()
        .push(cookie, &url, CookieSource::HTTP);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer AUTH_VALUE_MARKER"),
    );
    headers.insert(
        "x-private-request",
        HeaderValue::from_static("CUSTOM_REQUEST_MARKER"),
    );
    let request = RequestBuilder::new(
        None,
        UrlWithBlobClaim::new(url.clone(), None),
        Referrer::NoReferrer,
    )
    .origin(url.origin())
    .credentials_mode(CredentialsMode::Include)
    .headers(headers)
    .build();
    let response = fetch_with_context(request, &mut context);
    server.close();
    assert!(
        !response.is_network_error(),
        "Synthetic fetch must actually complete"
    );
    assert_no_markers(&[
        "AUTH_VALUE_MARKER",
        "REQUEST_COOKIE_MARKER",
        "RESPONSE_COOKIE_MARKER",
        "CUSTOM_HEADER_MARKER",
        "CUSTOM_REQUEST_MARKER",
        "QUERY_VALUE_MARKER",
        "PRIVATE_PATH_MARKER",
    ]);
}

fn check_cors_log_privacy(allow_request: bool) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let _capture = begin_capture();
    let preflight_seen = Arc::new(AtomicBool::new(false));
    let request_seen = Arc::new(AtomicBool::new(false));
    let observed_preflight = preflight_seen.clone();
    let observed_request = request_seen.clone();
    let handler =
        move |request: HyperRequest<Incoming>,
              response: &mut HyperResponse<BoxBody<Bytes, hyper::Error>>| {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            if request.method() == Method::OPTIONS {
                assert_eq!(
                    request.headers()[header::ACCESS_CONTROL_REQUEST_METHOD],
                    "CORS_REQUEST_METHOD_MARKER"
                );
                assert_eq!(
                    request.headers()[header::ACCESS_CONTROL_REQUEST_HEADERS],
                    "x-cors-header-marker"
                );
                observed_preflight.store(true, Ordering::SeqCst);
                response.headers_mut().insert(
                    header::ACCESS_CONTROL_ALLOW_METHODS,
                    HeaderValue::from_static(if allow_request {
                        "CORS_ALLOWED_METHOD_MARKER, CORS_REQUEST_METHOD_MARKER"
                    } else {
                        "CORS_ALLOWED_METHOD_MARKER"
                    }),
                );
                response.headers_mut().insert(
                    header::ACCESS_CONTROL_ALLOW_HEADERS,
                    HeaderValue::from_static("x-cors-header-marker"),
                );
            } else {
                assert_eq!(request.method().as_str(), "CORS_REQUEST_METHOD_MARKER");
                assert_eq!(
                    request.headers()["x-cors-header-marker"],
                    "CORS_HEADER_VALUE_MARKER"
                );
                observed_request.store(true, Ordering::SeqCst);
            }
        };
    let (server, url) = make_server(handler);
    let mut request = RequestBuilder::new(None, url, Referrer::NoReferrer)
        .origin(ServoUrl::parse("http://example.invalid").unwrap().origin())
        .credentials_mode(CredentialsMode::Omit)
        .build();
    request.mode = RequestMode::CorsMode;
    request.unsafe_request = true;
    request.method = Method::from_bytes(b"CORS_REQUEST_METHOD_MARKER").unwrap();
    request.headers.insert(
        "x-cors-header-marker",
        HeaderValue::from_static("CORS_HEADER_VALUE_MARKER"),
    );
    let response = fetch_with_context(request, &mut new_fetch_context(None, None));
    server.close();
    assert!(
        preflight_seen.load(Ordering::SeqCst),
        "Expected preflight; error={:?}",
        response.get_network_error()
    );
    assert_eq!(request_seen.load(Ordering::SeqCst), allow_request);
    if allow_request {
        assert!(
            !response.is_network_error(),
            "Allowed CORS fetch must complete"
        );
    } else {
        assert_eq!(
            response.get_network_error(),
            Some(&NetworkError::CorsMethod)
        );
    }
    assert!(
        LOGGER
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|message| { message.contains("net::http_loader CORS check:") }),
        "Expected real CORS diagnostics, not just unrelated logs"
    );
    assert_no_markers(&[
        "CORS_REQUEST_METHOD_MARKER",
        "CORS_ALLOWED_METHOD_MARKER",
        "x-cors-header-marker",
        "CORS_HEADER_VALUE_MARKER",
    ]);
}

#[test]
fn browser_log_privacy_cors_allowed_request_keeps_values_out_of_logs() {
    check_cors_log_privacy(true);
}

#[test]
fn browser_log_privacy_cors_denied_request_keeps_values_out_of_logs() {
    check_cors_log_privacy(false);
}
