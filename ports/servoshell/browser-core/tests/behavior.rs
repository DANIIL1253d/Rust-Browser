use rust_browser_core::{Profile, parse_subscription, proxy_config};

#[test]
fn bookmark_toggle_removes_duplicate_and_preserves_unicode() {
    let mut p = Profile::default();
    p.toggle_bookmark("https://example.org/", "Пример");
    assert_eq!(p.bookmarks[0].title, "Пример");
    p.toggle_bookmark("https://example.org/", "Another title");
    assert!(p.bookmarks.is_empty());
}

#[test]
fn history_updates_title_without_duplicates_and_ignores_internal_pages() {
    let mut p = Profile::default();
    p.visit("https://example.org/", "Old");
    p.visit("servo:newtab", "New tab");
    p.visit("https://example.org/", "Новый заголовок");
    assert_eq!(p.history.len(), 1);
    assert_eq!(p.history[0].title, "Новый заголовок");
}

#[test]
fn profile_round_trip_preserves_order_and_corrupt_file() {
    let dir = std::env::temp_dir().join(format!("rust-browser-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("profile.json");
    let mut p = Profile::default();
    p.session = vec!["https://b.example/".into(), "https://a.example/".into()];
    p.save(&path).unwrap();
    assert_eq!(Profile::load(&path).unwrap().session, p.session);
    std::fs::write(&path, b"broken profile").unwrap();
    assert!(Profile::load(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"broken profile");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vless_reality_keeps_transport_identity_and_loopback_proxy() {
    let nodes = parse_subscription("vless://11111111-1111-4111-8111-111111111111@edge.example:443?security=reality&sni=www.example.org&fp=chrome&pbk=test-public-key&sid=abcd&flow=xtls-rprx-vision&type=tcp#Test%20node").unwrap();
    assert_eq!(nodes[0].name, "Test node");
    let config = proxy_config(&nodes[0], 18888);
    assert_eq!(config["inbounds"][0]["listen"], "127.0.0.1");
    assert_eq!(config["inbounds"][0]["listen_port"], 18888);
    assert_eq!(config["outbounds"][0]["tls"]["reality"]["short_id"], "abcd");
    assert_eq!(config["route"]["final"], "subscription");
}

#[test]
fn base64_subscription_and_websocket_path_are_decoded() {
    use base64::Engine;
    let text = "trojan://example-password@edge.example:443?security=tls&type=ws&path=%2Fsocket&host=cdn.example#Node";
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let nodes = parse_subscription(&encoded).unwrap();
    assert_eq!(nodes[0].outbound["transport"]["path"], "/socket");
    assert_eq!(
        nodes[0].outbound["transport"]["headers"]["Host"],
        "cdn.example"
    );
}

#[test]
fn unsupported_transport_and_encrypted_happ_fail_without_echoing_secrets() {
    let result = parse_subscription("vless://secret-id@edge.example:443?type=unknown#Private");
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(!err.contains("secret-id"));
    assert!(parse_subscription("happ://crypt4/secret-token").is_err());
}

#[test]
fn xhttp_reality_uses_xray_and_preserves_encoded_path_and_extra() {
    let nodes = parse_subscription("vless://11111111-1111-4111-8111-111111111111@edge.example:443?security=reality&sni=www.example.org&fp=chrome&pbk=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&sid=abcd&type=xhttp&host=cdn.example&path=%2Fstream%3Fed%3D2560&mode=packet-up&extra=%7B%22noGRPCHeader%22%3Atrue%7D#XHTTP").unwrap();
    assert!(nodes[0].uses_xray());
    let config = proxy_config(&nodes[0], 18889);
    assert_eq!(config["inbounds"][0]["listen"], "127.0.0.1");
    assert_eq!(config["inbounds"][0]["port"], 18889);
    assert_eq!(config["outbounds"][0]["protocol"], "vless");
    let stream = &config["outbounds"][0]["streamSettings"];
    assert_eq!(stream["network"], "xhttp");
    assert_eq!(stream["security"], "reality");
    assert_eq!(stream["realitySettings"]["serverName"], "www.example.org");
    assert_eq!(stream["xhttpSettings"]["path"], "/stream?ed=2560");
    assert_eq!(stream["xhttpSettings"]["extra"]["noGRPCHeader"], true);
    assert_eq!(stream["xhttpSettings"]["mode"], "packet-up");
    assert_eq!(config["outbounds"].as_array().unwrap().len(), 1);
}

#[test]
fn happ_add_wrapper_and_encoded_url_are_normalized_without_decoding_inner_token() {
    use rust_browser_core::subscription_input;
    let original = "https://sub.example/s/token%2Fpart?key=a%2Bb";
    assert_eq!(
        subscription_input(&format!("happ://add/{original}")).unwrap(),
        original
    );
    assert_eq!(
        subscription_input(
            "happ://add/https%3A%2F%2Fsub.example%2Fs%2Ftoken%252Fpart%3Fkey%3Da%252Bb"
        )
        .unwrap(),
        original
    );
    assert!(
        subscription_input("happ://crypt4/secret")
            .unwrap_err()
            .contains("Happ")
    );
}
