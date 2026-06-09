use std::cell::RefCell;
use std::rc::Rc;

use dreamwell_types::*;
use gloo_net::http::{Request, RequestBuilder};
use gloo_timers::callback::Timeout;
use serde::Serialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Event, EventSource, MessageEvent};

struct ReconnectingEventSource {
    url: String,
    on_message: Rc<dyn Fn(String)>,
    source: RefCell<Option<EventSource>>,
    timeout: RefCell<Option<Timeout>>,
    stopped: RefCell<bool>,
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
            attempt: RefCell::new(0),
        });
        inner.connect();
        inner
    }

    fn connect(self: &Rc<Self>) {
        if *self.stopped.borrow() {
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
                inner.stopped.replace(true);
                inner.close_source();
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
                inner.schedule_reconnect();
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
        if *self.stopped.borrow() {
            return;
        }
        self.timeout.borrow_mut().take();
        let attempt = *self.attempt.borrow();
        *self.attempt.borrow_mut() = attempt.saturating_add(1);
        let delay_ms = 1_000u32
            .saturating_mul(2u32.saturating_pow(attempt.min(5)))
            .min(30_000);
        let inner = self.clone();
        let timeout = Timeout::new(delay_ms, move || {
            inner.connect();
        });
        *self.timeout.borrow_mut() = Some(timeout);
    }

    fn stop(&self) {
        *self.stopped.borrow_mut() = true;
        self.timeout.borrow_mut().take();
        self.close_source();
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

async fn send<T: serde::de::DeserializeOwned>(
    request: gloo_net::http::Request,
) -> Result<T, String> {
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.ok() {
        return Err(response
            .text()
            .await
            .unwrap_or_else(|_| response.status_text()));
    }
    response.json().await.map_err(|e| e.to_string())
}

async fn json<T: serde::de::DeserializeOwned>(builder: RequestBuilder) -> Result<T, String> {
    let response = builder.send().await.map_err(|e| e.to_string())?;
    if !response.ok() {
        return Err(response
            .text()
            .await
            .unwrap_or_else(|_| response.status_text()));
    }
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

pub async fn update_chat(id: i64, character_id: i64) -> Result<Chat, String> {
    json_body(
        "PATCH",
        &format!("/api/chats/{id}"),
        &serde_json::json!({ "character_id": character_id }),
    )
    .await
}

pub async fn archive_chat(id: i64) -> Result<(), String> {
    api_request("DELETE", &format!("/api/chats/{id}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
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
    api_request("DELETE", &format!("/api/chats/{id}/permanent"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
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

pub async fn get_queue() -> Result<QueueStatus, String> {
    json(api_request("GET", "/api/chats/queue")).await
}

pub async fn get_jobs() -> Result<QueueStatus, String> {
    json(api_request("GET", "/api/jobs")).await
}

pub async fn cancel_job(id: i64) -> Result<Job, String> {
    let response = api_request("POST", &format!("/api/jobs/{id}/cancel"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.ok() {
        return Err(response
            .text()
            .await
            .unwrap_or_else(|_| response.status_text()));
    }
    response.json().await.map_err(|e| e.to_string())
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
    api_request("DELETE", &format!("/api/characters/{id}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_variables(chat_id: i64) -> Result<Vec<ChatVariable>, String> {
    json(api_request(
        "GET",
        &format!("/api/chats/{chat_id}/variables"),
    ))
    .await
}

pub async fn upsert_variable(chat_id: i64, key: &str, value: &str) -> Result<ChatVariable, String> {
    json_body(
        "PUT",
        &format!("/api/chats/{chat_id}/variables"),
        &serde_json::json!({ "key": key, "value": value }),
    )
    .await
}

pub async fn delete_variable(chat_id: i64, key: &str) -> Result<(), String> {
    api_request(
        "DELETE",
        &format!(
            "/api/chats/{chat_id}/variables/{}",
            js_sys::encode_uri_component(key)
        ),
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_settings() -> Result<Settings, String> {
    json(api_request("GET", "/api/settings")).await
}

pub async fn update_settings(payload: &SettingsUpdate) -> Result<Settings, String> {
    json_body("PATCH", "/api/settings", payload).await
}

pub async fn list_models() -> Result<Vec<ModelInfo>, String> {
    json(api_request("GET", "/api/settings/models")).await
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
    if !response.ok() {
        return Err(response
            .text()
            .await
            .unwrap_or_else(|_| response.status_text()));
    }
    let result: ImportCharacterResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(result.character)
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
}

impl Drop for ChatStream {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

pub async fn list_stories() -> Result<Vec<Story>, String> {
    json(api_request("GET", "/api/stories")).await
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

pub async fn delete_story(id: i64) -> Result<(), String> {
    api_request("DELETE", &format!("/api/stories/{id}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
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
    api_request(
        "DELETE",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}"),
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
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
    api_request(
        "DELETE",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/beats/{beat_id}"),
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
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

pub async fn generate_chapter(story_id: i64, guidance_notes: &str) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/generate-chapter"),
        &serde_json::json!({ "guidance_notes": guidance_notes }),
    )
    .await
}

pub async fn generate_beat(
    story_id: i64,
    chapter_id: i64,
    guidance_notes: &str,
) -> Result<StoryDetail, String> {
    json_body(
        "POST",
        &format!("/api/stories/{story_id}/chapters/{chapter_id}/generate-beat"),
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
}

impl Drop for StoryStream {
    fn drop(&mut self) {
        self.inner.stop();
    }
}
