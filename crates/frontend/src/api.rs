use std::cell::RefCell;
use std::rc::Rc;

use crate::auth;
use crate::sse_client;
use dreamwell_types::*;
use gloo_net::http::{Request, RequestBuilder};
use gloo_timers::callback::Timeout;
use serde::Serialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Event, EventSource, MessageEvent};

const IDLE_RECONNECT_MS: u32 = sse_client::IDLE_RECONNECT_MS;

struct ReconnectingEventSource {
    url: String,
    on_message: Rc<dyn Fn(String)>,
    source: RefCell<Option<EventSource>>,
    timeout: RefCell<Option<Timeout>>,
    stopped: RefCell<bool>,
    paused: RefCell<bool>,
    attempt: RefCell<u32>,
}

impl ReconnectingEventSource {
    fn new(url: String, on_message: impl Fn(String) + 'static) -> Rc<Self> {
        let inner = Rc::new(Self {
            url,
            on_message: Rc::new(on_message),
            source: RefCell::new(None),
            timeout: RefCell::new(None),
            stopped: RefCell::new(false),
            paused: RefCell::new(false),
            attempt: RefCell::new(0),
        });
        inner.connect();
        inner
    }

    fn connect(self: &Rc<Self>) {
        if *self.stopped.borrow() || *self.paused.borrow() {
            return;
        }
        self.close_source();
        self.timeout.borrow_mut().take();

        let Some(source) = EventSource::new(&self.url).ok() else {
            self.schedule_reconnect();
            return;
        };

        {
            let on_message = self.on_message.clone();
            let inner = self.clone();
            let callback = Closure::wrap(Box::new(move |event: MessageEvent| {
                inner.attempt.replace(0);
                if let Some(text) = event.data().as_string() {
                    on_message(text);
                }
            }) as Box<dyn FnMut(_)>);
            source.set_onmessage(Some(callback.as_ref().unchecked_ref()));
            callback.forget();
        }

        {
            let inner = self.clone();
            let callback = Closure::wrap(Box::new(move |_event: Event| {
                inner.attempt.replace(0);
                inner.close_source();
                inner.schedule_idle_reconnect();
            }) as Box<dyn FnMut(_)>);
            let _ =
                source.add_event_listener_with_callback("idle", callback.as_ref().unchecked_ref());
            callback.forget();
        }

        {
            let inner = self.clone();
            let callback = Closure::wrap(Box::new(move |_event: Event| {
                if *inner.stopped.borrow() {
                    return;
                }
                inner.close_source();
                probe_auth_session(inner.clone());
            }) as Box<dyn FnMut(_)>);
            source.set_onerror(Some(callback.as_ref().unchecked_ref()));
            callback.forget();
        }

        *self.source.borrow_mut() = Some(source);
    }

    fn close_source(&self) {
        if let Some(source) = self.source.borrow_mut().take() {
            source.close();
        }
    }

    fn schedule_reconnect(self: &Rc<Self>) {
        if *self.stopped.borrow() || *self.paused.borrow() {
            return;
        }
        self.timeout.borrow_mut().take();
        let attempt = *self.attempt.borrow();
        *self.attempt.borrow_mut() = attempt.saturating_add(1);
        let delay_ms = sse_client::SseConnectionState::error_backoff_ms(attempt);
        self.schedule_reconnect_after(delay_ms);
    }

    fn schedule_idle_reconnect(self: &Rc<Self>) {
        if *self.stopped.borrow() || *self.paused.borrow() {
            return;
        }
        self.schedule_reconnect_after(IDLE_RECONNECT_MS);
    }

    fn schedule_reconnect_after(self: &Rc<Self>, delay_ms: u32) {
        if *self.stopped.borrow() || *self.paused.borrow() {
            return;
        }
        self.timeout.borrow_mut().take();
        let inner = self.clone();
        let timeout = Timeout::new(delay_ms, move || {
            inner.connect();
        });
        *self.timeout.borrow_mut() = Some(timeout);
    }

    fn stop(&self) {
        *self.stopped.borrow_mut() = true;
        *self.paused.borrow_mut() = false;
        self.timeout.borrow_mut().take();
        self.close_source();
    }

    fn pause(self: &Rc<Self>) {
        if *self.stopped.borrow() {
            return;
        }
        *self.paused.borrow_mut() = true;
        self.timeout.borrow_mut().take();
        self.close_source();
    }

    fn reconnect(self: &Rc<Self>) {
        if *self.stopped.borrow() {
            return;
        }
        *self.paused.borrow_mut() = false;
        self.attempt.replace(0);
        self.connect();
    }

    fn resume(self: &Rc<Self>) {
        if *self.stopped.borrow() {
            return;
        }
        *self.paused.borrow_mut() = false;
        self.attempt.replace(0);
        self.connect();
    }
}

fn api_request(method: &str, path: &str) -> RequestBuilder {
    (match method {
        "POST" => Request::post(path),
        "PATCH" => Request::patch(path),
        "PUT" => Request::put(path),
        "DELETE" => Request::delete(path),
        _ => Request::get(path),
    })
    .header("Content-Type", "application/json")
}

pub fn is_auth_expired(err: &str) -> bool {
    auth::is_auth_expired(err)
}

fn response_content_type(response: &gloo_net::http::Response) -> Option<String> {
    response.headers().get("content-type")
}

fn auth_failure_from_response(response: &gloo_net::http::Response) -> Option<String> {
    if auth::is_auth_redirect_response(
        response.status(),
        response_content_type(response).as_deref(),
    ) {
        auth::handle_auth_expiry();
        return Some(auth::auth_expiry_error());
    }
    None
}

async fn response_error_text(response: gloo_net::http::Response) -> String {
    let status = response.status();
    let text = response
        .text()
        .await
        .unwrap_or_else(|_| response.status_text());
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(detail) = json.get("detail").and_then(|v| v.as_str()) {
            if !detail.is_empty() {
                return detail.to_string();
            }
        }
    }
    if text.trim().is_empty() {
        format!("HTTP {status}")
    } else {
        text
    }
}

async fn ensure_ok_response(
    response: gloo_net::http::Response,
) -> Result<gloo_net::http::Response, String> {
    if let Some(err) = auth_failure_from_response(&response) {
        return Err(err);
    }
    if !response.ok() {
        return Err(response_error_text(response).await);
    }
    Ok(response)
}

async fn send_empty(builder: RequestBuilder) -> Result<(), String> {
    let response = builder.send().await.map_err(|e| e.to_string())?;
    ensure_ok_response(response).await?;
    Ok(())
}

fn probe_auth_session(inner: Rc<ReconnectingEventSource>) {
    wasm_bindgen_futures::spawn_local(async move {
        let response = match Request::get("/api/health").send().await {
            Ok(response) => response,
            Err(_) => {
                inner.schedule_reconnect();
                return;
            }
        };
        if auth_failure_from_response(&response).is_some() {
            inner.stop();
            return;
        }
        inner.schedule_reconnect();
    });
}

async fn send<T: serde::de::DeserializeOwned>(
    request: gloo_net::http::Request,
) -> Result<T, String> {
    let response = request.send().await.map_err(|e| e.to_string())?;
    let response = ensure_ok_response(response).await?;
    response.json().await.map_err(|e| e.to_string())
}

async fn json<T: serde::de::DeserializeOwned>(builder: RequestBuilder) -> Result<T, String> {
    let response = builder.send().await.map_err(|e| e.to_string())?;
    let response = ensure_ok_response(response).await?;
    response.json().await.map_err(|e| e.to_string())
}

async fn json_body<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    value: &impl Serialize,
) -> Result<T, String> {
    let request = api_request(method, path)
        .json(value)
        .map_err(|e| e.to_string())?;
    send(request).await
}

pub async fn list_chats() -> Result<Vec<Chat>, String> {
    json(api_request("GET", "/api/chats")).await
}

pub async fn list_archived_chats() -> Result<Vec<Chat>, String> {
    json(api_request("GET", "/api/chats/archived")).await
}

pub async fn create_chat(title: &str, character_id: i64) -> Result<Chat, String> {
    json_body(
        "POST",
        "/api/chats",
        &serde_json::json!({ "title": title, "character_id": character_id }),
    )
    .await
}

pub async fn update_chat(id: i64, payload: &ChatUpdate) -> Result<Chat, String> {
    json_body("PATCH", &format!("/api/chats/{id}"), payload).await
}

pub async fn archive_chat(id: i64) -> Result<(), String> {
    send_empty(api_request("DELETE", &format!("/api/chats/{id}"))).await
}

pub async fn restore_chat(id: i64) -> Result<Chat, String> {
    json_body(
        "POST",
        &format!("/api/chats/{id}/restore"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn permanently_delete_chat(id: i64) -> Result<(), String> {
    send_empty(api_request("DELETE", &format!("/api/chats/{id}/permanent"))).await
}

pub async fn get_messages(chat_id: i64) -> Result<Vec<Message>, String> {
    json(api_request(
        "GET",
        &format!("/api/chats/{chat_id}/messages"),
    ))
    .await
}

pub async fn send_message(chat_id: i64, content: &str) -> Result<Message, String> {
    json_body(
        "POST",
        &format!("/api/chats/{chat_id}/messages"),
        &serde_json::json!({ "content": content }),
    )
    .await
}

pub async fn update_message(
    chat_id: i64,
    message_id: i64,
    content: &str,
    rewind: bool,
) -> Result<Message, String> {
    json_body(
        "PATCH",
        &format!("/api/chats/{chat_id}/messages/{message_id}"),
        &serde_json::json!({ "content": content, "rewind": rewind }),
    )
    .await
}

pub async fn regenerate_message(
    chat_id: i64,
    message_id: i64,
    rewind: bool,
) -> Result<Message, String> {
    json_body(
        "POST",
        &format!("/api/chats/{chat_id}/messages/{message_id}/regenerate"),
        &serde_json::json!({ "rewind": rewind }),
    )
    .await
}

pub async fn recheck_message_variables(chat_id: i64, message_id: i64) -> Result<Job, String> {
    json_body(
        "POST",
        &format!("/api/chats/{chat_id}/messages/{message_id}/variables/recheck"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn summarize_chat(chat_id: i64) -> Result<Job, String> {
    json_body(
        "POST",
        &format!("/api/chats/{chat_id}/summarize"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn delete_chat_summary(chat_id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/chats/{chat_id}/summary"),
    ))
    .await
}

pub async fn regenerate_chat_summary(chat_id: i64, marker_id: i64) -> Result<Job, String> {
    json_body(
        "POST",
        &format!("/api/chats/{chat_id}/summary/regenerate"),
        &serde_json::json!({ "marker_id": marker_id }),
    )
    .await
}

pub async fn rewind_message(chat_id: i64, message_id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/chats/{chat_id}/messages/{message_id}"),
    ))
    .await
}

pub async fn get_queue() -> Result<QueueStatus, String> {
    json(api_request("GET", "/api/chats/queue")).await
}

pub async fn get_jobs() -> Result<QueueStatus, String> {
    json(api_request("GET", "/api/jobs")).await
}

pub async fn cancel_job(id: i64) -> Result<Job, String> {
    json(api_request("POST", &format!("/api/jobs/{id}/cancel"))).await
}

pub async fn list_characters() -> Result<Vec<Character>, String> {
    json(api_request("GET", "/api/characters")).await
}

pub async fn create_character(payload: &CharacterCreate) -> Result<Character, String> {
    json_body("POST", "/api/characters", payload).await
}

pub async fn update_character(id: i64, payload: &CharacterUpdate) -> Result<Character, String> {
    json_body("PATCH", &format!("/api/characters/{id}"), payload).await
}

pub async fn delete_character(id: i64) -> Result<(), String> {
    send_empty(api_request("DELETE", &format!("/api/characters/{id}"))).await
}

pub async fn get_chat_detail(chat_id: i64) -> Result<ChatDetail, String> {
    json(api_request("GET", &format!("/api/chats/{chat_id}"))).await
}

#[allow(dead_code)]
pub async fn patch_chat_state_entry(
    chat_id: i64,
    entry_id: i64,
    payload: &ChatStateEntryUpdate,
) -> Result<ChatDetail, String> {
    json_body(
        "PATCH",
        &format!("/api/chats/{chat_id}/state/{entry_id}"),
        payload,
    )
    .await
}

pub async fn get_variables(chat_id: i64) -> Result<Vec<ChatVariable>, String> {
    json(api_request(
        "GET",
        &format!("/api/chats/{chat_id}/variables"),
    ))
    .await
}

pub async fn upsert_variable(
    chat_id: i64,
    payload: &ChatVariableUpdate,
) -> Result<ChatVariable, String> {
    json_body("PUT", &format!("/api/chats/{chat_id}/variables"), payload).await
}

pub async fn delete_variable(chat_id: i64, variable_id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/chats/{chat_id}/variables/{variable_id}"),
    ))
    .await
}

pub async fn get_health() -> Result<HealthResponse, String> {
    json(api_request("GET", "/api/health")).await
}

pub async fn get_settings() -> Result<Settings, String> {
    json(api_request("GET", "/api/settings")).await
}

pub async fn update_settings(payload: &SettingsUpdate) -> Result<Settings, String> {
    json_body("PATCH", "/api/settings", payload).await
}

pub async fn create_inference_connection(
    payload: &InferenceConnectionCreate,
) -> Result<InferenceConnection, String> {
    json_body("POST", "/api/settings/connections", payload).await
}

pub async fn update_inference_connection(
    id: i64,
    payload: &InferenceConnectionUpdate,
) -> Result<InferenceConnection, String> {
    json_body("PATCH", &format!("/api/settings/connections/{id}"), payload).await
}

pub async fn delete_inference_connection(id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/settings/connections/{id}"),
    ))
    .await
}

pub async fn clone_inference_connection(id: i64) -> Result<InferenceConnection, String> {
    let response = api_request("POST", &format!("/api/settings/connections/{id}/clone"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let response = ensure_ok_response(response).await?;
    response.json().await.map_err(|e| e.to_string())
}

pub async fn list_models() -> Result<Vec<ModelInfo>, String> {
    json(api_request("GET", "/api/settings/models")).await
}

pub async fn list_tool_parsers() -> Result<Vec<String>, String> {
    json(api_request("GET", "/api/settings/tool-parsers")).await
}

pub async fn get_model_capabilities(model: &str) -> Result<ModelCapabilities, String> {
    let encoded = js_sys::encode_uri_component(model);
    json(api_request(
        "GET",
        &format!("/api/settings/model-capabilities?model={encoded}"),
    ))
    .await
}

pub async fn import_character(file: &web_sys::File) -> Result<Character, String> {
    let form = web_sys::FormData::new().map_err(|_| "FormData unsupported".to_string())?;
    form.append_with_blob("file", file)
        .map_err(|_| "append failed".to_string())?;
    let response = gloo_net::http::Request::post("/api/characters/import")
        .body(form)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let response = ensure_ok_response(response).await?;
    let result: ImportCharacterResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(result.character)
}

pub async fn list_scenarios() -> Result<Vec<Scenario>, String> {
    json(api_request("GET", "/api/scenarios")).await
}

pub async fn create_scenario(payload: &ScenarioCreate) -> Result<Scenario, String> {
    json_body("POST", "/api/scenarios", payload).await
}

pub async fn update_scenario(id: i64, payload: &ScenarioUpdate) -> Result<Scenario, String> {
    json_body("PATCH", &format!("/api/scenarios/{id}"), payload).await
}

pub async fn delete_scenario(id: i64) -> Result<(), String> {
    send_empty(api_request("DELETE", &format!("/api/scenarios/{id}"))).await
}

pub async fn export_scenario(id: i64) -> Result<ScenarioExport, String> {
    json(api_request("GET", &format!("/api/scenarios/{id}/export"))).await
}

pub async fn import_scenario(file: &web_sys::File) -> Result<Scenario, String> {
    let form = web_sys::FormData::new().map_err(|_| "FormData unsupported".to_string())?;
    form.append_with_blob("file", file)
        .map_err(|_| "append failed".to_string())?;
    let response = gloo_net::http::Request::post("/api/scenarios/import")
        .body(form)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let response = ensure_ok_response(response).await?;
    let result: ImportScenarioResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(result.scenario)
}

pub async fn generate_character_state(
    payload: &GenerateCharacterStateRequest,
) -> Result<GenerateCharacterStateResponse, String> {
    json_body("POST", "/api/scenarios/generate-character-state", payload).await
}

#[derive(Clone)]
pub struct StreamNudge {
    inner: Rc<ReconnectingEventSource>,
}

impl StreamNudge {
    pub fn pause(&self) {
        ReconnectingEventSource::pause(&self.inner);
    }

    /// Reconnect without tearing down the surrounding UI state.
    pub fn reconnect(&self) {
        ReconnectingEventSource::reconnect(&self.inner);
    }

    /// Resume after tab hide; clears the paused flag and opens a new connection.
    pub fn resume(&self) {
        ReconnectingEventSource::resume(&self.inner);
    }
}

pub struct ChatStream {
    inner: Rc<ReconnectingEventSource>,
}

impl ChatStream {
    pub fn new(chat_id: i64, on_update: impl Fn(ChatStreamPayload) + 'static) -> Self {
        let url = format!("/api/chats/{chat_id}/stream");
        let inner = ReconnectingEventSource::new(url, move |text| {
            if let Ok(payload) = serde_json::from_str::<ChatStreamPayload>(&text) {
                on_update(payload);
            }
        });
        Self { inner }
    }

    pub fn nudge(&self) -> StreamNudge {
        StreamNudge {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for ChatStream {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

pub async fn list_stories() -> Result<Vec<Story>, String> {
    json(api_request("GET", "/api/stories")).await
}

pub async fn list_archived_stories() -> Result<Vec<Story>, String> {
    json(api_request("GET", "/api/stories/archived")).await
}

pub async fn create_story(payload: &StoryCreate) -> Result<StoryDetail, String> {
    json_body("POST", "/api/stories", payload).await
}

pub async fn get_story(id: i64) -> Result<StoryDetail, String> {
    json(api_request("GET", &format!("/api/stories/{id}"))).await
}

pub async fn update_story(id: i64, payload: &StoryUpdate) -> Result<StoryDetail, String> {
    json_body("PATCH", &format!("/api/stories/{id}"), payload).await
}

pub async fn archive_story(id: i64) -> Result<(), String> {
    send_empty(api_request("DELETE", &format!("/api/stories/{id}"))).await
}

pub async fn restore_story(id: i64) -> Result<Story, String> {
    json_body(
        "POST",
        &format!("/api/stories/{id}/restore"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn permanently_delete_story(id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/stories/{id}/permanent"),
    ))
    .await
}

pub async fn update_chapter(
    story_id: i64,
    chapter_id: i64,
    payload: &StoryChapterUpdate,
) -> Result<StoryDetail, String> {
    json_body(
        "PATCH",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}"),
        payload,
    )
    .await
}

pub async fn delete_chapter(story_id: i64, chapter_id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}"),
    ))
    .await
}

pub async fn create_chapter(
    story_id: i64,
    payload: &StoryChapterCreate,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters"),
        payload,
    )
    .await
}

pub async fn update_beat(
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    payload: &StoryBeatUpdate,
) -> Result<StoryDetail, String> {
    json_body(
        "PATCH",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}"),
        payload,
    )
    .await
}

pub async fn delete_beat(story_id: i64, chapter_id: i64, beat_id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}"),
    ))
    .await
}

pub async fn create_beat(
    story_id: i64,
    chapter_id: i64,
    payload: &StoryBeatCreate,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats"),
        payload,
    )
    .await
}

pub async fn propose_chapters(story_id: i64, guidance_notes: &str) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/propose-chapters"),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn propose_beats(
    story_id: i64,
    chapter_id: i64,
    guidance_notes: &str,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/propose-beats"),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn generate_mechanical(
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!(
            "/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}/generate-mechanical"
        ),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn generate_prose(
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}/generate-prose"),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn continue_prose(
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}/continue-prose"),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn summarize_chapter_prose(
    story_id: i64,
    chapter_id: i64,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/summarize-prose"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn recheck_beat_variables(
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
) -> Result<Job, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}/variables/recheck"),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn align_beat_prose(
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
) -> Result<Job, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}/align-prose"),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn get_story_variables(story_id: i64) -> Result<Vec<StoryVariable>, String> {
    json(api_request(
        "GET",
        &format!("/api/stories/{story_id}/variables"),
    ))
    .await
}

pub async fn upsert_story_variable(
    story_id: i64,
    payload: &StoryVariableUpdate,
) -> Result<StoryVariable, String> {
    json_body(
        "PUT",
        &format!("/api/stories/{story_id}/variables"),
        payload,
    )
    .await
}

pub async fn delete_story_variable(story_id: i64, variable_id: i64) -> Result<(), String> {
    send_empty(api_request(
        "DELETE",
        &format!("/api/stories/{story_id}/variables/{variable_id}"),
    ))
    .await
}

pub struct StoryStream {
    inner: Rc<ReconnectingEventSource>,
}

impl StoryStream {
    pub fn new(story_id: i64, on_update: impl Fn(StoryStreamPayload) + 'static) -> Self {
        let url = format!("/api/stories/{story_id}/stream");
        let inner = ReconnectingEventSource::new(url, move |text| {
            if let Ok(payload) = serde_json::from_str::<StoryStreamPayload>(&text) {
                on_update(payload);
            }
        });
        Self { inner }
    }

    pub fn nudge(&self) -> StreamNudge {
        StreamNudge {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for StoryStream {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

pub async fn list_games() -> Result<Vec<Game>, String> {
    json(api_request("GET", "/api/games")).await
}

pub async fn list_archived_games() -> Result<Vec<Game>, String> {
    json(api_request("GET", "/api/games/archived")).await
}

pub async fn import_game_draft(file: &web_sys::File) -> Result<ImportGameDraftResponse, String> {
    let form = web_sys::FormData::new().map_err(|_| "FormData unsupported".to_string())?;
    form.append_with_blob("file", file)
        .map_err(|_| "append failed".to_string())?;
    let response = gloo_net::http::Request::post("/api/games/import")
        .body(form)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let response = ensure_ok_response(response).await?;
    response.json().await.map_err(|e| e.to_string())
}

pub async fn create_game(payload: &GameCreate) -> Result<GameDetail, String> {
    json_body("POST", "/api/games", payload).await
}

pub async fn get_game(id: i64) -> Result<GameDetail, String> {
    json(api_request("GET", &format!("/api/games/{id}"))).await
}

pub async fn update_game(id: i64, payload: &GameUpdate) -> Result<GameDetail, String> {
    json_body("PATCH", &format!("/api/games/{id}"), payload).await
}

pub async fn archive_game(id: i64) -> Result<(), String> {
    send_empty(api_request("DELETE", &format!("/api/games/{id}"))).await
}

pub async fn restore_game(id: i64) -> Result<Game, String> {
    json_body(
        "POST",
        &format!("/api/games/{id}/restore"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn permanently_delete_game(id: i64) -> Result<(), String> {
    send_empty(api_request("DELETE", &format!("/api/games/{id}/permanent"))).await
}

pub async fn submit_turn(game_id: i64, payload: &SubmitTurnRequest) -> Result<GameDetail, String> {
    json_body("POST", &format!("/api/games/{game_id}/turns"), payload).await
}

pub async fn continue_turn(game_id: i64, turn_id: i64) -> Result<GameDetail, String> {
    json_body(
        "POST",
        &format!("/api/games/{game_id}/turns/{turn_id}/continue"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn regenerate_turn(game_id: i64, turn_id: i64) -> Result<GameDetail, String> {
    json_body(
        "POST",
        &format!("/api/games/{game_id}/turns/{turn_id}/regenerate"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn rewind_turn(
    game_id: i64,
    turn_id: i64,
    include_turn: bool,
) -> Result<GameDetail, String> {
    json_body(
        "POST",
        &format!("/api/games/{game_id}/turns/{turn_id}/rewind"),
        &serde_json::json!({ "include_turn": include_turn }),
    )
    .await
}

pub async fn fork_turn(game_id: i64, turn_id: i64) -> Result<GameDetail, String> {
    json_body(
        "POST",
        &format!("/api/games/{game_id}/turns/{turn_id}/fork"),
        &serde_json::json!({}),
    )
    .await
}

pub async fn recheck_turn_prose(
    game_id: i64,
    turn_id: i64,
    guidance_notes: &str,
) -> Result<Job, String> {
    json_body(
        "POST",
        &format!("/api/games/{game_id}/turns/{turn_id}/prose/recheck"),
        &GenerateRequest {
            guidance_notes: guidance_notes.to_string(),
        },
    )
    .await
}

pub async fn recheck_turn_state(
    game_id: i64,
    turn_id: i64,
    guidance_notes: &str,
) -> Result<Job, String> {
    json_body(
        "POST",
        &format!("/api/games/{game_id}/turns/{turn_id}/state/recheck"),
        &GenerateRequest {
            guidance_notes: guidance_notes.to_string(),
        },
    )
    .await
}

pub struct GameStream {
    inner: Rc<ReconnectingEventSource>,
}

impl GameStream {
    pub fn new(game_id: i64, on_update: impl Fn(GameStreamPayload) + 'static) -> Self {
        let url = format!("/api/games/{game_id}/stream");
        let inner = ReconnectingEventSource::new(url, move |text| {
            if let Ok(payload) = serde_json::from_str::<GameStreamPayload>(&text) {
                on_update(payload);
            }
        });
        Self { inner }
    }

    #[allow(dead_code)]
    pub fn nudge(&self) -> StreamNudge {
        StreamNudge {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for GameStream {
    fn drop(&mut self) {
        self.inner.stop();
    }
}
