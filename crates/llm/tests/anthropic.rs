//! The anthropic adapter against a local stand-in for the Messages API: every
//! test records what the adapter sent and answers with a recorded reply. No
//! network, no key.

use std::{
    collections::VecDeque,
    future::IntoFuture,
    sync::{Arc, Mutex, Once},
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use llm::{ContentBlock, Message, Provider, ProviderConfig, ToolSpec, TurnRequest};
use serde_json::{Value, json};

const KEY_ENV: &str = "HARNESS_TEST_ANTHROPIC_KEY";
const KEY: &str = "sk-test-not-a-real-key";

/// One scripted reply: the status to answer with, the body, and extra headers.
type Reply = (StatusCode, Value, Vec<(&'static str, &'static str)>);

#[derive(Clone, Default)]
struct Api {
    /// Every request body received, oldest first, with its headers.
    requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    replies: Arc<Mutex<VecDeque<Reply>>>,
}

async fn messages(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    api.requests.lock().unwrap().push((headers, body));
    let (status, body, extra) = api
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .expect("the test scripted a reply");
    let mut response = (status, Json(body)).into_response();
    for (name, value) in extra {
        response.headers_mut().insert(name, value.parse().unwrap());
    }
    response
}

/// The adapter, pointed at a local server that plays `replies` in order.
async fn adapter(replies: impl IntoIterator<Item = Reply>) -> (Box<dyn Provider>, Api) {
    static KEY_SET: Once = Once::new();
    // Tests in this binary all read the same variable, and this runs to
    // completion before any of them builds an adapter, so nothing reads the
    // environment while it is written.
    KEY_SET.call_once(|| unsafe { std::env::set_var(KEY_ENV, KEY) });

    let api = Api {
        replies: Arc::new(Mutex::new(replies.into_iter().collect())),
        ..Api::default()
    };
    let router = Router::new()
        .route("/v1/messages", post(messages))
        .with_state(api.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(axum::serve(listener, router).into_future());

    let config = ProviderConfig {
        name: "claude".into(),
        kind: "anthropic".into(),
        model: "claude-opus-5-5".into(),
        base_url: Some(url),
        api_key_env: Some(KEY_ENV.into()),
        ..ProviderConfig::default()
    };
    (llm::build(&config).unwrap(), api)
}

fn ok(body: Value) -> Reply {
    (StatusCode::OK, body, vec![])
}

/// A reply with a thinking block, a note and one tool call, as the API sends it.
fn reply_with_call(id: &str) -> Value {
    json!({
        "id": "msg_01", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
        "content": [
            {"type": "thinking", "thinking": "", "signature": "opaque-signature"},
            {"type": "text", "text": "Opening Appearance."},
            {"type": "tool_use", "id": id, "name": "computer", "input": {"do": "act", "element": "e3", "op": "click"}}
        ],
        "stop_reason": "tool_use",
        "stop_details": null,
        "usage": {"input_tokens": 100, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 0, "output_tokens": 50}
    })
}

fn request(messages: Vec<Message>) -> TurnRequest {
    TurnRequest {
        model: "claude-opus-5-5".into(),
        system: "You operate one window.".into(),
        messages,
        tools: vec![ToolSpec {
            name: "computer".into(),
            description: "Do one thing.".into(),
            schema: json!({"type": "object", "properties": {"do": {"type": "string"}}, "required": ["do"]}),
        }],
    }
}

#[tokio::test]
async fn the_request_has_the_api_shape_and_the_reply_comes_back_neutral() {
    let (provider, api) = adapter([ok(reply_with_call("toolu_1"))]).await;
    let first = vec![
        ContentBlock::text("Task: turn on dark mode"),
        ContentBlock::Image {
            media_type: "image/png".into(),
            base64: "AAAA".into(),
        },
    ];
    let response = provider
        .turn(request(vec![Message::user(first)]))
        .await
        .unwrap();

    let (headers, body) = api.requests.lock().unwrap()[0].clone();
    assert_eq!(headers["x-api-key"], KEY);
    assert_eq!(headers["anthropic-version"], "2026-10-21");
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["max_tokens"], 16000);
    assert_eq!(body["system"][0]["text"], "You operate one window.");
    assert_eq!(body["tools"][0]["name"], "computer");
    assert_eq!(body["tools"][0]["input_schema"]["required"][0], "do");
    assert_eq!(body["cache_control"]["type"], "ephemeral");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(
        body["messages"][0]["content"][0]["text"],
        "Task: turn on dark mode"
    );
    assert_eq!(body["messages"][0]["content"][1]["source"]["data"], "AAAA");
    assert!(
        body.get("thinking").is_none(),
        "the model's default thinking"
    );
    assert!(body.get("temperature").is_none());

    assert_eq!(
        response.content,
        [
            ContentBlock::text("Opening Appearance."),
            ContentBlock::ToolCall {
                id: "toolu_1".into(),
                name: "computer".into(),
                args: json!({"do": "act", "element": "e3", "op": "click"}),
            }
        ],
        "thinking blocks stay out of the neutral reply"
    );
    // 100 + 1000 cached-write prompt tokens; cost at Opus 5.5 rates:
    // 100 * $4 + 1000 * $4 * 1.25 + 50 * $20 per million.
    assert_eq!(response.usage.input_tokens, 1100);
    assert_eq!(response.usage.output_tokens, 50);
    let expected = (100.0 * 4.0 + 1000.0 * 5.0 + 50.0 * 20.0) / 1_000_000.0;
    assert!((response.usage.cost_usd - expected).abs() < 1e-12);
}

#[tokio::test]
async fn the_assistant_turn_goes_back_with_its_thinking_blocks() {
    let (provider, api) = adapter([
        ok(reply_with_call("toolu_1")),
        ok(json!({
            "content": [{"type": "text", "text": "Dark mode is on."}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })),
    ])
    .await;
    let mut messages = vec![Message::user(vec![ContentBlock::text("Task")])];
    let first = provider.turn(request(messages.clone())).await.unwrap();
    // The loop sends the neutral reply back, with the tool's result.
    messages.push(Message::assistant(first.content));
    messages.push(Message::user(vec![ContentBlock::ToolResult {
        id: "toolu_1".into(),
        content: vec![ContentBlock::text("Done. Here is the window now.")],
        is_error: false,
    }]));
    let second = provider.turn(request(messages)).await.unwrap();
    assert_eq!(second.content, [ContentBlock::text("Dark mode is on.")]);

    let (_, body) = api.requests.lock().unwrap()[1].clone();
    let assistant = &body["messages"][1];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["content"][0]["type"], "thinking");
    assert_eq!(assistant["content"][0]["signature"], "opaque-signature");
    assert_eq!(assistant["content"][2]["type"], "tool_use");
    let result = &body["messages"][2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "toolu_1");
    assert!(result.get("is_error").is_none());
}

#[tokio::test]
async fn api_errors_are_readable_and_never_show_the_key() {
    let (provider, _) = adapter([(
        StatusCode::UNAUTHORIZED,
        json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}}),
        vec![],
    )])
    .await;
    let error = provider
        .turn(request(vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("401"), "{error}");
    assert!(error.contains("authentication_error"), "{error}");
    assert!(error.contains("invalid x-api-key"), "{error}");
    assert!(!error.contains(KEY), "{error}");
}

#[tokio::test]
async fn overloaded_answers_are_retried_after_the_wait_the_api_asks_for() {
    let (provider, api) = adapter([
        (
            StatusCode::from_u16(529).unwrap(),
            json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
            vec![("retry-after", "0")],
        ),
        ok(reply_with_call("toolu_9")),
    ])
    .await;
    let response = provider.turn(request(vec![])).await.unwrap();
    assert_eq!(api.requests.lock().unwrap().len(), 2);
    assert!(matches!(&response.content[1], ContentBlock::ToolCall { id, .. } if id == "toolu_9"));
}

#[tokio::test]
async fn a_bad_request_is_not_retried() {
    let (provider, api) = adapter([(
        StatusCode::BAD_REQUEST,
        json!({"type": "error", "error": {"type": "invalid_request_error", "message": "max_tokens"}}),
        vec![],
    )])
    .await;
    assert!(provider.turn(request(vec![])).await.is_err());
    assert_eq!(api.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_refusal_ends_the_turn_with_its_reason() {
    let (provider, _) = adapter([ok(json!({
        "content": [],
        "stop_reason": "refusal",
        "stop_details": {"type": "refusal", "category": "cyber", "explanation": "declined"},
        "usage": {"input_tokens": 10, "output_tokens": 0}
    }))])
    .await;
    let error = provider
        .turn(request(vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("refused") && error.contains("cyber"),
        "{error}"
    );
}

#[tokio::test]
async fn caps_and_prices_come_from_config_not_the_name() {
    let config = ProviderConfig {
        kind: "anthropic".into(),
        model: "claude-next-year".into(),
        api_key_env: Some(KEY_ENV.into()),
        vision: Some(false),
        max_image_edge: Some(2576),
        input_usd_per_mtok: Some(1.0),
        output_usd_per_mtok: Some(2.0),
        ..ProviderConfig::default()
    };
    let provider = llm::build(&config).unwrap();
    let caps = provider.caps("claude-next-year");
    assert!(!caps.vision);
    assert!(caps.tools);
    assert!(!caps.native_computer, "the native toolset isn't wired yet");
    assert_eq!(caps.max_image_edge, 2576);

    let half = ProviderConfig {
        input_usd_per_mtok: Some(1.0),
        output_usd_per_mtok: None,
        ..config
    };
    assert!(llm::build(&half).is_err(), "one price without the other");
}

#[tokio::test]
async fn without_a_key_the_turn_says_which_variable_to_set() {
    let config = ProviderConfig {
        kind: "anthropic".into(),
        model: "claude-opus-5-5".into(),
        api_key_env: Some("HARNESS_TEST_UNSET_KEY".into()),
        ..ProviderConfig::default()
    };
    let provider = llm::build(&config).unwrap();
    let error = provider
        .turn(request(vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("HARNESS_TEST_UNSET_KEY"), "{error}");
}
