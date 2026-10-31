use allowit_sdk::transport::rpc::{EnvelopeValidation, ErrorKind, Options, RpcClient};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

fn server(
    status: u16,
    body: Value,
    delay: Duration,
    extra: &str,
) -> (String, thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let extra = extra.to_owned();
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let header_end;
        loop {
            let mut buf = [0; 1024];
            let n = socket.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(p) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                header_end = p + 4;
                break;
            }
        }
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < header_end + length {
            let mut buf = [0; 1024];
            let n = socket.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let request = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        thread::sleep(delay);
        let body = serde_json::to_vec(&body).unwrap();
        let response = format!(
            "HTTP/1.1 {status} Reply\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes());
        let _ = socket.write_all(&body);
        listener.set_nonblocking(true).unwrap();
        assert!(
            listener.accept().is_err(),
            "The transport must not resubmit a call"
        );
        request
    });
    (url, handle)
}

#[test]
fn strict_identity_and_legacy_null_semantics_are_preserved() {
    let (url, server) = server(
        200,
        json!({"jsonrpc":"2.0","id":9,"result":null}),
        Duration::ZERO,
        "",
    );
    let rpc = RpcClient::new(&url, Duration::from_secs(1), 1024).unwrap();
    assert_eq!(
        rpc.call_with_id(
            "getGenesisHash",
            &json!([]),
            9,
            EnvelopeValidation::StrictIdentity,
            None
        )
        .unwrap(),
        Value::Null
    );
    assert_eq!(
        server.join().unwrap(),
        json!({"jsonrpc":"2.0","id":9,"method":"getGenesisHash","params":[]})
    );
    let (url, server) = self::server(200, json!({"result":null}), Duration::ZERO, "");
    assert_eq!(
        RpcClient::new(&url, Duration::from_secs(1), 1024)
            .unwrap()
            .call("getTransaction", &json!(["signature"]))
            .unwrap(),
        Value::Null
    );
    server.join().unwrap();
    let (url, server) = self::server(
        200,
        json!({"jsonrpc":"2.0","id":2,"result":"value"}),
        Duration::ZERO,
        "",
    );
    assert_eq!(
        RpcClient::new(&url, Duration::from_secs(1), 1024)
            .unwrap()
            .call_with_id(
                "method",
                &json!([]),
                1,
                EnvelopeValidation::StrictIdentity,
                None
            )
            .unwrap_err()
            .kind,
        ErrorKind::Envelope
    );
    server.join().unwrap();
}

#[test]
fn retry_metadata_has_no_upstream_body_or_credential_and_never_resubmits() {
    let (url, server) = server(
        429,
        json!({"secret":"upstream-private-detail"}),
        Duration::ZERO,
        "Retry-After: 3\r\n",
    );
    let error = RpcClient::new(&url, Duration::from_secs(1), 1024)
        .unwrap()
        .call("sendTransaction", &json!(["retained-public-packet"]))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Http);
    assert_eq!(error.http_status, Some(429));
    assert_eq!(error.retry_after_seconds, Some(3));
    assert!(!format!("{error:?} {error}").contains("upstream-private-detail"));
    assert!(!error.to_string().contains(&url));
    server.join().unwrap();
    let (url, server) = self::server(
        200,
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32005,"message":"provider detail"}}),
        Duration::ZERO,
        "",
    );
    let error = RpcClient::new(&url, Duration::from_secs(1), 1024)
        .unwrap()
        .call("read", &json!([]))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Refused);
    assert_eq!(error.rpc_code, Some(-32005));
    assert!(!error.to_string().contains("provider detail"));
    server.join().unwrap();
}

#[test]
fn byte_bound_and_expired_deadline_fail_closed() {
    let (url, server) = server(200, json!({"result":"x".repeat(1024)}), Duration::ZERO, "");
    assert_eq!(
        RpcClient::new(&url, Duration::from_secs(1), 100)
            .unwrap()
            .call("read", &json!([]))
            .unwrap_err()
            .kind,
        ErrorKind::Size
    );
    server.join().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let rpc = RpcClient::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        Duration::from_secs(1),
        1024,
    )
    .unwrap();
    assert_eq!(
        rpc.call_before(
            "read",
            &json!([]),
            Some(Instant::now() - Duration::from_secs(1))
        )
        .unwrap_err()
        .kind,
        ErrorKind::Deadline
    );
    assert!(listener.accept().is_err());
}

#[test]
fn remaining_deadline_caps_the_entire_response() {
    let (url, server) = server(200, json!({"result":true}), Duration::from_millis(150), "");
    let rpc = RpcClient::with_options(
        &url,
        Options {
            timeout: Duration::from_secs(1),
            max_response_bytes: 1024,
            no_proxy: true,
            connect_timeout: Some(Duration::from_millis(100)),
        },
    )
    .unwrap();
    let now = Instant::now();
    assert!(
        rpc.call_before("read", &json!([]), Some(now + Duration::from_millis(30)))
            .is_err()
    );
    assert!(now.elapsed() < Duration::from_millis(120));
    server.join().unwrap();
}

#[test]
fn endpoint_rebinding_preserves_the_new_origin_and_response_policy() {
    let (one, first) = server(200, json!({"result":"first"}), Duration::ZERO, "");
    let (two, second) = server(201, json!({"result":"second"}), Duration::ZERO, "");
    let rpc = RpcClient::new(&one, Duration::from_secs(1), 1024).unwrap();
    assert_eq!(rpc.call("read", &json!([])).unwrap(), "first");
    assert_eq!(
        rpc.accept_success_statuses()
            .for_endpoint(&two)
            .unwrap()
            .call("read", &json!([]))
            .unwrap(),
        "second"
    );
    first.join().unwrap();
    second.join().unwrap();
}

#[test]
fn neutral_refusal_metadata_preserves_shape_without_vendor_retry_policy() {
    for (body, valid) in [
        (json!({"error":{"code":-1,"message":"refused"}}), true),
        (json!({"error":{"code":-32005,"message":42}}), false),
        (
            json!({"error":{"code":-32005,"message":"refused"},"result":null}),
            false,
        ),
    ] {
        let (url, server) = server(200, body, Duration::ZERO, "");
        let error = RpcClient::new(&url, Duration::from_secs(1), 1024)
            .unwrap()
            .call("read", &json!([]))
            .unwrap_err();
        assert_eq!(error.is_well_formed_refusal(), valid);
        server.join().unwrap();
    }
}

#[test]
fn http_preserves_duplicate_headers_exact_body_and_never_follows_redirects() {
    use allowit_sdk::transport::http::{HttpClient, Method, Request as HttpRequest};
    let options = Options {
        timeout: Duration::from_secs(1),
        max_response_bytes: 1024,
        no_proxy: true,
        connect_timeout: None,
    };
    let client = HttpClient::without_connection_reuse(options).unwrap();
    let (url, server) = server(
        402,
        json!({"challenge":true}),
        Duration::ZERO,
        "Payment-Required: one\r\nPayment-Required: two\r\n",
    );
    let body = br#"{"bound":"bytes"}"#;
    let response = client
        .send(
            HttpRequest {
                method: Method::Post,
                url: &url,
                headers: &[("Content-Type".into(), "application/json".into())],
                body: Some(body),
            },
            None,
        )
        .unwrap();
    assert_eq!(response.status, 402);
    assert_eq!(
        response
            .header_values("payment-required")
            .collect::<Vec<_>>(),
        vec![b"one".as_slice(), b"two".as_slice()]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&response.body).unwrap(),
        json!({"challenge":true})
    );
    assert_eq!(server.join().unwrap(), json!({"bound":"bytes"}));
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let (url, server) = self::server(
        302,
        json!({}),
        Duration::ZERO,
        &format!("Location: http://{}\r\n", target.local_addr().unwrap()),
    );
    assert_eq!(
        client
            .send_success_body(
                HttpRequest {
                    method: Method::Post,
                    url: &url,
                    headers: &[],
                    body: Some(b"{}")
                },
                None
            )
            .unwrap()
            .status,
        302
    );
    server.join().unwrap();
    assert!(target.accept().is_err());
}

#[test]
fn http_response_bounds_and_expired_deadlines_fail_before_any_retry() {
    use allowit_sdk::transport::http::{HttpClient, Method, Request as HttpRequest};
    let client = HttpClient::new(Options {
        timeout: Duration::from_secs(1),
        max_response_bytes: 10,
        no_proxy: true,
        connect_timeout: None,
    })
    .unwrap();
    let (url, server) = server(200, json!({"large":"x".repeat(32)}), Duration::ZERO, "");
    assert_eq!(
        client
            .send(
                HttpRequest {
                    method: Method::Post,
                    url: &url,
                    headers: &[],
                    body: Some(b"{}")
                },
                None
            )
            .unwrap_err()
            .kind,
        ErrorKind::Size
    );
    server.join().unwrap();
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let url = format!("http://{}", target.local_addr().unwrap());
    assert_eq!(
        client
            .send(
                HttpRequest {
                    method: Method::Post,
                    url: &url,
                    headers: &[],
                    body: Some(b"{}")
                },
                Some(Instant::now() - Duration::from_secs(1))
            )
            .unwrap_err()
            .kind,
        ErrorKind::Deadline
    );
    assert!(target.accept().is_err());
}
