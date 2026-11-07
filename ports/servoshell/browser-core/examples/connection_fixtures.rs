//! Public synthetic fixtures only. Does not read a real subscription or profile.
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("fixture output directory"));
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = [
        (
            "xhttp-reality",
            "vless://11111111-1111-4111-8111-111111111111@server.invalid:443?security=reality&sni=www.example.org&fp=chrome&pbk=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&sid=abcd&type=xhttp&host=cdn.example&path=%2Fstream&mode=auto#PublicFixture",
        ),
        (
            "xhttp-tls",
            "vless://11111111-1111-4111-8111-111111111111@server.invalid:443?security=tls&sni=server.invalid&type=xhttp&path=%2Fstream&mode=packet-up#PublicFixture",
        ),
        (
            "vless-ws",
            "vless://11111111-1111-4111-8111-111111111111@server.invalid:443?security=tls&sni=server.invalid&type=ws&path=%2Fsocket&host=server.invalid#PublicFixture",
        ),
        (
            "trojan",
            "trojan://public-test-password@server.invalid:443?security=tls#PublicFixture",
        ),
        (
            "shadowsocks",
            "ss://YWVzLTEyOC1nY206cHVibGljLXRlc3QtcGFzc3dvcmQ@server.invalid:8388#PublicFixture",
        ),
    ];
    for (name, uri) in fixtures {
        let node = rust_browser_core::parse_subscription(uri)
            .unwrap()
            .remove(0);
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&rust_browser_core::proxy_config(&node, 19991)).unwrap(),
        )
        .unwrap();
    }
}
