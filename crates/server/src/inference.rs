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

pub async fn list_models(base_url: &str) -> AppResult<Vec<ModelInfo>> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = http_client().get(&url).send().await?;
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

pub async fn probe_model_capabilities(base_url: &str, model: &str) -> ModelCapabilities {
    if model.trim().is_empty() {
        return ModelCapabilities {
            model: model.to_string(),
            context_length: None,
            context_source: None,
        };
    }

    if let Some((length, source)) = probe_ollama_show(base_url, model).await {
        return ModelCapabilities {
            model: model.to_string(),
            context_length: Some(length),
            context_source: Some(source),
        };
    }

    if let Some((length, source)) = probe_llama_props(base_url, model).await {
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

async fn probe_ollama_show(base_url: &str, model: &str) -> Option<(i64, String)> {
    let root = inference_server_root(base_url);
    let url = format!("{root}/api/show");
    let response = probe_client()
        .post(&url)
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

async fn probe_llama_props(base_url: &str, model: &str) -> Option<(i64, String)> {
    let root = inference_server_root(base_url);
    let response = probe_client()
        .get(format!("{root}/props"))
        .query(&[("model", model)])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        // Some backends expose /props without a model query when only one model is loaded.
        let fallback_url = format!("{root}/props");
        let response = probe_client().get(&fallback_url).send().await.ok()?;
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
    base_url: &str,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
) -> AppResult<Pin<Box<dyn Stream<Item = AppResult<String>> + Send>>> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": true,
    });
    let response = streaming_client().post(url).json(&payload).send().await?;
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
    base_url: &str,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
) -> AppResult<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": false,
    });
    let response = http_client().post(url).json(&payload).send().await?;
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
}
