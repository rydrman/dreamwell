use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, patch, post},
    Json, Router,
};
use dreamwell_types::{
    Chat, ChatCreate, ChatStreamPayload, ChatUpdate, ChatVariable, ChatVariableUpdate, Job,
    Message, MessageRole, OkResponse, QueueStatus, RegenerateMessageRequest,
    RegenerateSummaryRequest, SendMessageRequest, UpdateMessageRequest,
};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::queue::enqueue_generation;
use crate::routes::AppState;
use crate::variables::{
    revert_message_variable_updates, revert_variable_updates_from_messages,
    strip_variable_key_from_chat_messages,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/queue", get(get_queue))
        .route("/archived", get(list_archived_chats))
        .route("/", get(list_chats).post(create_chat))
        .route(
            "/:id",
            get(get_chat).patch(update_chat).delete(archive_chat),
        )
        .route("/:id/restore", post(restore_chat))
        .route("/:id/permanent", delete(permanently_delete_chat))
        .route("/:id/messages", get(list_messages).post(send_message))
        .route(
            "/:id/messages/:message_id",
            patch(update_message).delete(rewind_message),
        )
        .route(
            "/:id/messages/:message_id/regenerate",
            post(regenerate_message),
        )
        .route(
            "/:id/messages/:message_id/variables/recheck",
            post(recheck_message_variables),
        )
        .route("/:id/variables", get(list_variables).put(upsert_variable))
        .route("/:id/variables/:variable_id", delete(delete_variable))
        .route("/:id/summarize", post(summarize_chat))
        .route("/:id/summary", delete(delete_chat_summary))
        .route("/:id/summary/regenerate", post(regenerate_chat_summary))
        .route("/:id/stream", get(stream_chat))
}

async fn list_chats(State(state): State<AppState>) -> AppResult<Json<Vec<Chat>>> {
    Ok(Json(db::list_chats(&state.pool).await?))
}

async fn list_archived_chats(State(state): State<AppState>) -> AppResult<Json<Vec<Chat>>> {
    Ok(Json(db::list_archived_chats(&state.pool).await?))
}

async fn create_chat(
    State(state): State<AppState>,
    Json(payload): Json<ChatCreate>,
) -> AppResult<Json<Chat>> {
    Ok(Json(
        db::create_chat(&state.pool, payload.title, payload.character_id).await?,
    ))
}

async fn get_queue(State(state): State<AppState>) -> AppResult<Json<QueueStatus>> {
    let (running, queued) = db::list_queue(&state.pool).await?;
    Ok(Json(QueueStatus { running, queued }))
}

async fn get_chat(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Chat>> {
    Ok(Json(db::get_chat(&state.pool, id).await?))
}

async fn update_chat(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<ChatUpdate>,
) -> AppResult<Json<Chat>> {
    Ok(Json(db::update_chat(&state.pool, id, payload).await?))
}

async fn archive_chat(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    let _ = db::get_chat(&state.pool, id).await?;
    for job in db::list_active_jobs_for_chat(&state.pool, id).await? {
        let _ = state.queue.cancel_job(&state.pool, job.id).await;
    }
    db::archive_chat(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn restore_chat(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Chat>> {
    Ok(Json(db::restore_chat(&state.pool, id).await?))
}

async fn permanently_delete_chat(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    db::permanently_delete_chat(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<Message>>> {
    crate::summarize::cleanup_stale_summary_markers(&state.pool, id).await?;
    Ok(Json(db::list_messages(&state.pool, id).await?))
}

async fn rewind_after_message(state: &AppState, chat_id: i64, message_id: i64) -> AppResult<usize> {
    let messages = db::list_messages(&state.pool, chat_id).await?;
    let Some(idx) = messages.iter().position(|m| m.id == message_id) else {
        return Err(AppError::not_found("Message not found"));
    };
    let after: Vec<Message> = messages[(idx + 1)..].to_vec();
    for message in &after {
        for job in db::list_active_jobs_for_message(&state.pool, message.id).await? {
            let _ = state.queue.cancel_job(&state.pool, job.id).await;
        }
    }
    let deleted = db::delete_messages_after(&state.pool, chat_id, message_id).await?;
    revert_variable_updates_from_messages(&state.pool, chat_id, &after).await?;
    Ok(deleted.len())
}

async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<SendMessageRequest>,
) -> AppResult<Json<Message>> {
    if payload.content.trim().is_empty() {
        return Err(AppError::bad_request("Message cannot be empty"));
    }
    let _ = db::get_settings(&state.pool).await?;
    db::insert_message(
        &state.pool,
        id,
        MessageRole::User,
        payload.content.trim().to_string(),
        false,
    )
    .await?;
    let assistant = db::insert_message(
        &state.pool,
        id,
        MessageRole::Assistant,
        String::new(),
        false,
    )
    .await?;
    let job = enqueue_generation(&state.pool, &state.queue, id, assistant.id).await?;
    db::touch_chat(&state.pool, id).await?;
    Ok(Json(Message {
        job_status: Some(job.status),
        ..assistant
    }))
}

async fn update_message(
    State(state): State<AppState>,
    Path((id, message_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateMessageRequest>,
) -> AppResult<Json<Message>> {
    if payload.content.trim().is_empty() {
        return Err(AppError::bad_request("Message cannot be empty"));
    }
    let message = db::get_message(&state.pool, id, message_id).await?;
    if message.is_summary {
        return Err(AppError::bad_request("Cannot edit summary messages"));
    }
    if message.role == MessageRole::System {
        return Err(AppError::bad_request("Cannot edit system messages"));
    }
    if payload.rewind {
        rewind_after_message(&state, id, message_id).await?;
    }
    db::update_message_content(&state.pool, message_id, payload.content.trim()).await?;
    if message.role == MessageRole::Assistant {
        db::clear_message_thoughts(&state.pool, message_id).await?;
    }
    db::touch_chat(&state.pool, id).await?;
    Ok(Json(db::get_message(&state.pool, id, message_id).await?))
}

async fn rewind_message(
    State(state): State<AppState>,
    Path((id, message_id)): Path<(i64, i64)>,
) -> AppResult<Json<OkResponse>> {
    let _ = db::get_message(&state.pool, id, message_id).await?;
    rewind_after_message(&state, id, message_id).await?;
    db::touch_chat(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn regenerate_message(
    State(state): State<AppState>,
    Path((id, message_id)): Path<(i64, i64)>,
    Json(payload): Json<RegenerateMessageRequest>,
) -> AppResult<Json<Message>> {
    let message = db::get_message(&state.pool, id, message_id).await?;
    if message.role != MessageRole::Assistant {
        return Err(AppError::bad_request(
            "Only assistant messages can be regenerated",
        ));
    }
    if message.is_summary {
        return Err(AppError::bad_request("Cannot regenerate summary messages"));
    }

    let variable_updates = message.variable_updates.clone();
    let is_last = db::is_last_message(&state.pool, id, message_id).await?;
    if !is_last || payload.rewind {
        rewind_after_message(&state, id, message_id).await?;
    }

    revert_message_variable_updates(&state.pool, id, message_id, &variable_updates).await?;

    for job in db::list_active_jobs_for_message(&state.pool, message_id).await? {
        state.queue.cancel_job(&state.pool, job.id).await?;
    }

    db::update_message_content(&state.pool, message_id, "").await?;
    db::clear_message_thoughts(&state.pool, message_id).await?;
    db::clear_message_variable_updates(&state.pool, message_id).await?;
    let job = enqueue_generation(&state.pool, &state.queue, id, message_id).await?;
    db::touch_chat(&state.pool, id).await?;
    let mut updated = db::get_message(&state.pool, id, message_id).await?;
    updated.job_status = Some(job.status);
    Ok(Json(updated))
}

async fn recheck_message_variables(
    State(state): State<AppState>,
    Path((id, message_id)): Path<(i64, i64)>,
) -> AppResult<Json<Job>> {
    let _ = db::get_message(&state.pool, id, message_id).await?;
    let settings = db::get_settings(&state.pool).await?;
    let job = state
        .queue
        .enqueue_variable_recheck(&state.pool, id, message_id, &settings)
        .await?;
    db::touch_chat(&state.pool, id).await?;
    Ok(Json(job))
}

async fn list_variables(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<ChatVariable>>> {
    Ok(Json(db::list_variables(&state.pool, id).await?))
}

async fn upsert_variable(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<ChatVariableUpdate>,
) -> AppResult<Json<ChatVariable>> {
    Ok(Json(
        db::upsert_variable_manual(&state.pool, id, payload).await?,
    ))
}

async fn delete_variable(
    State(state): State<AppState>,
    Path((id, variable_id)): Path<(i64, i64)>,
) -> AppResult<Json<OkResponse>> {
    let key = db::get_chat_variable(&state.pool, id, variable_id)
        .await?
        .key;
    db::delete_variable(&state.pool, id, variable_id).await?;
    strip_variable_key_from_chat_messages(&state.pool, id, &key).await?;
    db::touch_chat(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn summarize_chat(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Job>> {
    let _ = db::get_chat(&state.pool, id).await?;
    let settings = db::get_settings(&state.pool).await?;
    let job = state
        .queue
        .enqueue_summarize(&state.pool, id, &settings)
        .await?;
    db::touch_chat(&state.pool, id).await?;
    Ok(Json(job))
}

async fn delete_chat_summary(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    crate::summarize::delete_chat_summary(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn regenerate_chat_summary(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<RegenerateSummaryRequest>,
) -> AppResult<Json<Job>> {
    let _ = db::get_chat(&state.pool, id).await?;
    let settings = db::get_settings(&state.pool).await?;
    let job = state
        .queue
        .enqueue_regenerate_summary(&state.pool, id, payload.marker_id, &settings)
        .await?;
    db::touch_chat(&state.pool, id).await?;
    Ok(Json(job))
}

async fn stream_chat(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<impl IntoResponse> {
    let _ = db::get_chat(&state.pool, id).await?;
    let pool = state.pool.clone();
    let interval = Duration::from_millis(state.sse_poll_interval_ms);

    let event_stream = stream! {
        let mut last_payload = String::new();
        loop {
            let chat = match db::get_chat(&pool, id).await {
                Ok(chat) => chat,
                Err(_) => {
                    yield Ok::<_, Infallible>(Event::default().event("error").data("{\"detail\":\"not found\"}"));
                    break;
                }
            };
            let messages = db::list_messages(&pool, id).await.unwrap_or_default();
            let active_job = db::get_active_job(&pool, id).await.ok().flatten();
            let has_active_job = active_job.is_some();
            let payload = serde_json::to_string(&ChatStreamPayload {
                chat,
                messages,
                active_job,
            }).unwrap_or_default();

            if payload != last_payload {
                last_payload = payload.clone();
                yield Ok(Event::default().data(payload));
            }

            if !has_active_job {
                yield Ok(Event::default().event("idle").data(format!("{{\"chat_id\":{id}}}")));
                break;
            }

            tokio::time::sleep(interval).await;
        }
    };

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod stream_tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use dreamwell_types::{CharacterCreate, JobStatus, MessageRole};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::db;
    use crate::queue::JobQueue;
    use crate::routes::AppState;

    use super::stream_chat;

    async fn test_state(poll_ms: u64) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.db");
        let pool = db::connect(&format!("sqlite:{}", path.display()))
            .await
            .expect("connect");
        let queue = JobQueue::new(pool.clone());
        let state = AppState {
            pool,
            queue,
            sse_poll_interval_ms: poll_ms,
        };
        (dir, state)
    }

    async fn seed_chat(state: &AppState) -> (i64, i64) {
        let character = db::create_character(
            &state.pool,
            CharacterCreate {
                name: "Test".into(),
                description: String::new(),
                personality: String::new(),
                scenario: String::new(),
                first_message: String::new(),
                example_dialogue: String::new(),
                system_prompt: String::new(),
                avatar_url: None,
            },
        )
        .await
        .expect("character");
        let chat = db::create_chat(&state.pool, "Chat".into(), character.id)
            .await
            .expect("chat");
        let message = db::insert_message(
            &state.pool,
            chat.id,
            MessageRole::Assistant,
            String::new(),
            false,
        )
        .await
        .expect("message");
        (chat.id, message.id)
    }

    fn parse_sse_events(body: &str) -> Vec<(Option<String>, String)> {
        body.split("\n\n")
            .filter(|block| !block.trim().is_empty())
            .map(|block| {
                let mut event = None;
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(name) = line.strip_prefix("event:") {
                        event = Some(name.trim().to_string());
                    } else if let Some(payload) = line.strip_prefix("data:") {
                        data = payload.trim().to_string();
                    }
                }
                (event, data)
            })
            .collect()
    }

    #[tokio::test]
    async fn stream_emits_data_then_idle_when_no_active_job() {
        let (_dir, state) = test_state(50).await;
        let (chat_id, _message_id) = seed_chat(&state).await;

        let app = Router::new()
            .route("/:id/stream", axum::routing::get(stream_chat))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/{chat_id}/stream"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        let events = parse_sse_events(&text);
        assert!(!events.is_empty());
        assert!(events
            .iter()
            .any(|(event, data)| { event.is_none() && data.contains("\"messages\"") }));
        assert!(events
            .iter()
            .any(|(event, _)| event.as_deref() == Some("idle")));
    }

    #[tokio::test]
    async fn stream_emits_idle_after_job_completes() {
        let (_dir, state) = test_state(50).await;
        let (chat_id, message_id) = seed_chat(&state).await;
        let job = db::enqueue_job(&state.pool, chat_id, message_id)
            .await
            .expect("enqueue");
        sqlx::query("UPDATE generation_jobs SET status = 'running' WHERE id = ?1")
            .bind(job.id)
            .execute(&state.pool)
            .await
            .expect("mark running");

        let pool = state.pool.clone();
        let job_id = job.id;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            db::complete_job(&pool, job_id, JobStatus::Completed, None)
                .await
                .expect("complete");
        });

        let app = Router::new()
            .route("/:id/stream", axum::routing::get(stream_chat))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/{chat_id}/stream"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        let events = parse_sse_events(&text);
        assert!(events
            .iter()
            .any(|(event, _)| event.as_deref() == Some("idle")));
    }
}
