//! The prompt box and Start button. Emits `Started` once the backend took the run.

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::*;
use proto::{RunInfo, StartRun};

use super::format;
use crate::net::Net;

pub enum NewRunEvent {
    Started(RunInfo),
}

pub struct NewRun {
    input: Entity<InputState>,
    busy: bool,
    error: Option<String>,
    _subscription: Subscription,
}

impl EventEmitter<NewRunEvent> for NewRun {}

impl NewRun {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(format::PROMPT_PLACEHOLDER));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.start(window, cx);
                }
            });
        Self {
            input,
            busy: false,
            error: None,
            _subscription: subscription,
        }
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.input.read(cx).value().trim().to_owned();
        if prompt.is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();

        let request = StartRun {
            prompt,
            provider: None,
            model: None,
            target: None,
            max_steps: None,
            max_cost_usd: None,
        };
        let started = Net::get(cx).call(move |api| async move { api.start_run(&request).await });
        cx.spawn_in(window, async move |this, cx| {
            let started = started.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match started {
                    Ok(run) => {
                        this.input.update(cx, |input, cx| input.clean(window, cx));
                        cx.emit(NewRunEvent::Started(run));
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for NewRun {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let note = match &self.error {
            Some(error) => div().text_color(theme.danger).child(error.clone()),
            None => div()
                .text_color(theme.muted_foreground)
                .child(format::ENTER_TO_START),
        };
        v_flex()
            .p_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.sidebar_border)
            .child(Input::new(&self.input).disabled(self.busy))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .child(note.text_xs().flex_1().min_w_0())
                    .child(
                        Button::new("start-run")
                            .primary()
                            .small()
                            .label(format::START_RUN)
                            .loading(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.start(window, cx))),
                    ),
            )
    }
}
