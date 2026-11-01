//! Claude, through Anthropic's Messages API (`POST /v1/messages`).
//!
//! What it does: translates the neutral request (system, tools, messages) to
//! the API's JSON and the reply back, counts tokens into a cost, retries the
//! retryable errors, and never puts the key in an error or a log.
//!
//! Two things worth knowing:
//!
//! - The reply can carry `thinking` blocks that the API wants back unchanged
//!   on the next turn, and stripping them can fail the request. The neutral
//!   `ContentBlock` has no such block, so this adapter remembers each raw reply
//!   by its first tool call's id and replays it verbatim when the loop sends
//!   that assistant turn back. The loop never knows.
//! - The tools go over as they are: the generic `computer` tool is a JSON
//!   schema the model reads. Claude's own computer toolset (where the model
//!   calls `left_click`, `screenshot`, ... directly) is not wired yet; until it
//!   is, `native_computer` defaults to false.
//!
//! Prices and the API version are from the provider's docs at the time of
//! writing (API version `2026-10-21`); set the price fields in config for a
//! model that isn't in the table.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use async_trait::async_trait;
use proto::{ContentBlock, Message, Role, Usage};
use reqwest::{StatusCode, header};
use serde_json::{Value, json};

use crate::{Adapter, Caps, Provider, ProviderConfig, TurnRequest, TurnResponse};

pub const ADAPTER: Adapter = Adapter {
    kind: "anthropic",
    build: |config| Anthropic::new(config).map(|provider| Box::new(provider) as Box<dyn Provider>),
};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2026-10-21";
const DEFAULT_KEY_ENV: &str = "ANTHROPIC_API_KEY";
/// Room for a note and a tool call many times over; a turn that hits it is an error.
const MAX_TOKENS: u32 = 16_000;
/// Retries on 429, 529 and 5xx, after the `retry-after` the API asks for.
const RETRIES: u32 = 2;
/// The longest image edge every current Claude model takes. Newer ones take
/// more: set `max_image_edge` in config to use it.
const SAFE_IMAGE_EDGE: u32 = 1568;
/// Raw replies kept for replay. A run is a few dozen turns; this is plenty.
const REMEMBERED_TURNS: usize = 512;

pub struct Anthropic {
    http: reqwest::Client,
    url: String,
    key: Option<String>,
    key_env: String,
    caps: Caps,
    prices: Option<Prices>,
    warned_no_prices: AtomicBool,
    remembered: Mutex<Remembered>,
}

/// Raw assistant replies by the id of their first tool call, oldest first.
#[derive(Default)]
struct Remembered {
    order: VecDeque<String>,
    content: HashMap<String, Vec<Value>>,
}

impl Remembered {
    fn insert(&mut self, id: String, content: Vec<Value>) {
        if self.content.insert(id.clone(), content).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > REMEMBERED_TURNS {
            if let Some(old) = self.order.pop_front() {
                self.content.remove(&old);
            }
        }
    }
}

/// US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Prices {
    input: f64,
    output: f64,
    cache_read: f64,
}

impl Prices {
    fn new(input: f64, output: f64, cache_read: f64) -> Self {
        Self {
            input,
            output,
            cache_read,
        }
    }

    /// Current Claude models, first-party API rates. Longest prefix wins. Cache
    /// reads are the published rate where there is one, else the usual 0.1x input.
    fn known(model: &str) -> Option<Self> {
        const TABLE: &[(&str, f64, f64, f64)] = &[
            ("claude-fable-5-1", 10.0, 50.0, 0.25),
            ("claude-fable-5", 10.0, 50.0, 1.0),
            ("claude-opus-5-5", 4.0, 20.0, 0.20),
            ("claude-opus-5", 5.0, 25.0, 0.50),
            ("claude-opus-4-8", 5.0, 25.0, 0.50),
            ("claude-opus-4-7", 5.0, 25.0, 0.50),
            ("claude-opus-4-6", 5.0, 25.0, 0.50),
            ("claude-sonnet-5-5", 2.0, 10.0, 0.20),
            ("claude-sonnet-5", 2.0, 10.0, 0.20),
            ("claude-sonnet-4-6", 3.0, 15.0, 0.30),
            ("claude-haiku-5-5", 0.10, 0.50, 0.01),
            ("claude-haiku-4-5", 1.0, 5.0, 0.10),
        ];
        TABLE
            .iter()
            .filter(|(prefix, ..)| model.starts_with(prefix))
            .max_by_key(|(prefix, ..)| prefix.len())
            .map(|&(_, input, output, cache_read)| Self::new(input, output, cache_read))
    }

    /// Cache writes cost 1.25x input; reads are the model's cache-read rate.
    fn cost(&self, counts: &Counts) -> f64 {
        let per = |tokens: u64, usd_per_mtok: f64| tokens as f64 * usd_per_mtok / 1_000_000.0;
        per(counts.input, self.input)
            + per(counts.cache_write, self.input * 1.25)
            + per(counts.cache_read, self.cache_read)
            + per(counts.output, self.output)
    }
}

/// The reply's `usage`, as the API counts it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    input: u64,
    cache_write: u64,
    cache_read: u64,
    output: u64,
}

impl Counts {
    fn from_usage(usage: &Value) -> Self {
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        Self {
            input: count("input_tokens"),
            cache_write: count("cache_creation_input_tokens"),
            cache_read: count("cache_read_input_tokens"),
            output: count("output_tokens"),
        }
    }

    /// The whole prompt, cached or not.
    fn prompt(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }
}

impl Anthropic {
    pub fn new(config: &ProviderConfig) -> anyhow::Result<Self> {
        let key_env = config
            .api_key_env
            .clone()
            .unwrap_or_else(|| DEFAULT_KEY_ENV.to_owned());
        let key = std::env::var(&key_env).ok().filter(|key| !key.is_empty());
        let base = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/');
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .build()
            .context("the HTTP client builds with these settings")?;
        let prices = match (config.input_usd_per_mtok, config.output_usd_per_mtok) {
            (Some(input), Some(output)) => Some(Prices::new(input, output, input * 0.1)),
            (None, None) => Prices::known(&config.model),
            _ => bail!("set both input_usd_per_mtok and output_usd_per_mtok, or neither"),
        };
        Ok(Self {
            http,
            url: format!("{base}/v1/messages"),
            key,
            key_env,
            caps: Caps {
                vision: config.vision.unwrap_or(true),
                tools: true,
                native_computer: config.native_computer.unwrap_or(false),
                max_image_edge: config.max_image_edge.unwrap_or(SAFE_IMAGE_EDGE),
            },
            prices,
            warned_no_prices: AtomicBool::new(false),
            remembered: Mutex::default(),
        })
    }

    fn body(&self, req: &TurnRequest) -> Value {
        let mut body = json!({
            "model": req.model,
            "max_tokens": MAX_TOKENS,
            "messages": req.messages.iter().map(|m| self.message(m)).collect::<Vec<_>>(),
            // The prefix (tools, system, every earlier turn) repeats each turn:
            // the API caches up to the newest cacheable block.
            "cache_control": {"type": "ephemeral"},
        });
        if !req.system.is_empty() {
            body["system"] = json!([{"type": "text", "text": req.system}]);
        }
        if !req.tools.is_empty() {
            body["tools"] = req
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.schema,
                    })
                })
                .collect();
        }
        body
    }

    fn message(&self, message: &Message) -> Value {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let content = match message.role {
            Role::Assistant => self
                .remembered_reply(&message.content)
                .unwrap_or_else(|| blocks(&message.content)),
            Role::User => blocks(&message.content),
        };
        json!({"role": role, "content": content})
    }

    /// The raw reply this assistant turn came from, thinking blocks and all.
    fn remembered_reply(&self, content: &[ContentBlock]) -> Option<Vec<Value>> {
        let id = first_call(content)?;
        self.remembered.lock().unwrap().content.get(id).cloned()
    }

    fn parse(&self, text: &str, model: &str) -> anyhow::Result<TurnResponse> {
        let reply: Value =
            serde_json::from_str(text).context("unexpected answer from the Anthropic API")?;
        match reply["stop_reason"].as_str() {
            Some("refusal") => {
                let details = &reply["stop_details"];
                let category = details["category"].as_str().unwrap_or("unspecified");
                let explanation = details["explanation"].as_str().unwrap_or("");
                bail!("the model refused this request ({category}): {explanation}")
            }
            Some("max_tokens") => bail!("the reply ran past {MAX_TOKENS} tokens and was cut off"),
            _ => {}
        }
        let raw = reply["content"]
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("the reply has no content"))?;
        let content: Vec<ContentBlock> = raw.iter().filter_map(block_from_api).collect();
        if let Some(id) = first_call(&content) {
            self.remembered
                .lock()
                .unwrap()
                .insert(id.to_owned(), raw.clone());
        }
        let counts = Counts::from_usage(&reply["usage"]);
        let cost_usd = match &self.prices {
            Some(prices) => prices.cost(&counts),
            None => {
                if !self.warned_no_prices.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        model,
                        "no prices known for this model: cost stays 0. Set \
                         input_usd_per_mtok and output_usd_per_mtok in its config."
                    );
                }
                0.0
            }
        };
        Ok(TurnResponse {
            content,
            usage: Usage {
                input_tokens: counts.prompt().try_into().unwrap_or(u32::MAX),
                output_tokens: counts.output.try_into().unwrap_or(u32::MAX),
                cost_usd,
            },
        })
    }
}

#[async_trait]
impl Provider for Anthropic {
    fn caps(&self, _model: &str) -> Caps {
        self.caps
    }

    async fn turn(&self, req: TurnRequest) -> anyhow::Result<TurnResponse> {
        let key = self.key.as_deref().ok_or_else(|| {
            anyhow!(
                "no API key for the anthropic provider: put it in the {} environment variable",
                self.key_env
            )
        })?;
        let body = self.body(&req);
        let mut attempt = 0;
        loop {
            let response = self
                .http
                .post(&self.url)
                .header("x-api-key", key)
                .header("anthropic-version", API_VERSION)
                .json(&body)
                .send()
                .await
                .context("can't reach the Anthropic API")?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok());
            let text = response
                .text()
                .await
                .context("the Anthropic API's answer broke off")?;
            if status.is_success() {
                return self.parse(&text, &req.model);
            }
            let (kind, message) = error_parts(&text);
            let retryable = status == StatusCode::TOO_MANY_REQUESTS
                || status == 529
                || status.is_server_error();
            if retryable && attempt < RETRIES {
                attempt += 1;
                let wait = retry_after.unwrap_or(attempt as u64);
                tracing::warn!(%status, kind, attempt, wait, "the Anthropic API didn't take the turn; retrying");
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }
            bail!("the Anthropic API answered {status} ({kind}): {message}");
        }
    }
}

/// The id of the turn's first tool call, which names the raw reply it came from.
fn first_call(content: &[ContentBlock]) -> Option<&str> {
    content.iter().find_map(|block| match block {
        ContentBlock::ToolCall { id, .. } => Some(id.as_str()),
        _ => None,
    })
}

fn blocks(content: &[ContentBlock]) -> Vec<Value> {
    content.iter().map(block_to_api).collect()
}

fn block_to_api(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({"type": "text", "text": text}),
        ContentBlock::Image { media_type, base64 } => json!({
            "type": "image",
            "source": {"type": "base64", "media_type": media_type, "data": base64},
        }),
        ContentBlock::ToolCall { id, name, args } => {
            json!({"type": "tool_use", "id": id, "name": name, "input": args})
        }
        ContentBlock::ToolResult {
            id,
            content,
            is_error,
        } => {
            let mut result = json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": blocks(content),
            });
            if *is_error {
                result["is_error"] = json!(true);
            }
            result
        }
    }
}

/// Text and tool calls. Thinking blocks are kept raw for replay, not shown.
fn block_from_api(block: &Value) -> Option<ContentBlock> {
    match block["type"].as_str()? {
        "text" => Some(ContentBlock::text(block["text"].as_str()?)),
        "tool_use" => Some(ContentBlock::ToolCall {
            id: block["id"].as_str()?.to_owned(),
            name: block["name"].as_str()?.to_owned(),
            args: block["input"].clone(),
        }),
        _ => None,
    }
}

/// The `type` and `message` of an error body, or the body itself.
fn error_parts(text: &str) -> (String, String) {
    let parsed: Option<Value> = serde_json::from_str(text).ok();
    let error = parsed.as_ref().map(|value| &value["error"]);
    let kind = error
        .and_then(|error| error["type"].as_str())
        .unwrap_or("error")
        .to_owned();
    let message = error
        .and_then(|error| error["message"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| text.chars().take(200).collect());
    (kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_pick_the_longest_matching_model() {
        assert_eq!(
            Prices::known("claude-opus-5-5").unwrap(),
            Prices::new(4.0, 20.0, 0.20)
        );
        assert_eq!(
            Prices::known("claude-opus-5").unwrap(),
            Prices::new(5.0, 25.0, 0.50)
        );
        assert_eq!(Prices::known("someone-elses-model"), None);
    }

    #[test]
    fn cost_counts_cache_writes_and_reads_differently() {
        let prices = Prices::new(4.0, 20.0, 0.20);
        let counts = Counts {
            input: 1_000_000,
            cache_write: 1_000_000,
            cache_read: 1_000_000,
            output: 1_000_000,
        };
        assert!((prices.cost(&counts) - (4.0 + 5.0 + 0.20 + 20.0)).abs() < 1e-9);
        assert_eq!(counts.prompt(), 3_000_000);
    }

    #[test]
    fn blocks_map_both_ways() {
        let call = ContentBlock::ToolCall {
            id: "toolu_1".into(),
            name: "computer".into(),
            args: json!({"do": "observe"}),
        };
        let api = block_to_api(&call);
        assert_eq!(api["type"], "tool_use");
        assert_eq!(api["input"]["do"], "observe");
        assert_eq!(block_from_api(&api).unwrap(), call);

        let result = ContentBlock::ToolResult {
            id: "toolu_1".into(),
            content: vec![
                ContentBlock::text("Done."),
                ContentBlock::Image {
                    media_type: "image/png".into(),
                    base64: "AAAA".into(),
                },
            ],
            is_error: true,
        };
        let api = block_to_api(&result);
        assert_eq!(api["tool_use_id"], "toolu_1");
        assert_eq!(api["is_error"], true);
        assert_eq!(api["content"][1]["source"]["media_type"], "image/png");

        let thinking = json!({"type": "thinking", "thinking": "", "signature": "sig"});
        assert_eq!(block_from_api(&thinking), None);
    }

    #[test]
    fn error_bodies_read_well() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert_eq!(
            error_parts(body),
            ("authentication_error".into(), "invalid x-api-key".into())
        );
        assert_eq!(error_parts("<html>"), ("error".into(), "<html>".into()));
    }

    #[test]
    fn remembered_replies_are_bounded() {
        let mut remembered = Remembered::default();
        for n in 0..(REMEMBERED_TURNS + 10) {
            remembered.insert(format!("toolu_{n}"), vec![]);
        }
        assert_eq!(remembered.content.len(), REMEMBERED_TURNS);
        assert!(!remembered.content.contains_key("toolu_0"));
        assert!(remembered.content.contains_key("toolu_10"));
    }
}
