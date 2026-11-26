//! Local HTTP tests: verify request shape, auth headers and response parsing
//! without any external provider.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;

use mappamente_ai::{ChatMessage, ChatRequest, Protocol, ProviderClient, ProviderSettings};
use serde_json::{Value, json};

struct RequestCapture {
    request_line: String,
    headers: String,
    body: Value,
}

fn mock_server(
    response_status: &str,
    response_body: Value,
) -> (String, thread::JoinHandle<RequestCapture>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = response_status.to_owned();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());

        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let request_line = request_line.trim().to_owned();

        let mut headers = String::new();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = value.trim().parse().unwrap();
            }
            headers.push_str(&line.to_ascii_lowercase());
        }

        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        let response = response_body.to_string();
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
        stream.flush().unwrap();

        RequestCapture {
            request_line,
            headers,
            body,
        }
    });
    (format!("http://{address}"), handle)
}

fn settings(base_url: String, protocol: Protocol) -> ProviderSettings {
    ProviderSettings {
        id: "test".to_owned(),
        name: "Test provider".to_owned(),
        protocol,
        base_url,
        model: "test-model".to_owned(),
        api_key: Some("test-key".to_owned()),
        session_id: None,
        key_env: "TEST_API_KEY".to_owned(),
        needs_key: true,
    }
}

fn request(settings: ProviderSettings) -> ChatRequest {
    ChatRequest {
        settings,
        system: "system prompt".to_owned(),
        messages: vec![ChatMessage::user("hello")],
    }
}

#[test]
fn openai_compatible_request_and_response() {
    let (base, server) = mock_server(
        "200 OK",
        json!({"choices":[{"message":{"content":"{\"reply\":\"ok\",\"actions\":[]}"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}),
    );
    let client = ProviderClient::new().unwrap();
    let response = client
        .send(&request(settings(
            base.clone() + "/v1",
            Protocol::OpenAiCompatible,
        )))
        .unwrap();

    assert_eq!(response.text, "{\"reply\":\"ok\",\"actions\":[]}");
    assert_eq!(response.usage.input, Some(5));
    assert_eq!(response.usage.output, Some(2));

    let capture = server.join().unwrap();
    assert!(
        capture.request_line.contains("/v1/chat/completions"),
        "{}",
        capture.request_line
    );
    assert!(capture.headers.contains("authorization: bearer test-key"));
    assert_eq!(capture.body["model"], "test-model");
    assert_eq!(capture.body["messages"][0]["role"], "system");
    assert_eq!(capture.body["messages"][1]["content"], "hello");
}

#[test]
fn anthropic_request_and_response() {
    let (base, server) = mock_server(
        "200 OK",
        json!({"content":[{"type":"text","text":"hello"}],"usage":{"input_tokens":4,"output_tokens":1}}),
    );
    let client = ProviderClient::new().unwrap();
    let response = client
        .send(&request(settings(base, Protocol::Anthropic)))
        .unwrap();

    assert_eq!(response.text, "hello");
    assert_eq!(response.usage.input, Some(4));

    let capture = server.join().unwrap();
    assert!(
        capture.request_line.contains("/v1/messages"),
        "{}",
        capture.request_line
    );
    assert!(capture.headers.contains("x-api-key: test-key"));
    assert!(capture.headers.contains("anthropic-version: 2023-06-01"));
    assert_eq!(capture.body["system"], "system prompt");
    assert_eq!(capture.body["messages"][0]["role"], "user");
    assert!(capture.body["max_tokens"].as_u64().is_some());
}

#[test]
fn http_errors_are_reported_with_the_provider_message() {
    let (base, server) = mock_server("401 Unauthorized", json!({"error":{"message":"bad key"}}));
    let client = ProviderClient::new().unwrap();
    let error = client
        .send(&request(settings(base + "/v1", Protocol::OpenAiCompatible)))
        .unwrap_err();
    assert!(error.to_string().contains("401"));
    assert!(error.to_string().contains("bad key"));
    let capture = server.join().unwrap();
    assert!(capture.headers.contains("authorization: bearer test-key"));
}

#[test]
fn opencode_go_request_sends_the_session_header() {
    let (base, server) = mock_server("200 OK", json!({"choices":[{"message":{"content":"ok"}}]}));
    let client = ProviderClient::new().unwrap();
    let mut provider = settings(base + "/v1", Protocol::OpenAiCompatible);
    provider.id = "opencode-go".to_owned();
    provider.session_id = Some("ses_test123".to_owned());
    assert_eq!(client.send(&request(provider)).unwrap().text, "ok");

    let capture = server.join().unwrap();
    assert!(capture.headers.contains("x-opencode-session: ses_test123"));
    assert!(capture.headers.contains("authorization: bearer test-key"));
}

#[test]
fn console_payment_errors_point_go_subscribers_to_the_go_provider() {
    let (base, server) = mock_server(
        "402 Payment Required",
        json!({"error":{"message":"Insufficient account funds"}}),
    );
    let client = ProviderClient::new().unwrap();
    let mut provider = settings(base + "/v1", Protocol::OpenAiCompatible);
    provider.id = "opencode".to_owned();
    let error = client.send(&request(provider)).unwrap_err().to_string();
    assert!(error.contains("Insufficient account funds"));
    assert!(error.contains("OpenCode Go"));
    server.join().unwrap();
}

#[test]
fn opencode_local_request_and_response() {
    let (base, server) = mock_server("200 OK", json!({"data":{"text":"hello"}}));
    let client = ProviderClient::new().unwrap();
    let mut provider = settings(base, Protocol::OpenCodeServer);
    provider.needs_key = false;
    provider.model = "opencode/gpt-5".to_owned();
    provider.api_key = Some("password".to_owned());
    let response = client.send(&request(provider)).unwrap();
    assert_eq!(response.text, "hello");

    let capture = server.join().unwrap();
    assert!(
        capture
            .request_line
            .contains("/api/experimental/generate/text"),
        "{}",
        capture.request_line
    );
    assert!(capture.headers.contains("authorization: basic "));
    assert_eq!(capture.body["model"]["providerID"], "opencode");
    assert_eq!(capture.body["model"]["id"], "gpt-5");
    assert!(
        capture.body["prompt"]
            .as_str()
            .unwrap()
            .contains("User: hello")
    );
}
