use dreamwell_types::*;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api;
use crate::game_presets_ui::GmTonePresetPicker;

#[derive(Properties, PartialEq)]
pub struct GameCreateModalProps {
    pub characters: Vec<Character>,
    pub on_close: Callback<()>,
    pub on_create: Callback<GameCreate>,
}

#[derive(Clone, PartialEq)]
struct GameCreateDraft {
    title: String,
    premise: String,
    setting: String,
    gm_style: String,
    opening_message: String,
    pc_name: String,
    pc_description: String,
    character_id: Option<i64>,
    use_as_pc: bool,
}

impl Default for GameCreateDraft {
    fn default() -> Self {
        Self {
            title: "Untitled Game".into(),
            premise: String::new(),
            setting: String::new(),
            gm_style: String::new(),
            opening_message: String::new(),
            pc_name: String::new(),
            pc_description: String::new(),
            character_id: None,
            use_as_pc: false,
        }
    }
}

impl GameCreateDraft {
    fn from_character(character: &Character, use_as_pc: bool) -> Self {
        let payload = CharacterCreate {
            name: character.name.clone(),
            description: character.description.clone(),
            personality: character.personality.clone(),
            scenario: character.scenario.clone(),
            first_message: character.first_message.clone(),
            example_dialogue: character.example_dialogue.clone(),
            system_prompt: character.system_prompt.clone(),
            avatar_url: character.avatar_url.clone(),
        };
        let mode = if use_as_pc {
            GameCharacterImportMode::PlayerCharacter
        } else {
            GameCharacterImportMode::World
        };
        let game = game_create_from_character(payload, mode, None, Some(character.id));
        Self {
            title: game.title,
            premise: game.premise,
            setting: game.setting,
            gm_style: game.gm_style,
            opening_message: game.opening_message,
            pc_name: game.pc_name,
            pc_description: game.pc_description,
            character_id: game.character_id,
            use_as_pc,
        }
    }

    fn from_imported(draft: GameCreate) -> Self {
        let use_as_pc = !draft.pc_name.is_empty();
        Self {
            title: draft.title,
            premise: draft.premise,
            setting: draft.setting,
            gm_style: draft.gm_style,
            opening_message: draft.opening_message,
            pc_name: draft.pc_name,
            pc_description: draft.pc_description,
            character_id: draft.character_id,
            use_as_pc,
        }
    }

    fn to_create(&self) -> GameCreate {
        GameCreate {
            title: self.title.trim().to_string(),
            premise: self.premise.clone(),
            setting: self.setting.clone(),
            gm_style: self.gm_style.clone(),
            opening_message: self.opening_message.clone(),
            character_id: self.character_id,
            scenario_id: None,
            pc_name: self.pc_name.clone(),
            pc_description: self.pc_description.clone(),
            pc_traits: default_game_traits(),
            ..Default::default()
        }
    }
}

#[function_component(GameCreateModal)]
pub fn game_create_modal(props: &GameCreateModalProps) -> Html {
    let draft = use_state(GameCreateDraft::default);
    let selected_character_id = use_state(|| None::<i64>);
    let file_input = use_node_ref();

    let apply_character = {
        let draft = draft.clone();
        let selected_character_id = selected_character_id.clone();
        Callback::from(move |(character, use_as_pc): (Character, bool)| {
            selected_character_id.set(Some(character.id));
            draft.set(GameCreateDraft::from_character(&character, use_as_pc));
        })
    };

    html! {
        <>
            <div class="modal-backdrop" onclick={props.on_close.reform(|_| ())} />
            <div class="modal modal-wide">
                <h2>{"Start a game"}</h2>
                <p class="muted game-create-lead">{"Set the scene with an opening message, then take your first action."}</p>

                <div class="game-create-source">
                    <div class="game-create-source-actions">
                        <span class="game-create-source-label muted">{"Import from"}</span>
                        <button class="btn secondary btn-compact" onclick={{
                            let file_input = file_input.clone();
                            Callback::from(move |_| {
                                if let Some(input) = file_input.cast::<HtmlInputElement>() {
                                    input.click();
                                }
                            })
                        }}>{"JSON/PNG card"}</button>
                    </div>
                    <p class="muted game-create-card-hint">{"Character cards map world text and opening hooks into the fields below."}</p>
                    <input type="file" accept=".json,.png" ref={file_input} style="display:none;" onchange={{
                        let draft = draft.clone();
                        let selected_character_id = selected_character_id.clone();
                        Callback::from(move |e: Event| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            if let Some(file) = input.files().and_then(|f| f.get(0)) {
                                let draft = draft.clone();
                                let selected_character_id = selected_character_id.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(imported) = api::import_game_draft(&file).await {
                                        selected_character_id.set(None);
                                        draft.set(GameCreateDraft::from_imported(imported.draft));
                                    }
                                });
                            }
                        })
                    }} />
                    if props.characters.is_empty() {
                        <p class="muted">{"No saved characters yet. Import a card or create one from the Character menu."}</p>
                    } else {
                        <div class="modal-list game-create-character-list">
                            { for props.characters.iter().map(|character| {
                                let id = character.id;
                                let selected = *selected_character_id == Some(id);
                                let pick_world = character.clone();
                                let pick_pc = character.clone();
                                html! {
                                    <div
                                        key={id}
                                        class={classes!("modal-item", selected.then_some("modal-item-selected"))}
                                        onclick={{
                                            let apply_character = apply_character.clone();
                                            let use_as_pc = draft.use_as_pc;
                                            let character = character.clone();
                                            Callback::from(move |_| apply_character.emit((character.clone(), use_as_pc)))
                                        }}
                                    >
                                        <span>{ &character.name }</span>
                                        <div style="display:flex;gap:0.35rem;margin-top:0.35rem;">
                                            <button
                                                type="button"
                                                class="btn secondary btn-compact"
                                                onclick={{
                                                    let apply_character = apply_character.clone();
                                                    let pick_world = pick_world.clone();
                                                    Callback::from(move |e: MouseEvent| {
                                                        e.stop_propagation();
                                                        apply_character.emit((pick_world.clone(), false));
                                                    })
                                                }}
                                            >{"World"}</button>
                                            <button
                                                type="button"
                                                class="btn secondary btn-compact"
                                                onclick={{
                                                    let apply_character = apply_character.clone();
                                                    let pick_pc = pick_pc.clone();
                                                    Callback::from(move |e: MouseEvent| {
                                                        e.stop_propagation();
                                                        apply_character.emit((pick_pc.clone(), true));
                                                    })
                                                }}
                                            >{"As PC"}</button>
                                        </div>
                                    </div>
                                }
                            }) }
                        </div>
                    }
                </div>

                <div class="game-create-form">
                    <label class="field">
                        <span class="muted">{"Game title"}</span>
                        <input type="text" value={draft.title.clone()} oninput={draft_input(draft.clone(), DraftField::Title)} />
                    </label>
                    <label class="field">
                        <span class="muted">{"Opening message"}</span>
                        <textarea rows="4" value={draft.opening_message.clone()} oninput={draft_input(draft.clone(), DraftField::OpeningMessage)} />
                    </label>
                    <label class="field">
                        <span class="muted">{"Premise / scenario"}</span>
                        <textarea rows="3" value={draft.premise.clone()} oninput={draft_input(draft.clone(), DraftField::Premise)} />
                    </label>
                    <GmTonePresetPicker on_apply={Callback::from({
                        let draft = draft.clone();
                        move |(setting, gm_style)| {
                            let mut next = (*draft).clone();
                            next.setting = setting;
                            next.gm_style = gm_style;
                            draft.set(next);
                        }
                    })} />
                    <label class="field">
                        <span class="muted">{"Setting / tone"}</span>
                        <textarea rows="3" value={draft.setting.clone()} oninput={draft_input(draft.clone(), DraftField::Setting)} />
                    </label>
                    <label class="field">
                        <span class="muted">{"GM style"}</span>
                        <textarea rows="2" value={draft.gm_style.clone()} oninput={draft_input(draft.clone(), DraftField::GmStyle)} />
                    </label>
                    <div class="game-create-pc-grid">
                        <label class="field">
                            <span class="muted">{"PC name"}</span>
                            <input type="text" value={draft.pc_name.clone()} oninput={draft_input(draft.clone(), DraftField::PcName)} />
                        </label>
                        <label class="field">
                            <span class="muted">{"PC description"}</span>
                            <textarea rows="3" value={draft.pc_description.clone()} oninput={draft_input(draft.clone(), DraftField::PcDescription)} />
                        </label>
                    </div>
                </div>

                <div style="display:flex;gap:0.5rem;margin-top:0.75rem;">
                    <button class="btn" onclick={{
                        let draft = draft.clone();
                        let on_create = props.on_create.clone();
                        Callback::from(move |_| on_create.emit(draft.to_create()))
                    }}>{"Start game"}</button>
                    <button class="btn secondary" onclick={props.on_close.reform(|_| ())}>{"Cancel"}</button>
                </div>
            </div>
        </>
    }
}

#[derive(Clone, Copy)]
enum DraftField {
    Title,
    OpeningMessage,
    Premise,
    Setting,
    GmStyle,
    PcName,
    PcDescription,
}

fn draft_input(draft: UseStateHandle<GameCreateDraft>, field: DraftField) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        let value = input.value();
        let mut next = (*draft).clone();
        match field {
            DraftField::Title => next.title = value,
            DraftField::OpeningMessage => next.opening_message = value,
            DraftField::Premise => next.premise = value,
            DraftField::Setting => next.setting = value,
            DraftField::GmStyle => next.gm_style = value,
            DraftField::PcName => next.pc_name = value,
            DraftField::PcDescription => next.pc_description = value,
        }
        draft.set(next);
    })
}
