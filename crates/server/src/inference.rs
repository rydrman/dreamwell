use std::error::Error;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Duration;

use dreamwell_types::{JsonFormatStrategy, ModelCapabilities, ModelInfo};
use futures_util::Stream;
use futures_util::StreamExt as FuturesStreamExt;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use tokio_stream::StreamExt as TokioStreamExt;

use crate::error::{AppError, AppResult};

/// Resolved inference endpoint credentials for outbound API calls.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    pub base_url: String,
    api_key: Option<String>,
    pub connection_id: Option<i64>,
    pub json_format_strategy: JsonFormatStrategy,
    pub tool_call_parser: String,
}

impl InferenceConfig {
    pub fn with_connection(
        base_url: String,
        api_key: Option<String>,
        connection_id: Option<i64>,
        json_format_strategy: JsonFormatStrategy,
        tool_call_parser: String,
    ) -> Self {
        let api_key = api_key.filter(|key| !key.is_empty());
        Self {
            base_url,
            api_key,
            connection_id,
            json_format_strategy,
            tool_call_parser,
        }
    }

    pub fn url(&self) -> &str {
        &self.base_url
    }

    fn with_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        }
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(600);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(900);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

fn http_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("http client")
    })
}

fn probe_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(PROBE_TIMEOUT)
            .build()
            .expect("probe http client")
    })
}

fn format_inference_http_error(status: StatusCode, body: &str) -> String {
    let trimmed = body.trim();
    if let Ok(json) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = json.pointer("/error/message").and_then(|v| v.as_str()) {
            let code = json.pointer("/error/code").and_then(|v| v.as_str());
            let base = match code.filter(|code| !code.is_empty()) {
                Some(code) => {
                    format!("Inference server returned HTTP {status} ({code}): {message}")
                }
                None => format!("Inference server returned HTTP {status}: {message}"),
            };
            return match openrouter_error_detail(&json, message) {
                Some(detail) => format!("{base} — {detail}"),
                None => base,
            };
        }
        if let Some(message) = json.get("error").and_then(|v| v.as_str()) {
            return format!("Inference server returned HTTP {status}: {message}");
        }
        if let Some(message) = json.get("detail").and_then(|v| v.as_str()) {
            return format!("Inference server returned HTTP {status}: {message}");
        }
        if let Some(message) = json.get("message").and_then(|v| v.as_str()) {
            return format!("Inference server returned HTTP {status}: {message}");
        }
    }
    if trimmed.is_empty() {
        format!("Inference server returned HTTP {status} (empty response body)")
    } else {
        format!("Inference server returned HTTP {status}: {trimmed}")
    }
}

/// Extracts upstream provider detail from an OpenRouter-style error body
/// (`error.metadata.provider_name` + `error.metadata.raw`). The `raw` field is
/// itself a JSON-encoded upstream error; its `message` is preferred when present.
/// Returns None when metadata is absent or adds nothing beyond `message`.
fn openrouter_error_detail(json: &Value, message: &str) -> Option<String> {
    const RAW_LIMIT: usize = 500;
    let metadata = json.pointer("/error/metadata")?;
    let provider = metadata
        .get("provider_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let raw = metadata
        .get("raw")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let upstream = raw.map(|raw| {
        if let Ok(upstream_json) = serde_json::from_str::<Value>(raw) {
            if let Some(m) = upstream_json.get("message").and_then(|v| v.as_str()) {
                return m.to_string();
            }
        }
        if raw.len() > RAW_LIMIT {
            let cut = raw
                .char_indices()
                .take_while(|(i, _)| *i < RAW_LIMIT)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            format!("{}…", &raw[..cut])
        } else {
            raw.to_string()
        }
    });
    match (provider, upstream) {
        (Some(provider), Some(detail)) if detail != message => {
            Some(format!("{provider}: {detail}"))
        }
        (Some(provider), _) => Some(provider.to_string()),
        (None, Some(detail)) if detail != message => Some(detail),
        _ => None,
    }
}

fn format_reqwest_inference_error(err: &reqwest::Error, url: &str) -> String {
    let mut parts = vec![format!("Inference request failed for {url}")];
    if let Some(status) = err.status() {
        parts.push(format!("HTTP {status}"));
    }
    if err.is_connect() {
        parts.push(
            "connection error (check inference URL, DNS, TLS certificate, and network)".to_string(),
        );
    } else if err.is_timeout() {
        parts.push("request timed out".to_string());
    }
    let base = err.to_string();
    if !base.is_empty() {
        parts.push(base);
    }
    if let Some(source) = err.source() {
        parts.push(source.to_string());
    }
    parts.join(": ")
}

fn inference_reqwest(err: reqwest::Error, url: &str) -> AppError {
    AppError::inference(format_reqwest_inference_error(&err, url))
}

fn inference_http_error(status: StatusCode, body: &str) -> AppError {
    AppError::inference(format_inference_http_error(status, body))
}

fn inference_stream_error(data: &str) -> AppError {
    if data.trim().is_empty() {
        return AppError::inference("Inference stream returned an empty error event".to_string());
    }
    if let Ok(json) = serde_json::from_str::<Value>(data) {
        let code = json
            .get("code")
            .or_else(|| json.pointer("/error/code"))
            .and_then(|v| v.as_i64())
            .map(|code| StatusCode::from_u16(code as u16).unwrap_or(StatusCode::BAD_REQUEST))
            .unwrap_or(StatusCode::BAD_REQUEST);
        AppError::inference(format_inference_http_error(code, data))
    } else {
        AppError::inference(format!("Inference stream error: {data}"))
    }
}

fn streaming_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("streaming http client")
    })
}

/// Strip a trailing `/v1` from an OpenAI-compatible base URL to reach the server root.
pub fn inference_server_root(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
}

/// Read a positive context length from common model-list object fields.
pub fn context_from_model_object(obj: &serde_json::Map<String, Value>) -> Option<i64> {
    for key in [
        "context_length",
        "max_model_len",
        "max_context_length",
        "n_ctx",
    ] {
        if let Some(n) = obj.get(key).and_then(|v| v.as_i64()).filter(|n| *n > 0) {
            return Some(n);
        }
    }
    None
}

/// Parse Ollama `/api/show` `model_info` for `*.context_length`.
pub fn context_from_ollama_model_info(model_info: &Value) -> Option<i64> {
    let obj = model_info.as_object()?;
    let mut best: Option<i64> = None;
    for (key, value) in obj {
        if key.ends_with(".context_length") {
            if let Some(n) = value.as_i64().filter(|n| *n > 0) {
                best = Some(best.map(|b| b.max(n)).unwrap_or(n));
            }
        }
    }
    best
}

/// Parse llama.cpp `/props` JSON for `n_ctx`.
pub fn context_from_llama_props(data: &Value) -> Option<i64> {
    data.get("default_generation_settings")
        .and_then(|s| s.get("n_ctx"))
        .or_else(|| data.get("n_ctx"))
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
}

pub async fn list_models(config: &InferenceConfig) -> AppResult<Vec<ModelInfo>> {
    let url = format!("{}/models", config.url().trim_end_matches('/'));
    let response = config
        .with_auth(http_client().get(&url))
        .send()
        .await
        .map_err(|err| inference_reqwest(err, &url))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(inference_http_error(status, &body));
    }
    let data: Value = response.json().await?;
    let models_value = data.get("data").cloned().unwrap_or_else(|| data.clone());
    let mut result = Vec::new();
    if let Some(arr) = models_value.as_array() {
        for item in arr {
            if let Some(s) = item.as_str() {
                result.push(ModelInfo {
                    id: s.to_string(),
                    name: Some(s.to_string()),
                    context_length: None,
                    context_source: None,
                });
            } else if let Some(obj) = item.as_object() {
                let id = obj
                    .get("id")
                    .or_else(|| obj.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let name = obj.get("name").and_then(|v| v.as_str()).map(str::to_string);
                let context_length = context_from_model_object(obj);
                let context_source = context_length.map(|_| "models_list".to_string());
                if !id.is_empty() {
                    result.push(ModelInfo {
                        id,
                        name,
                        context_length,
                        context_source,
                    });
                }
            }
        }
    }
    Ok(result)
}

pub async fn probe_model_capabilities(config: &InferenceConfig, model: &str) -> ModelCapabilities {
    if model.trim().is_empty() {
        return ModelCapabilities {
            model: model.to_string(),
            context_length: None,
            context_source: None,
        };
    }

    if let Some((length, source)) = probe_ollama_show(config, model).await {
        return ModelCapabilities {
            model: model.to_string(),
            context_length: Some(length),
            context_source: Some(source),
        };
    }

    if let Some((length, source)) = probe_llama_props(config, model).await {
        return ModelCapabilities {
            model: model.to_string(),
            context_length: Some(length),
            context_source: Some(source),
        };
    }

    ModelCapabilities {
        model: model.to_string(),
        context_length: None,
        context_source: None,
    }
}

async fn probe_ollama_show(config: &InferenceConfig, model: &str) -> Option<(i64, String)> {
    let root = inference_server_root(config.url());
    let url = format!("{root}/api/show");
    let response = config
        .with_auth(probe_client().post(&url))
        .json(&serde_json::json!({ "model": model }))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let data: Value = response.json().await.ok()?;
    context_from_ollama_model_info(&data["model_info"]).map(|n| (n, "ollama_show".to_string()))
}

async fn probe_llama_props(config: &InferenceConfig, model: &str) -> Option<(i64, String)> {
    let root = inference_server_root(config.url());
    let response = config
        .with_auth(probe_client().get(format!("{root}/props")))
        .query(&[("model", model)])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        // Some backends expose /props without a model query when only one model is loaded.
        let fallback_url = format!("{root}/props");
        let response = config
            .with_auth(probe_client().get(&fallback_url))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let data: Value = response.json().await.ok()?;
        return context_from_llama_props(&data).map(|n| (n, "llama_props".to_string()));
    }
    let data: Value = response.json().await.ok()?;
    context_from_llama_props(&data).map(|n| (n, "llama_props".to_string()))
}

pub async fn stream_chat_completion(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
) -> AppResult<Pin<Box<dyn Stream<Item = AppResult<String>> + Send>>> {
    let url = format!("{}/chat/completions", config.url().trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": true,
    });
    let response = config
        .with_auth(streaming_client().post(&url))
        .json(&payload)
        .send()
        .await
        .map_err(|err| inference_reqwest(err, &url))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(inference_http_error(status, &body));
    }

    let stream_url = url.clone();
    let stream = TokioStreamExt::timeout(
        FuturesStreamExt::map(response.bytes_stream(), move |chunk| {
            chunk.map_err(|err| inference_reqwest(err, &stream_url))
        }),
        STREAM_IDLE_TIMEOUT,
    );
    let stream = FuturesStreamExt::map(stream, |result| match result {
        Ok(item) => item,
        Err(_elapsed) => Err(AppError::inference(format!(
            "Inference stream stalled (no data for {}s)",
            STREAM_IDLE_TIMEOUT.as_secs()
        ))),
    });

    let stream = FuturesStreamExt::map(stream, |chunk| {
        let chunk = chunk?;
        let text = String::from_utf8_lossy(&chunk);
        let mut tokens = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("error: ") {
                let data = line.trim_start_matches("error: ").trim();
                return Err(inference_stream_error(data));
            }
            if !line.starts_with("data: ") {
                continue;
            }
            let data = line.trim_start_matches("data: ").trim();
            if data == "[DONE]" {
                break;
            }
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                if let Some(token) = json["choices"][0]["delta"]["content"].as_str() {
                    if !token.is_empty() {
                        tokens.push(token.to_string());
                    }
                }
            }
        }
        Ok(tokens)
    });

    Ok(Box::pin(FuturesStreamExt::flat_map(stream, |result| {
        futures_util::stream::iter(match result {
            Ok(tokens) => tokens.into_iter().map(Ok).collect::<Vec<_>>(),
            Err(err) => vec![Err(err)],
        })
    })))
}

#[derive(Debug, Clone)]
pub enum ToolStreamChunk {
    Content(String),
    Done {
        native_tool_calls: Vec<ToolCall>,
        #[allow(dead_code)]
        finish_reason: Option<String>,
    },
}

#[derive(Default)]
struct NativeToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat_completion_with_tools(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    tool_choice: &serde_json::Value,
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
) -> AppResult<Pin<Box<dyn Stream<Item = AppResult<ToolStreamChunk>> + Send>>> {
    let url = format!("{}/chat/completions", config.url().trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "tool_choice": tool_choice,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": true,
    });
    let response = config
        .with_auth(streaming_client().post(&url))
        .json(&payload)
        .send()
        .await
        .map_err(|err| inference_reqwest(err, &url))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(inference_http_error(status, &body));
    }

    let byte_stream_url = url.clone();
    let byte_stream = TokioStreamExt::timeout(
        FuturesStreamExt::map(response.bytes_stream(), move |chunk| {
            chunk.map_err(|err| inference_reqwest(err, &byte_stream_url))
        }),
        STREAM_IDLE_TIMEOUT,
    );
    let byte_stream = FuturesStreamExt::map(byte_stream, |result| match result {
        Ok(item) => item,
        Err(_elapsed) => Err(AppError::inference(format!(
            "Inference stream stalled (no data for {}s)",
            STREAM_IDLE_TIMEOUT.as_secs()
        ))),
    });

    let stream = async_stream::try_stream! {
        let mut native_calls: std::collections::BTreeMap<usize, NativeToolCallBuilder> =
            std::collections::BTreeMap::new();
        let mut finish_reason = None;
        let mut byte_stream = std::pin::pin!(byte_stream);
        while let Some(chunk_result) = FuturesStreamExt::next(&mut byte_stream).await {
            let chunk: bytes::Bytes = chunk_result?;
            let text = String::from_utf8_lossy(&chunk);
            let mut saw_done = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with("error: ") {
                    let data = line.trim_start_matches("error: ").trim();
                    Err(inference_stream_error(data))?;
                }
                if !line.starts_with("data: ") {
                    continue;
                }
                let data = line.trim_start_matches("data: ").trim();
                if data == "[DONE]" {
                    saw_done = true;
                    break;
                }
                let Ok(json) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                if let Some(reason) = json["choices"][0]["finish_reason"].as_str() {
                    finish_reason = Some(reason.to_string());
                }
                let delta = &json["choices"][0]["delta"];
                if let Some(token) = delta["content"].as_str() {
                    if !token.is_empty() {
                        yield ToolStreamChunk::Content(token.to_string());
                    }
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for call in calls {
                        let index = call["index"].as_u64().unwrap_or(0) as usize;
                        let entry = native_calls.entry(index).or_default();
                        if let Some(id) = call["id"].as_str() {
                            entry.id = Some(id.to_string());
                        }
                        if let Some(name) = call["function"]["name"].as_str() {
                            entry.name = Some(name.to_string());
                        }
                        if let Some(args) = call["function"]["arguments"].as_str() {
                            entry.arguments.push_str(args);
                        }
                    }
                }
            }
            if saw_done {
                break;
            }
        }
        let native_tool_calls = native_calls
            .into_iter()
            .filter_map(|(index, builder)| {
                let name = builder.name?;
                if name.is_empty() {
                    return None;
                }
                Some(ToolCall {
                    id: builder.id.unwrap_or_else(|| format!("call_{index}")),
                    name,
                    arguments: if builder.arguments.is_empty() {
                        "{}".to_string()
                    } else {
                        builder.arguments
                    },
                })
            })
            .collect();
        yield ToolStreamChunk::Done {
            native_tool_calls,
            finish_reason,
        };
    };

    Ok(Box::pin(stream))
}

pub async fn chat_completion(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
) -> AppResult<String> {
    chat_completion_with_format(
        config,
        model,
        messages,
        temperature,
        top_p,
        max_tokens,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn chat_completion_with_format(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    response_format: Option<&Value>,
    guided_json: Option<&Value>,
) -> AppResult<String> {
    let url = format!("{}/chat/completions", config.url().trim_end_matches('/'));
    let mut payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": false,
    });
    if let Some(format) = response_format {
        payload["response_format"] = format.clone();
    }
    if let Some(schema) = guided_json {
        payload["guided_json"] = schema.clone();
    }
    let response = config
        .with_auth(http_client().post(&url))
        .json(&payload)
        .send()
        .await
        .map_err(|err| inference_reqwest(err, &url))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(inference_http_error(status, &body));
    }
    let data: Value = response.json().await?;
    Ok(data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string())
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Providers (e.g. Mistral) reject an assistant message whose `tool_calls`
/// carry duplicate or empty ids. Text-parsed calls reuse `call_fallback_{index}`
/// per parsed section and streamed calls may omit ids, so normalize a pending
/// batch before it is echoed back as an assistant message.
pub fn ensure_unique_tool_call_ids(calls: &mut [ToolCall]) {
    let mut seen = std::collections::HashSet::new();
    for call in calls.iter_mut() {
        if call.id.is_empty() || seen.contains(&call.id) {
            let mut n = 0;
            loop {
                let candidate = format!("call_fix_{n}");
                if !seen.contains(&candidate) {
                    call.id = candidate;
                    break;
                }
                n += 1;
            }
        }
        seen.insert(call.id.clone());
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ChatCompletionWithToolsResult {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolLoopConfig {
    pub max_iterations: u32,
    pub max_tokens_per_call: i64,
}

impl Default for ToolLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 8,
            max_tokens_per_call: 4096,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn chat_completion_with_tools(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    tool_choice: &serde_json::Value,
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
) -> AppResult<ChatCompletionWithToolsResult> {
    let url = format!("{}/chat/completions", config.url().trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "tool_choice": tool_choice,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": false,
    });
    let response = config
        .with_auth(http_client().post(&url))
        .json(&payload)
        .send()
        .await
        .map_err(|err| inference_reqwest(err, &url))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(inference_http_error(status, &body));
    }
    let data: Value = response.json().await?;
    let choice = &data["choices"][0];
    let message = &choice["message"];
    let finish_reason = choice["finish_reason"].as_str().map(str::to_string);
    let content = message["content"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut tool_calls = Vec::new();
    if let Some(calls) = message["tool_calls"].as_array() {
        for call in calls {
            let id = call["id"].as_str().unwrap_or("call").to_string();
            let name = call["function"]["name"].as_str().unwrap_or("").to_string();
            let arguments = call["function"]["arguments"]
                .as_str()
                .unwrap_or("{}")
                .to_string();
            if !name.is_empty() {
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }
    }
    Ok(ChatCompletionWithToolsResult {
        content,
        tool_calls,
        finish_reason,
    })
}

#[allow(clippy::too_many_arguments, dead_code)]
pub async fn run_tool_loop<F, Fut>(
    config: &InferenceConfig,
    model: &str,
    mut messages: Vec<serde_json::Value>,
    tools: &[serde_json::Value],
    loop_config: &ToolLoopConfig,
    temperature: f64,
    top_p: f64,
    mut on_tool_call: F,
) -> AppResult<(Vec<serde_json::Value>, u32, u32)>
where
    F: FnMut(&ToolCall) -> Fut,
    Fut: std::future::Future<Output = AppResult<serde_json::Value>>,
{
    let mut iterations = 0u32;
    let mut tool_call_count = 0u32;
    for _ in 0..loop_config.max_iterations {
        let result = chat_completion_with_tools(
            config,
            model,
            &messages,
            tools,
            &serde_json::json!("auto"),
            temperature,
            top_p,
            loop_config.max_tokens_per_call,
        )
        .await?;
        iterations += 1;

        if result.tool_calls.is_empty() {
            if let Some(content) = result.content {
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": content
                }));
            }
            break;
        }

        let assistant_tool_calls: Vec<serde_json::Value> = result
            .tool_calls
            .iter()
            .map(|tc| {
                serde_json::json!({
                    "id": tc.id,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        "arguments": tc.arguments
                    }
                })
            })
            .collect();
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": result.content,
            "tool_calls": assistant_tool_calls
        }));

        for tc in &result.tool_calls {
            tool_call_count += 1;
            let tool_result = on_tool_call(tc).await?;
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": serde_json::to_string(&tool_result).unwrap_or_else(|_| "{}".to_string())
            }));
        }

        if result.finish_reason.as_deref() == Some("stop") {
            break;
        }
    }
    Ok((messages, iterations, tool_call_count))
}

fn json_schema_response_format(schema: &Value) -> Value {
    serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": "response",
            "strict": true,
            "schema": schema
        }
    })
}

fn json_object_response_format() -> Value {
    serde_json::json!({ "type": "json_object" })
}

/// Concrete wire format for schema-constrained JSON (excludes [`JsonFormatStrategy::Auto`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireJsonFormatStrategy {
    ResponseJsonSchema,
    GuidedJson,
    JsonObject,
}

impl From<WireJsonFormatStrategy> for JsonFormatStrategy {
    fn from(value: WireJsonFormatStrategy) -> Self {
        match value {
            WireJsonFormatStrategy::ResponseJsonSchema => Self::ResponseJsonSchema,
            WireJsonFormatStrategy::GuidedJson => Self::GuidedJson,
            WireJsonFormatStrategy::JsonObject => Self::JsonObject,
        }
    }
}

fn initial_wire_strategy(preference: JsonFormatStrategy) -> (WireJsonFormatStrategy, bool) {
    match preference {
        JsonFormatStrategy::Auto => (WireJsonFormatStrategy::ResponseJsonSchema, true),
        JsonFormatStrategy::ResponseJsonSchema => {
            (WireJsonFormatStrategy::ResponseJsonSchema, false)
        }
        JsonFormatStrategy::GuidedJson => (WireJsonFormatStrategy::GuidedJson, false),
        JsonFormatStrategy::JsonObject => (WireJsonFormatStrategy::JsonObject, false),
    }
}

fn advance_wire_strategy(
    current: WireJsonFormatStrategy,
    auto_detect: bool,
) -> Option<WireJsonFormatStrategy> {
    if !auto_detect {
        return None;
    }
    match current {
        WireJsonFormatStrategy::ResponseJsonSchema => Some(WireJsonFormatStrategy::GuidedJson),
        WireJsonFormatStrategy::GuidedJson => Some(WireJsonFormatStrategy::JsonObject),
        WireJsonFormatStrategy::JsonObject => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn chat_completion_with_json_format_fallback(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    schema: Option<&Value>,
    strategy: &mut WireJsonFormatStrategy,
    auto_detect: bool,
) -> AppResult<String> {
    loop {
        let result = match *strategy {
            WireJsonFormatStrategy::ResponseJsonSchema => {
                let response_format = schema.map(json_schema_response_format);
                chat_completion_with_format(
                    config,
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_tokens,
                    response_format.as_ref(),
                    None,
                )
                .await
            }
            WireJsonFormatStrategy::GuidedJson => {
                chat_completion_with_format(
                    config,
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_tokens,
                    None,
                    schema,
                )
                .await
            }
            WireJsonFormatStrategy::JsonObject => {
                chat_completion_with_format(
                    config,
                    model,
                    messages,
                    temperature,
                    top_p,
                    max_tokens,
                    Some(&json_object_response_format()),
                    None,
                )
                .await
            }
        };

        match result {
            Ok(raw) => return Ok(raw),
            Err(AppError::Inference(body)) if schema.is_some() => {
                if let Some(next) = advance_wire_strategy(*strategy, auto_detect) {
                    *strategy = next;
                } else {
                    return Err(AppError::Inference(body));
                }
            }
            Err(err) => return Err(err),
        }
    }
}

/// Schema-validated JSON completion with repair retries on parse failure.
///
/// When `learned_strategy` is set and the connection preference is [`JsonFormatStrategy::Auto`],
/// the caller should persist the learned value for that connection.
///
/// `repair_hint` is appended to repair-turn user messages so the model sees the expected
/// field names after a parse failure (useful when the API schema constraint is weak).
#[allow(clippy::too_many_arguments)]
pub async fn chat_completion_json<T>(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    response_format: Option<&Value>,
    max_attempts: u32,
    token: &tokio_util::sync::CancellationToken,
    learned_strategy: &mut Option<JsonFormatStrategy>,
    repair_hint: Option<&str>,
) -> AppResult<T>
where
    T: serde::de::DeserializeOwned,
{
    let schema = response_format;
    let (mut strategy, auto_detect) = initial_wire_strategy(config.json_format_strategy);

    let attempts = max_attempts.max(1);
    let mut last_error = "JSON parse failed".to_string();
    let mut last_raw = String::new();
    let mut attempt_messages = messages.to_vec();

    for attempt in 1..=attempts {
        if token.is_cancelled() {
            return Err(AppError::internal("cancelled"));
        }

        let raw = chat_completion_with_json_format_fallback(
            config,
            model,
            &attempt_messages,
            temperature,
            top_p,
            max_tokens,
            schema,
            &mut strategy,
            auto_detect,
        )
        .await?;

        let json_str = strip_json_fence(&raw);
        match serde_json::from_str::<T>(json_str) {
            Ok(parsed) => {
                if auto_detect && schema.is_some() {
                    *learned_strategy = Some(strategy.into());
                }
                return Ok(parsed);
            }
            Err(err) => {
                last_error = err.to_string();
                last_raw = raw.clone();
                if raw.trim().is_empty() {
                    last_error =
                        "model returned an empty response (reasoning tokens may have exhausted max_tokens)"
                            .to_string();
                }
                if attempt < attempts {
                    // An empty assistant message (no content, no tool calls) is
                    // rejected by some providers (e.g. Mistral), and echoing an
                    // empty reply back adds no context — ask for the JSON again
                    // in a fresh user turn instead.
                    if raw.trim().is_empty() {
                        attempt_messages.push(serde_json::json!({
                            "role": "user",
                            "content": json_empty_response_user_message(repair_hint)
                        }));
                    } else {
                        attempt_messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": raw
                        }));
                        attempt_messages.push(serde_json::json!({
                            "role": "user",
                            "content": json_repair_user_message(&last_error, repair_hint)
                        }));
                    }
                }
            }
        }
    }
    Err(AppError::inference(format_json_parse_failure(
        &last_error,
        &last_raw,
        attempts,
        max_tokens,
    )))
}

fn json_repair_user_message(parse_error: &str, repair_hint: Option<&str>) -> String {
    let hint = repair_hint.map(|h| format!("\n\n{h}")).unwrap_or_default();
    format!(
        "Your previous response was not valid JSON: {parse_error}.{hint}\n\nReply with ONLY corrected JSON — no prose, no markdown fences."
    )
}

fn json_empty_response_user_message(repair_hint: Option<&str>) -> String {
    let hint = repair_hint.map(|h| format!("\n\n{h}")).unwrap_or_default();
    format!(
        "Your previous response was empty.{hint}\n\nReply with ONLY the JSON object — no reasoning, no prose, no markdown fences."
    )
}

fn format_json_parse_failure(
    parse_error: &str,
    raw: &str,
    attempts: u32,
    max_tokens: i64,
) -> String {
    const RAW_LIMIT: usize = 4_000;
    let raw_excerpt = if raw.len() > RAW_LIMIT {
        format!(
            "{}…\n[truncated for display, {} bytes total]",
            &raw[..RAW_LIMIT],
            raw.len()
        )
    } else if raw.is_empty() {
        "(empty response)".to_string()
    } else {
        raw.to_string()
    };
    format!(
        "JSON parse failed after {attempts} attempt(s) (max_tokens={max_tokens}): {parse_error}\n\n---\nRaw model response:\n{raw_excerpt}"
    )
}

/// Extract JSON from a model response that may have wrapped it in a Markdown
/// code fence. Handles the common cases models produce:
/// - the whole response fenced (```json ... ``` or ``` ... ```)
/// - prose before and/or after the fence
/// - a missing closing fence (truncated response)
///
/// When no fence is present the trimmed input is returned unchanged.
fn strip_json_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(open) = trimmed.find("```") else {
        return trimmed;
    };
    // Skip the opening fence and an optional language tag on the same line
    // (e.g. ```json), up to and including the newline.
    let after_open = &trimmed[open + 3..];
    let body = match after_open.find('\n') {
        Some(nl) => &after_open[nl + 1..],
        // Fence and content on one line: ```{...}``` or ```json {...}
        None => after_open.trim_start_matches("json").trim_start(),
    };
    // Take everything up to the closing fence, if there is one.
    let inner = match body.find("```") {
        Some(close) => &body[..close],
        None => body,
    };
    let inner = inner.trim();
    if inner.is_empty() {
        trimmed
    } else {
        inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }
    }

    #[test]
    fn unique_ids_rewrites_duplicate_fallback_ids() {
        let mut calls = vec![
            tool_call("call_fallback_0", "roll_dice"),
            tool_call("abc123", "set_variable"),
            tool_call("call_fallback_0", "set_measurement"),
        ];
        ensure_unique_tool_call_ids(&mut calls);
        let ids: Vec<&str> = calls.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids[0], "call_fallback_0");
        assert_eq!(ids[1], "abc123");
        assert_ne!(ids[2], "call_fallback_0");
        assert!(!ids[2].is_empty());
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn unique_ids_rewrites_empty_ids() {
        let mut calls = vec![tool_call("", "roll_dice"), tool_call("", "roll_dice")];
        ensure_unique_tool_call_ids(&mut calls);
        assert!(calls.iter().all(|c| !c.id.is_empty()));
        assert_ne!(calls[0].id, calls[1].id);
    }

    #[test]
    fn unique_ids_leaves_distinct_ids_untouched() {
        let mut calls = vec![
            tool_call("a1", "roll_dice"),
            tool_call("b2", "set_variable"),
        ];
        ensure_unique_tool_call_ids(&mut calls);
        assert_eq!(calls[0].id, "a1");
        assert_eq!(calls[1].id, "b2");
    }

    #[test]
    fn inference_server_root_strips_v1_suffix() {
        assert_eq!(
            inference_server_root("http://localhost:11434/v1"),
            "http://localhost:11434"
        );
        assert_eq!(
            inference_server_root("http://localhost:11434/v1/"),
            "http://localhost:11434"
        );
        assert_eq!(
            inference_server_root("http://localhost:8080"),
            "http://localhost:8080"
        );
    }

    #[test]
    fn strip_json_fence_handles_fence_variants() {
        // Plain JSON, no fence.
        assert_eq!(strip_json_fence("{\"a\":1}"), "{\"a\":1}");
        // Fenced with json tag.
        assert_eq!(strip_json_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        // Fenced without language tag.
        assert_eq!(strip_json_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
        // Prose before and after the fence.
        assert_eq!(
            strip_json_fence("Here you go:\n```json\n{\"a\":1}\n```\nHope that helps!"),
            "{\"a\":1}"
        );
        // Missing closing fence (truncated response).
        assert_eq!(strip_json_fence("```json\n{\"a\":1}"), "{\"a\":1}");
        // Fence and content on a single line.
        assert_eq!(strip_json_fence("```json {\"a\":1}```"), "{\"a\":1}");
        // Empty fence falls back to the original trimmed text.
        assert_eq!(strip_json_fence("```\n```"), "```\n```");
    }

    #[test]
    fn context_from_model_object_reads_common_keys() {
        let obj = json!({ "id": "m", "max_model_len": 32768 })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(context_from_model_object(&obj), Some(32768));
    }

    #[test]
    fn context_from_ollama_model_info_finds_architecture_key() {
        let info = json!({
            "general.architecture": "llama",
            "llama.context_length": 8192
        });
        assert_eq!(context_from_ollama_model_info(&info), Some(8192));
    }

    #[test]
    fn context_from_llama_props_reads_default_generation_settings() {
        let data = json!({
            "default_generation_settings": { "n_ctx": 65536 }
        });
        assert_eq!(context_from_llama_props(&data), Some(65536));
    }

    #[test]
    fn format_json_parse_failure_includes_error_and_raw_excerpt() {
        let raw = r#"{"checks":[{"label":"test","stakes":"long"#;
        let msg = format_json_parse_failure("EOF while parsing a string", raw, 3, 512);
        assert!(msg.contains("JSON parse failed after 3 attempt(s)"));
        assert!(msg.contains("max_tokens=512"));
        assert!(msg.contains("EOF while parsing a string"));
        assert!(msg.contains("Raw model response:"));
        assert!(msg.contains(raw));
    }

    #[test]
    fn format_json_parse_failure_truncates_long_raw() {
        let raw = "x".repeat(5_000);
        let msg = format_json_parse_failure("invalid", &raw, 1, 768);
        assert!(msg.contains("[truncated for display, 5000 bytes total]"));
    }

    #[test]
    fn json_repair_user_message_includes_hint_when_provided() {
        let msg = json_repair_user_message("missing field `label`", Some("use checks array"));
        assert!(msg.contains("missing field `label`"));
        assert!(msg.contains("use checks array"));
        assert!(msg.contains("no prose"));
    }

    #[test]
    fn json_repair_user_message_omits_hint_when_none() {
        let msg = json_repair_user_message("EOF", None);
        assert!(!msg.contains("Required JSON"));
    }

    #[test]
    fn format_inference_http_error_parses_openai_style_json() {
        let body = r#"{"error":{"message":"model not ready","type":"invalid_request_error","code":"model_pending_deploy"}}"#;
        let msg = format_inference_http_error(StatusCode::BAD_REQUEST, body);
        assert!(msg.contains("HTTP 400 Bad Request"));
        assert!(msg.contains("model_pending_deploy"));
        assert!(msg.contains("model not ready"));
    }

    #[test]
    fn format_inference_http_error_includes_status_for_empty_body() {
        let msg = format_inference_http_error(StatusCode::INTERNAL_SERVER_ERROR, "");
        assert!(msg.contains("HTTP 500 Internal Server Error"));
        assert!(msg.contains("empty response body"));
    }

    #[test]
    fn format_inference_http_error_parses_string_error_field() {
        let body = r#"{"error":"Insufficient balance"}"#;
        let msg = format_inference_http_error(StatusCode::PAYMENT_REQUIRED, body);
        assert!(msg.contains("Insufficient balance"));
    }

    #[test]
    fn format_inference_http_error_surfaces_openrouter_provider_detail() {
        let body = r#"{"error":{"message":"Provider returned error","code":400,"metadata":{"raw":"{\"object\":\"error\",\"message\":\"Assistant message must have either content or tool_calls, but not none.\",\"type\":\"invalid_request_assistant_message\",\"code\":\"3240\"}","provider_name":"Mistral","is_byok":false}}}"#;
        let msg = format_inference_http_error(StatusCode::BAD_REQUEST, body);
        assert!(msg.contains("HTTP 400 Bad Request"));
        assert!(msg.contains("Provider returned error"));
        assert!(msg.contains("Mistral"));
        assert!(msg.contains("Assistant message must have either content or tool_calls"));
    }

    #[test]
    fn format_inference_http_error_includes_non_json_raw_metadata() {
        let body = r#"{"error":{"message":"Provider returned error","code":502,"metadata":{"raw":"upstream timeout after 30s","provider_name":"Chutes"}}}"#;
        let msg = format_inference_http_error(StatusCode::BAD_GATEWAY, body);
        assert!(msg.contains("Chutes"));
        assert!(msg.contains("upstream timeout after 30s"));
    }

    #[test]
    fn format_inference_http_error_dedupes_identical_upstream_message() {
        let body = r#"{"error":{"message":"model overloaded","code":503,"metadata":{"raw":"{\"message\":\"model overloaded\"}","provider_name":"Mistral"}}}"#;
        let msg = format_inference_http_error(StatusCode::SERVICE_UNAVAILABLE, body);
        assert_eq!(msg.matches("model overloaded").count(), 1);
        assert!(msg.contains("Mistral"));
    }

    #[test]
    fn json_empty_response_user_message_asks_for_json_only() {
        let msg = json_empty_response_user_message(Some("use checks array"));
        assert!(msg.contains("response was empty"));
        assert!(msg.contains("use checks array"));
        assert!(msg.contains("ONLY the JSON"));
    }

    #[test]
    fn json_empty_response_user_message_omits_hint_when_none() {
        let msg = json_empty_response_user_message(None);
        assert!(msg.contains("response was empty"));
        assert!(!msg.contains("\n\n\n"));
    }

    #[test]
    fn inference_stream_error_parses_llama_cpp_event() {
        let data =
            r#"{"code":400,"message":"context size exceeded","type":"invalid_request_error"}"#;
        let err = inference_stream_error(data);
        match err {
            AppError::Inference(msg) => {
                assert!(msg.contains("context size exceeded"));
                assert!(msg.contains("HTTP 400"));
            }
            other => panic!("expected inference error, got {other:?}"),
        }
    }
}
