use dreamwell_types::*;
use gloo_net::http::{Request, RequestBuilder};
use serde::Serialize;
use wasm_bindgen::JsCast;
use web_sys::EventSource;

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

pub async fn create_chat(title: &str, character_id: Option<i64>) -> Result<Chat, String> {
    json_body(
        "POST",
        "/api/chats",
        &serde_json::json!({ "title": title, "character_id": character_id }),
    )
    .await
}

pub async fn update_chat(id: i64, character_id: Option<i64>) -> Result<Chat, String> {
    json_body(
        "PATCH",
        &format!("/api/chats/{id}"),
        &serde_json::json!({ "character_id": character_id }),
    )
    .await
}

pub async fn delete_chat(id: i64) -> Result<(), String> {
    api_request("DELETE", &format!("/api/chats/{id}"))
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

pub async fn get_queue() -> Result<QueueStatus, String> {
    json(api_request("GET", "/api/chats/queue")).await
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

pub async fn get_facts(chat_id: i64) -> Result<Vec<Fact>, String> {
    json(api_request("GET", &format!("/api/chats/{chat_id}/facts"))).await
}

pub async fn upsert_fact(chat_id: i64, key: &str, value: &str) -> Result<Fact, String> {
    json_body(
        "PUT",
        &format!("/api/chats/{chat_id}/facts"),
        &serde_json::json!({ "key": key, "value": value }),
    )
    .await
}

pub async fn delete_fact(chat_id: i64, key: &str) -> Result<(), String> {
    api_request(
        "DELETE",
        &format!(
            "/api/chats/{chat_id}/facts/{}",
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
    source: EventSource,
}

impl ChatStream {
    pub fn new(chat_id: i64, on_update: impl Fn(ChatStreamPayload) + 'static) -> Self {
        let url = format!("/api/chats/{chat_id}/stream");
        let source = EventSource::new(&url).expect("EventSource");
        let on_update = std::rc::Rc::new(on_update);
        {
            let on_update = on_update.clone();
            let callback = wasm_bindgen::closure::Closure::wrap(Box::new(
                move |event: web_sys::MessageEvent| {
                    if let Some(text) = event.data().as_string() {
                        if let Ok(payload) = serde_json::from_str::<ChatStreamPayload>(&text) {
                            on_update(payload);
                        }
                    }
                },
            ) as Box<dyn FnMut(_)>);
            source.set_onmessage(Some(callback.as_ref().unchecked_ref()));
            callback.forget();
        }
        Self { source }
    }
}

impl Drop for ChatStream {
    fn drop(&mut self) {
        self.source.close();
    }
}
