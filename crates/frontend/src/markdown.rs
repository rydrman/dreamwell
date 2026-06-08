use pulldown_cmark::{html, Options, Parser};
use yew::prelude::*;

fn preserve_line_breaks(text: &str) -> String {
    text.replace('\n', "  \n")
}

pub fn message_content_to_html(text: &str) -> String {
    let prepared = preserve_line_breaks(text);
    let mut html_output = String::new();
    html::push_html(
        &mut html_output,
        Parser::new_ext(&prepared, Options::empty()),
    );
    html_output
}

pub fn render_message_content(text: &str) -> Html {
    let inner = message_content_to_html(text);
    html! {
        <div class="message-body">
            { Html::from_html_unchecked(AttrValue::from(inner)) }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_italics_and_bold() {
        let html = message_content_to_html("*italic* and **bold**");
        assert!(html.contains("<em>italic</em>"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn preserves_single_line_breaks() {
        let html = message_content_to_html("line one\nline two");
        assert!(html.contains("line one<br"));
        assert!(html.contains("line two"));
    }

    #[test]
    fn leaves_unmatched_asterisks() {
        let html = message_content_to_html("not *italic");
        assert!(!html.contains("<em>"));
    }

    #[test]
    fn renders_long_roleplay_message() {
        let text = concat!(
            "(NOTICE: Type \"Status\" at any time)\n\n",
            "*You and 3 of your close friends have gathered.*\n\n",
            "\"Hello {{User}}?\" She asks."
        );
        let html = message_content_to_html(text);
        assert!(html.contains("<em>"));
        assert!(html.len() > 20);
    }
}
