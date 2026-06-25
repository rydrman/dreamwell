use yew::prelude::*;

pub fn format_thought_duration(ms: i64) -> String {
    let total_secs = (ms / 1000).max(0);
    if total_secs < 60 {
        format!("{total_secs}s")
    } else {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}m{secs}s")
    }
}

#[derive(Properties, PartialEq)]
pub struct ThoughtBlockProps {
    pub thought_content: String,
    pub thought_duration_ms: Option<i64>,
    pub thought_in_progress: bool,
}

#[function_component(ThoughtBlock)]
pub fn thought_block(props: &ThoughtBlockProps) -> Html {
    let expanded = use_state(|| false);

    let label = if props.thought_in_progress {
        "thinking...".to_string()
    } else if let Some(ms) = props.thought_duration_ms {
        format!("thought for {}", format_thought_duration(ms))
    } else {
        "thought".to_string()
    };

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };

    html! {
        <div class="message-thought">
            <button type="button" class="message-thought-toggle" onclick={toggle}>
                if props.thought_in_progress {
                    <span class="thought-spinner" aria-hidden="true"></span>
                }
                <span class="message-thought-label">{ label }</span>
                <span class="message-thought-chevron" aria-hidden="true">
                    { if *expanded { "▾" } else { "▸" } }
                </span>
            </button>
            if *expanded {
                <pre class="message-thought-body">{ &props.thought_content }</pre>
            }
        </div>
    }
}
