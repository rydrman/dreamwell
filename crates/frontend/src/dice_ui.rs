use dreamwell_types::CheckTier;
use yew::prelude::*;

fn dice_sides_from_expr(expr: &str) -> u32 {
    expr.rsplit('d')
        .next()
        .and_then(|s| s.trim().parse().ok())
        .filter(|&sides| sides > 0)
        .unwrap_or(6)
}

pub(crate) fn format_modifier(modifier: i64) -> String {
    if modifier >= 0 {
        format!("+{modifier}")
    } else {
        modifier.to_string()
    }
}

/// Pip grid coordinates (row, col) for standard d6 faces.
fn d6_pips(value: i64) -> &'static [(u8, u8)] {
    match value {
        1 => &[(1, 1)],
        2 => &[(0, 0), (2, 2)],
        3 => &[(0, 0), (1, 1), (2, 2)],
        4 => &[(0, 0), (0, 2), (2, 0), (2, 2)],
        5 => &[(0, 0), (0, 2), (1, 1), (2, 0), (2, 2)],
        6 => &[(0, 0), (0, 2), (1, 0), (1, 2), (2, 0), (2, 2)],
        _ => &[],
    }
}

#[derive(Properties, PartialEq)]
pub struct DieFaceProps {
    pub value: i64,
    pub sides: u32,
}

#[function_component(DieFace)]
pub fn die_face(props: &DieFaceProps) -> Html {
    if props.sides == 6 && (1..=6).contains(&props.value) {
        html! {
            <span class="die die--pip" role="img" aria-label={format!("d6 showing {}", props.value)}>
                { for d6_pips(props.value).iter().map(|&(row, col)| html! {
                    <span
                        class="die-pip"
                        style={format!("grid-row: {}; grid-column: {};", row + 1, col + 1)}
                    />
                }) }
            </span>
        }
    } else {
        html! {
            <span
                class="die die--number"
                role="img"
                aria-label={format!("d{} showing {}", props.sides, props.value)}
            >
                { props.value }
            </span>
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct DiceRollDisplayProps {
    pub rolls: Vec<i64>,
    #[prop_or_default]
    pub dice_expr: Option<String>,
    #[prop_or_default]
    pub modifier: Option<i64>,
    #[prop_or_default]
    pub total: Option<i64>,
    #[prop_or_default]
    pub tier: Option<CheckTier>,
    #[prop_or_default]
    pub label: Option<String>,
    #[prop_or_default]
    pub class: &'static str,
}

fn tier_class(tier: Option<CheckTier>) -> &'static str {
    match tier {
        Some(CheckTier::Fail) => "tier-fail",
        Some(CheckTier::Mixed) => "tier-mixed",
        Some(CheckTier::Strong) => "tier-strong",
        None => "",
    }
}

fn tier_label(tier: Option<CheckTier>) -> &'static str {
    match tier {
        Some(CheckTier::Fail) => "Fail",
        Some(CheckTier::Mixed) => "Mixed",
        Some(CheckTier::Strong) => "Strong",
        None => "",
    }
}

#[function_component(DiceRollDisplay)]
pub fn dice_roll_display(props: &DiceRollDisplayProps) -> Html {
    if props.rolls.is_empty() {
        return html! {};
    }

    let sides = props
        .dice_expr
        .as_deref()
        .map(dice_sides_from_expr)
        .unwrap_or(6);
    let show_calc = props.modifier.is_some() || props.total.is_some();
    let modifier = props.modifier.unwrap_or(0);
    let tier = props.tier;

    html! {
        <div class={classes!("dice-roll-display", props.class, tier_class(tier))}>
            if let Some(label) = props.label.as_ref().filter(|l| !l.is_empty()) {
                <span class="roll-label">{ label }{ ": " }</span>
            }
            <span class="dice-roll" aria-label="Dice results">
                { for props.rolls.iter().map(|&value| html! {
                    <DieFace value={value} sides={sides} />
                }) }
            </span>
            if show_calc {
                if modifier != 0 {
                    <span class="roll-modifier">{ format_modifier(modifier) }</span>
                }
                if let Some(total) = props.total {
                    <span class="roll-equals">{ "=" }</span>
                    <span class="roll-total">{ total }</span>
                }
            }
            if tier.is_some() {
                <span class="tier-badge">{ tier_label(tier) }</span>
            }
            if let Some(expr) = props.dice_expr.as_ref().filter(|e| !e.is_empty()) {
                if props.total.is_none() && props.modifier.is_none() {
                    <span class="muted dice-expr">{ format!(" ({expr})") }</span>
                }
            }
        </div>
    }
}
