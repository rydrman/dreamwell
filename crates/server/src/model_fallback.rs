use std::pin::Pin;

use dreamwell_types::{
    connection_label, fallback_connections, resolve_sampling, structured_output_tokens,
    InferenceConnection, SamplingOverrides, Settings,
};
use futures_util::Stream;
use sqlx::SqlitePool;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::inference::{
    chat_completion, chat_completion_json, stream_chat_completion,
    stream_chat_completion_with_tools, ToolStreamChunk,
};

pub fn has_inference_provider(settings: &Settings) -> bool {
    dreamwell_types::has_inference_provider(&settings.connections)
}

fn effective_model(conn: &InferenceConnection, model_override: Option<&str>) -> String {
    model_override
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| conn.model.trim())
        .to_string()
}

fn effective_sampling(
    conn: &InferenceConnection,
    settings: &Settings,
    model: &str,
    sampling_override: Option<SamplingOverrides>,
) -> (f64, f64) {
    let params = resolve_sampling(
        conn.temperature,
        conn.top_p,
        model,
        &settings.model_profiles,
        sampling_override.filter(|overrides| !overrides.is_empty()),
    );
    (params.temperature, params.top_p)
}

fn structured_tokens_for_connection(conn: &InferenceConnection) -> i64 {
    structured_output_tokens(&Settings {
        inference_url: String::new(),
        active_connection_id: None,
        connections: Vec::new(),
        model: conn.model.clone(),
        temperature: conn.temperature,
        top_p: conn.top_p,
        max_tokens: conn.max_tokens,
        system_prompt_prefix: String::new(),
        system_prompt_suffix: String::new(),
        user_name: String::new(),
        persona_description: String::new(),
        summarize_enabled: false,
        summarize_adaptive: false,
        summarize_after_messages: 12,
        summarize_keep_recent: 4,
        variables_enabled: false,
        thought_blocks_enabled: false,
        max_context_messages: conn.max_context_messages,
        context_tokens: conn.context_tokens,
        auto_context_on_model_change: conn.auto_context_on_model_change,
        max_concurrent_jobs: 1,
        model_profiles: Vec::new(),
        chat_model_plan: String::new(),
        chat_model_prose: String::new(),
        chat_temperature_plan: None,
        chat_top_p_plan: None,
        chat_temperature_prose: None,
        chat_top_p_prose: None,
    })
}

fn is_fallback_eligible(err: &AppError) -> bool {
    matches!(err, AppError::Inference(_))
}

fn short_error(err: &AppError) -> String {
    let full = err.to_string();
    if full.len() > 160 {
        format!("{}…", &full[..160])
    } else {
        full
    }
}

pub fn fallback_notice(from: &str, to: &str, err: &AppError) -> String {
    format!(
        "Provider \"{from}\" failed ({error}); trying \"{to}\"…",
        error = short_error(err)
    )
}

pub async fn set_message_generation_notice(
    pool: &SqlitePool,
    message_id: i64,
    notice: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE messages SET generation_notice = ?1 WHERE id = ?2")
        .bind(notice)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_message_generation_notice(pool: &SqlitePool, message_id: i64) -> AppResult<()> {
    sqlx::query("UPDATE messages SET generation_notice = '' WHERE id = ?1")
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn record_inference_attempt(
    pool: &SqlitePool,
    job_id: Option<i64>,
    conn: &InferenceConnection,
    model: &str,
) {
    if let Some(job_id) = job_id {
        let provider = connection_label(conn);
        if let Err(err) = db::set_job_generation_inference(pool, job_id, &provider, model).await {
            tracing::warn!(%err, job_id, "failed to persist job inference label");
        }
    }
}

async fn record_fallback(
    pool: Option<&SqlitePool>,
    job_id: Option<i64>,
    message_id: Option<i64>,
    from: &str,
    to: &str,
    err: &AppError,
) {
    tracing::warn!(from_provider = from, to_provider = to, error = %err, "inference provider fallback");
    let notice = fallback_notice(from, to, err);
    if let Some(pool) = pool {
        if let Some(message_id) = message_id {
            if let Err(update_err) = set_message_generation_notice(pool, message_id, &notice).await
            {
                tracing::warn!(%update_err, "failed to persist message generation notice");
            }
        }
        if let Some(job_id) = job_id {
            if let Err(update_err) = db::set_job_generation_notice(pool, job_id, &notice).await {
                tracing::warn!(%update_err, "failed to persist job generation notice");
            }
        }
    }
}

macro_rules! attempt_connections {
    ($settings:expr, $pool:expr, $job_id:expr, $message_id:expr, $override:expr, $sampling:expr, | $conn:ident, $config:ident, $model:ident, $temperature:ident, $top_p:ident | $attempt:expr) => {{
        let connections: Vec<&InferenceConnection> = fallback_connections(&$settings.connections);
        if connections.is_empty() {
            Err(AppError::bad_request("No inference provider configured"))
        } else {
            let mut last_err = None;
            for (index, $conn) in connections.iter().enumerate() {
                let $config = db::get_inference_config_for_connection($pool, $conn.id).await?;
                let $model = effective_model($conn, $override);
                let ($temperature, $top_p) =
                    effective_sampling($conn, $settings, &$model, $sampling);
                record_inference_attempt($pool, $job_id, $conn, &$model).await;
                match ($attempt).await {
                    Ok(value) => return Ok(value),
                    Err(err) if is_fallback_eligible(&err) && index + 1 < connections.len() => {
                        let next = connections[index + 1];
                        record_fallback(
                            Some($pool),
                            $job_id,
                            $message_id,
                            &connection_label($conn),
                            &connection_label(next),
                            &err,
                        )
                        .await;
                        last_err = Some(err);
                    }
                    Err(err) => return Err(err),
                }
            }
            Err(last_err
                .unwrap_or_else(|| AppError::bad_request("No inference provider configured")))
        }
    }};
}

#[allow(clippy::too_many_arguments)]
pub async fn chat_completion_with_connection_fallback(
    pool: &SqlitePool,
    settings: &Settings,
    messages: &[serde_json::Value],
    job_id: Option<i64>,
    message_id: Option<i64>,
    model_override: Option<&str>,
    sampling_override: Option<SamplingOverrides>,
) -> AppResult<String> {
    attempt_connections!(
        settings,
        pool,
        job_id,
        message_id,
        model_override,
        sampling_override,
        |conn, config, model, temperature, top_p| {
            chat_completion(
                &config,
                &model,
                messages,
                temperature,
                top_p,
                conn.max_tokens,
            )
        }
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat_completion_with_connection_fallback(
    pool: &SqlitePool,
    settings: &Settings,
    messages: &[serde_json::Value],
    job_id: Option<i64>,
    message_id: Option<i64>,
    model_override: Option<&str>,
    sampling_override: Option<SamplingOverrides>,
) -> AppResult<Pin<Box<dyn Stream<Item = AppResult<String>> + Send>>> {
    attempt_connections!(
        settings,
        pool,
        job_id,
        message_id,
        model_override,
        sampling_override,
        |conn, config, model, temperature, top_p| {
            stream_chat_completion(
                &config,
                &model,
                messages,
                temperature,
                top_p,
                conn.max_tokens,
            )
        }
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat_completion_with_tools_connection_fallback(
    pool: &SqlitePool,
    settings: &Settings,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    tool_choice: &serde_json::Value,
    job_id: Option<i64>,
    message_id: Option<i64>,
    model_override: Option<&str>,
    sampling_override: Option<SamplingOverrides>,
) -> AppResult<Pin<Box<dyn Stream<Item = AppResult<ToolStreamChunk>> + Send>>> {
    attempt_connections!(
        settings,
        pool,
        job_id,
        message_id,
        model_override,
        sampling_override,
        |conn, config, model, temperature, top_p| {
            stream_chat_completion_with_tools(
                &config,
                &model,
                messages,
                tools,
                tool_choice,
                temperature,
                top_p,
                conn.max_tokens,
            )
        }
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn chat_completion_json_with_connection_fallback<T>(
    pool: &SqlitePool,
    settings: &Settings,
    messages: &[serde_json::Value],
    response_format: Option<&serde_json::Value>,
    max_attempts: u32,
    token: &tokio_util::sync::CancellationToken,
    job_id: Option<i64>,
    message_id: Option<i64>,
    model_override: Option<&str>,
    sampling_override: Option<SamplingOverrides>,
    repair_hint: Option<&str>,
) -> AppResult<T>
where
    T: serde::de::DeserializeOwned,
{
    let connections: Vec<&InferenceConnection> = fallback_connections(&settings.connections);
    if connections.is_empty() {
        return Err(AppError::bad_request("No inference provider configured"));
    }
    let mut last_err: Option<AppError> = None;
    for (index, conn) in connections.iter().enumerate() {
        let config = db::get_inference_config_for_connection(pool, conn.id).await?;
        let model = effective_model(conn, model_override);
        let (temperature, top_p) = effective_sampling(conn, settings, &model, sampling_override);
        record_inference_attempt(pool, job_id, conn, &model).await;
        let mut learned = None;
        match chat_completion_json(
            &config,
            &model,
            messages,
            temperature,
            top_p,
            structured_tokens_for_connection(conn),
            response_format,
            max_attempts,
            token,
            &mut learned,
            repair_hint,
        )
        .await
        {
            Ok(parsed) => {
                if let (Some(connection_id), Some(strategy)) = (config.connection_id, learned) {
                    db::persist_learned_json_format_strategy(pool, connection_id, strategy).await?;
                }
                return Ok(parsed);
            }
            Err(err) if is_fallback_eligible(&err) && index + 1 < connections.len() => {
                let next = connections[index + 1];
                record_fallback(
                    Some(pool),
                    job_id,
                    message_id,
                    &connection_label(conn),
                    &connection_label(next),
                    &err,
                )
                .await;
                last_err = Some(err);
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::bad_request("No inference provider configured")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dreamwell_types::InferenceConnection;

    fn sample_connection(
        id: i64,
        name: &str,
        enabled: bool,
        sort_order: i64,
    ) -> InferenceConnection {
        InferenceConnection {
            id,
            name: name.into(),
            inference_url: "http://localhost:8080/v1".into(),
            api_key_set: false,
            model: "demo".into(),
            enabled,
            sort_order,
            json_format_strategy: dreamwell_types::JsonFormatStrategy::Auto,
            tool_call_parser: "auto".into(),
            temperature: 0.8,
            top_p: 0.9,
            max_tokens: 512,
            context_tokens: 8192,
            max_context_messages: 40,
            auto_context_on_model_change: true,
        }
    }

    #[test]
    fn fallback_connections_respects_order_and_enabled() {
        let settings = Settings {
            inference_url: String::new(),
            active_connection_id: None,
            connections: vec![
                sample_connection(3, "third", true, 2),
                sample_connection(1, "first", true, 0),
                sample_connection(2, "second", false, 1),
            ],
            model: String::new(),
            temperature: 0.8,
            top_p: 0.9,
            max_tokens: 512,
            system_prompt_prefix: String::new(),
            system_prompt_suffix: String::new(),
            user_name: String::new(),
            persona_description: String::new(),
            summarize_enabled: false,
            summarize_adaptive: false,
            summarize_after_messages: 12,
            summarize_keep_recent: 4,
            variables_enabled: false,
            thought_blocks_enabled: false,
            max_context_messages: 40,
            context_tokens: 8192,
            auto_context_on_model_change: true,
            max_concurrent_jobs: 1,
            model_profiles: Vec::new(),
            chat_model_plan: String::new(),
            chat_model_prose: String::new(),
            chat_temperature_plan: None,
            chat_top_p_plan: None,
            chat_temperature_prose: None,
            chat_top_p_prose: None,
        };
        let labels: Vec<String> = fallback_connections(&settings.connections)
            .into_iter()
            .map(connection_label)
            .collect();
        assert_eq!(labels, vec!["first".to_string(), "third".to_string()]);
    }
}
