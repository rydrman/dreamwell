use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Duration;

use dreamwell_types::{ModelCapabilities, ModelInfo};
use futures_util::Stream;
use futures_util::StreamExt as FuturesStreamExt;
use reqwest::Client;
use serde_json::Value;
use tokio_stream::StreamExt as TokioStreamExt;

use crate::error::{AppError, AppResult};

/// Resolved inference endpoint credentials for outbound API calls.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    pub base_url: String,
    api_key: Option<String>,
}

impl InferenceConfig {
    pub fn new(base_url: String, api_key: Option<String>) -> Self {
        let api_key = api_key.filter(|key| !key.is_empty());
        Self { base_url, api_key }
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
    let response = config.with_auth(http_client().get(&url)).send().await?;
    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::inference(format!(
            "Failed to list models: {}",
            body
        )));
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
        .with_auth(streaming_client().post(url))
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::inference(format!(
            "Inference server returned {status}: {body}"
        )));
    }

    let stream = TokioStreamExt::timeout(
        FuturesStreamExt::map(response.bytes_stream(), |chunk| {
            chunk.map_err(AppError::from)
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
    )
    .await
}

pub async fn chat_completion_with_format(
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    response_format: Option<&Value>,
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
    let response = config
        .with_auth(http_client().post(url))
        .json(&payload)
        .send()
        .await?;
    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::inference(body));
    }
    let data: Value = response.json().await?;
    Ok(data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string())
}

/// Schema-validated JSON completion with repair retries on parse failure.
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
) -> AppResult<T>
where
    T: serde::de::DeserializeOwned,
{
    let format = response_format.map(|schema| {
        serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "response",
                "strict": true,
                "schema": schema
            }
        })
    });
    let format_ref = format.as_ref();

    let attempts = max_attempts.max(1);
    let mut last_error = "JSON parse failed".to_string();
    let mut last_raw = String::new();
    let mut attempt_messages = messages.to_vec();

    for attempt in 1..=attempts {
        if token.is_cancelled() {
            return Err(AppError::internal("cancelled"));
        }

        let raw = chat_completion_with_format(
            config,
            model,
            &attempt_messages,
            temperature,
            top_p,
            max_tokens,
            format_ref,
        )
        .await?;

        let json_str = strip_json_fence(&raw);
        match serde_json::from_str::<T>(json_str) {
            Ok(parsed) => return Ok(parsed),
            Err(err) => {
                last_error = err.to_string();
                last_raw = raw;
                if attempt < attempts {
                    attempt_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": raw
                    }));
                    attempt_messages.push(serde_json::json!({
                        "role": "user",
                        "content": format!(
                            "Your previous response was not valid JSON: {last_error}. Reply with ONLY corrected JSON."
                        )
                    }));
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

fn strip_json_fence(text: &str) -> &str {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
