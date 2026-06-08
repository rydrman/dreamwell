use std::sync::OnceLock;
use std::time::Duration;

use dreamwell_types::ModelInfo;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;

use crate::error::{AppError, AppResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(900);

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
                });
            } else if let Some(obj) = item.as_object() {
                let id = obj
                    .get("id")
                    .or_else(|| obj.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let name = obj.get("name").and_then(|v| v.as_str()).map(str::to_string);
                if !id.is_empty() {
                    result.push(ModelInfo { id, name });
                }
            }
        }
    }
    Ok(result)
}

pub async fn stream_chat_completion(
    base_url: &str,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
) -> AppResult<impl futures_util::Stream<Item = AppResult<String>>> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_tokens,
        "stream": true,
    });
    let response = http_client().post(url).json(&payload).send().await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::inference(format!(
            "Inference server returned {status}: {body}"
        )));
    }

    let stream = response.bytes_stream().map(|chunk| {
        let chunk = chunk.map_err(AppError::from)?;
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

    Ok(stream.flat_map(|result| {
        futures_util::stream::iter(match result {
            Ok(tokens) => tokens.into_iter().map(Ok).collect::<Vec<_>>(),
            Err(err) => vec![Err(err)],
        })
    }))
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
