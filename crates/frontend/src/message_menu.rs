use std::rc::Rc;

use gloo_timers::callback::Timeout;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{DomRect, Element, HtmlButtonElement, HtmlElement};
use yew::prelude::*;

const MESSAGE_MENU_VIEWPORT_PADDING: f64 = 8.0;
const MESSAGE_MENU_ANCHOR_GAP: f64 = 4.0;

#[derive(Clone, Copy, PartialEq)]
struct MenuPlacementBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl MenuPlacementBounds {
    fn from_viewport_and_container(container: Option<&DomRect>) -> Self {
        let (viewport_width, viewport_height) = viewport_size();
        let padding = MESSAGE_MENU_VIEWPORT_PADDING;
        let mut bounds = Self {
            min_x: padding,
            min_y: padding,
            max_x: viewport_width - padding,
            max_y: viewport_height - padding,
        };
        if let Some(container) = container {
            bounds.min_x = bounds.min_x.max(container.left());
            bounds.min_y = bounds.min_y.max(container.top());
            bounds.max_x = bounds.max_x.min(container.right());
            bounds.max_y = bounds.max_y.min(container.bottom());
        }
        bounds
    }

    fn height(self) -> f64 {
        (self.max_y - self.min_y).max(0.0)
    }
}

#[derive(Clone, PartialEq)]
struct MenuPlacement {
    top: f64,
    left: f64,
    max_height: Option<f64>,
}

fn viewport_size() -> (f64, f64) {
    web_sys::window()
        .map(|window| {
            let width = window
                .inner_width()
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let height = window
                .inner_height()
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            (width, height)
        })
        .unwrap_or((0.0, 0.0))
}

fn messages_container_element(anchor: &HtmlElement) -> Option<Element> {
    let mut element = anchor.parent_element();
    while let Some(current) = element {
        if current.class_list().contains("messages") {
            return Some(current);
        }
        element = current.parent_element();
    }
    None
}

fn messages_container_rect(anchor: &HtmlElement) -> Option<DomRect> {
    messages_container_element(anchor).map(|element| element.get_bounding_client_rect())
}

fn compute_message_menu_placement(
    anchor: &DomRect,
    menu_width: f64,
    menu_height: f64,
    bounds: MenuPlacementBounds,
    align_end: bool,
) -> MenuPlacement {
    let gap = MESSAGE_MENU_ANCHOR_GAP;
    let mut left = if align_end {
        anchor.right() - menu_width
    } else {
        anchor.left()
    };

    let below_top = anchor.bottom() + gap;
    let above_top = anchor.top() - gap - menu_height;
    let fits_below = below_top + menu_height <= bounds.max_y;
    let fits_above = above_top >= bounds.min_y;
    let mut top = if fits_below {
        below_top
    } else if fits_above {
        above_top
    } else {
        let space_below = bounds.max_y - below_top;
        let space_above = anchor.top() - gap - bounds.min_y;
        if space_below >= space_above {
            below_top
        } else {
            above_top
        }
    };

    if left + menu_width > bounds.max_x {
        left = bounds.max_x - menu_width;
    }
    if left < bounds.min_x {
        left = bounds.min_x;
    }

    let available_height = bounds.height();
    let max_height = if menu_height > available_height {
        Some(available_height.floor())
    } else {
        None
    };
    let effective_height = max_height.unwrap_or(menu_height);

    if top + effective_height > bounds.max_y {
        top = bounds.max_y - effective_height;
    }
    if top < bounds.min_y {
        top = bounds.min_y;
    }

    MenuPlacement {
        top,
        left,
        max_height,
    }
}

fn update_message_menu_position(
    menu_btn_ref: &NodeRef,
    menu_ref: &NodeRef,
    menu_style: &UseStateHandle<Option<String>>,
    align_end: bool,
) {
    let Some(button) = menu_btn_ref.cast::<HtmlElement>() else {
        return;
    };
    let Some(menu) = menu_ref.cast::<HtmlElement>() else {
        return;
    };

    let anchor = button.get_bounding_client_rect();
    let menu_rect = menu.get_bounding_client_rect();
    let menu_width = menu_rect.width().max(menu.offset_width() as f64);
    let menu_height = menu_rect.height().max(menu.offset_height() as f64);
    if menu_width <= 0.0 || menu_height <= 0.0 {
        return;
    }

    let container = messages_container_rect(&button);
    let bounds = MenuPlacementBounds::from_viewport_and_container(container.as_ref());
    let placement =
        compute_message_menu_placement(&anchor, menu_width, menu_height, bounds, align_end);

    let mut style = format!(
        "top:{}px;left:{}px;",
        placement.top.round(),
        placement.left.round()
    );
    if let Some(max_height) = placement.max_height {
        style.push_str(&format!(
            "max-height:{}px;overflow-y:auto;",
            max_height.round()
        ));
    }
    menu_style.set(Some(style));
}

#[derive(Properties, PartialEq)]
pub struct MessageOptionsMenuProps {
    #[prop_or(true)]
    pub align_end: bool,
    #[prop_or(false)]
    pub disabled: bool,
    #[prop_or("Message options".into())]
    pub title: String,
    pub children: Children,
}

#[function_component(MessageOptionsMenu)]
pub fn message_options_menu(props: &MessageOptionsMenuProps) -> Html {
    let menu_open = use_state(|| false);
    let menu_btn_ref = use_node_ref();
    let menu_ref = use_node_ref();
    let menu_style = use_state(|| None::<String>);
    let align_end = props.align_end;

    {
        let menu_open = menu_open.clone();
        let menu_btn_ref = menu_btn_ref.clone();
        let menu_ref = menu_ref.clone();
        let menu_style = menu_style.clone();
        use_effect_with(*menu_open, move |open| {
            let open = *open;
            let reposition = {
                let menu_btn_ref = menu_btn_ref.clone();
                let menu_ref = menu_ref.clone();
                let menu_style = menu_style.clone();
                Rc::new(move || {
                    update_message_menu_position(&menu_btn_ref, &menu_ref, &menu_style, align_end);
                })
            };

            let scroll_container = if open {
                menu_btn_ref
                    .cast::<HtmlElement>()
                    .and_then(|button| messages_container_element(&button))
            } else {
                menu_style.set(None);
                None
            };

            let scroll_callback = Closure::wrap(Box::new({
                let reposition = reposition.clone();
                move |_event: web_sys::Event| reposition()
            }) as Box<dyn FnMut(_)>);

            let resize_callback = Closure::wrap(Box::new({
                let reposition = reposition.clone();
                move |_event: web_sys::Event| reposition()
            }) as Box<dyn FnMut(_)>);

            if open {
                Timeout::new(0, {
                    let reposition = reposition.clone();
                    move || reposition()
                })
                .forget();

                if let Some(container) = scroll_container.as_ref() {
                    let _ = container.add_event_listener_with_callback(
                        "scroll",
                        scroll_callback.as_ref().unchecked_ref(),
                    );
                }
                if let Some(window) = web_sys::window() {
                    let _ = window.add_event_listener_with_callback(
                        "resize",
                        resize_callback.as_ref().unchecked_ref(),
                    );
                }
            }

            move || {
                if open {
                    if let Some(container) = scroll_container.as_ref() {
                        let _ = container.remove_event_listener_with_callback(
                            "scroll",
                            scroll_callback.as_ref().unchecked_ref(),
                        );
                    }
                    if let Some(window) = web_sys::window() {
                        let _ = window.remove_event_listener_with_callback(
                            "resize",
                            resize_callback.as_ref().unchecked_ref(),
                        );
                    }
                }
            }
        });
    }

    let close_menu = {
        let menu_open = menu_open.clone();
        Callback::from(move |_| menu_open.set(false))
    };

    let on_menu_click = {
        let menu_open = menu_open.clone();
        Callback::from(move |e: MouseEvent| {
            e.stop_propagation();
            if e.target()
                .and_then(|target| target.dyn_into::<HtmlButtonElement>().ok())
                .is_some()
            {
                menu_open.set(false);
            }
        })
    };

    html! {
        <div class="message-menu-wrap">
            if *menu_open {
                <div class="message-menu-backdrop" onclick={close_menu.clone()} />
            }
            <button
                type="button"
                class="message-menu-btn"
                ref={menu_btn_ref.clone()}
                title={props.title.clone()}
                onclick={Callback::from({
                    let menu_open = menu_open.clone();
                    move |e: MouseEvent| {
                        e.stop_propagation();
                        menu_open.set(!*menu_open);
                    }
                })}
                disabled={props.disabled}
            >
                {"⋯"}
            </button>
            if *menu_open {
                <div
                    class={classes!(
                        "message-menu",
                        menu_style.is_some().then_some("message-menu--anchored")
                    )}
                    ref={menu_ref.clone()}
                    style={(*menu_style).clone()}
                    onclick={on_menu_click}
                >
                    { for props.children.iter() }
                </div>
            }
        </div>
    }
}
