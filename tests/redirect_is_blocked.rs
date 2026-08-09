//! Proves the allowlist survives a redirect.
//!
//! An integration test, because the only honest way to show a redirect is
//! blocked is to serve one and watch the client refuse it.
//!
//! # Why both hosts are the same machine
//!
//! An earlier version of this test redirected to `attacker.test`, which does
//! not resolve. The request failed either way and the test passed with the
//! guard removed, so it proved nothing.
//!
//! Here the redirect target is `localhost`, which is the same server on the
//! same port but a different hostname. The allowlist holds `127.0.0.1` only.
//! So the second hop is reachable and would return 200 if followed, and the
//! test can tell the difference.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use poe_wayfinder_app::adapter::http_adapter::{HttpAdapter, HttpClient, NetworkPolicy};

/// Serve one 302 then one 200, and report the port.
///
/// The thread is detached and never joined. When the redirect is correctly
/// refused the second connection never arrives, so a join would block forever
/// on the very behaviour the test is checking for.
fn server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binding a local port");
    let port = listener.local_addr().unwrap().port();

    std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };

            let mut buf = [0u8; 2048];
            let read = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..read]).to_string();

            // The first path redirects to the same server under a different
            // hostname. Anything else answers 200 so a followed redirect is
            // visible as a success.
            let response = if request.starts_with("GET /start") {
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/second\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nLEAKED"
                    .to_string()
            };

            let _ = stream.write_all(response.as_bytes());
        }
    });

    port
}

#[tokio::test]
async fn a_redirect_to_an_unlisted_host_is_not_followed() {
    let port = server();

    // 127.0.0.1 is allowed. localhost is not, even though it is the same
    // machine, because the allowlist matches the hostname exactly.
    let policy = NetworkPolicy::new(true, true, "127.0.0.1");
    let http = HttpAdapter::new(policy, Duration::from_secs(5)).unwrap();

    let result = http
        .get(&format!("http://127.0.0.1:{port}/start"), &[])
        .await;

    match result {
        // Refused outright is a pass.
        Err(_) => {}
        Ok(response) => {
            assert_ne!(
                response.body, "LEAKED",
                "the client followed a redirect off the allowlist"
            );
            assert_eq!(
                response.status, 302,
                "the client should have stopped at the redirect"
            );
        }
    }
}

#[tokio::test]
async fn a_redirect_inside_the_allowlist_is_followed() {
    let port = server();

    // Both hostnames allowed, so the hop is legitimate and must go through.
    // Without this the fix would be a denial of service on any API that
    // redirects to its own canonical URL.
    let policy = NetworkPolicy::new(true, true, "127.0.0.1,localhost");
    let http = HttpAdapter::new(policy, Duration::from_secs(5)).unwrap();

    let result = http
        .get(&format!("http://127.0.0.1:{port}/start"), &[])
        .await;

    let response = result.expect("an allowed redirect must be followed");

    assert_eq!(response.status, 200);
    assert_eq!(response.body, "LEAKED");
}
