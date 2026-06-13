use dreamwell_types::MessageVariableUpdate;
use yew::prelude::*;

fn format_variable_update_value(update: &MessageVariableUpdate) -> String {
    if update.deleted {
        if let Some(previous) = &update.previous_value {
            format!("{previous} → (deleted)")
        } else {
            "(deleted)".to_string()
        }
    } else if let Some(previous) = &update.previous_value {
        format!("{previous} → {}", update.value)
    } else {
        update.value.clone()
    }
}

#[derive(Properties, PartialEq)]
pub struct VariableUpdatesBlockProps {
    pub updates: Vec<MessageVariableUpdate>,
}

#[function_component(VariableUpdatesBlock)]
pub fn variable_updates_block(props: &VariableUpdatesBlockProps) -> Html {
    let expanded = use_state(|| false);
    let count = props.updates.len();

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };

    html! {
        <div class="message-variable-updates">
            <button type="button" class="message-variable-updates-toggle" onclick={toggle}>
                <span class="message-variable-updates-label">
                    { format!("Updated variables ({count})") }
                </span>
                <span class="message-variable-updates-chevron" aria-hidden="true">
                    { if *expanded { "▾" } else { "▸" } }
                </span>
            </button>
            if *expanded {
                <div class="message-variable-updates-body">
                    <div class="message-variable-updates-grid" role="table">
                        <div class="message-variable-updates-grid-header" role="columnheader">{"Name"}</div>
                        <div class="message-variable-updates-grid-header" role="columnheader">{"Value"}</div>
                        { for props.updates.iter().map(|update| {
                            html! {
                                <>
                                    <div class="message-variable-updates-key" role="cell">{ &update.key }</div>
                                    <div class="message-variable-updates-value" role="cell">{ format_variable_update_value(update) }</div>
                                </>
                            }
                        }) }
                    </div>
                </div>
            }
        </div>
    }
}
