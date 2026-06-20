use dreamwell_types::{game_tone_preset_by_id, GAME_TONE_PRESETS};
use web_sys::HtmlSelectElement;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct GmTonePresetPickerProps {
    pub on_apply: Callback<(String, String)>,
}

#[function_component(GmTonePresetPicker)]
pub fn gm_tone_preset_picker(props: &GmTonePresetPickerProps) -> Html {
    let reset_key = use_state(|| 0u32);

    html! {
        <label class="field game-tone-preset" key={*reset_key}>
            <span class="muted">{"Tone preset"}</span>
            <select
                class="input"
                onchange={{
                    let on_apply = props.on_apply.clone();
                    let reset_key = reset_key.clone();
                    Callback::from(move |e: Event| {
                        let select: HtmlSelectElement = e.target_unchecked_into();
                        let value = select.value();
                        if value.is_empty() {
                            return;
                        }
                        if let Some(preset) = game_tone_preset_by_id(&value) {
                            on_apply.emit((
                                preset.setting.to_string(),
                                preset.gm_style.to_string(),
                            ));
                            reset_key.set(*reset_key + 1);
                        }
                    })
                }}
            >
                <option value="" selected=true>{"Choose a preset…"}</option>
                { for GAME_TONE_PRESETS.iter().map(|preset| {
                    html! {
                        <option value={preset.id}>{ preset.label }</option>
                    }
                }) }
            </select>
            <span class="muted game-tone-preset-hint">
                {"Fills setting and GM style below. You can edit the text after applying."}
            </span>
        </label>
    }
}
