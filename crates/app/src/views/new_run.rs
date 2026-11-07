//! The prompt box, the model picker and the Start button. Emits `Started` once
//! the backend took the run.

use std::time::Duration;

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    select::{Select, SelectItem, SelectState},
    v_flex,
};
use gpui_kit::*;
use proto::{ProviderInfo, RunInfo, StartRun};

use super::format;
use crate::net::Net;

/// How often the picker reads `GET /api/providers` again, so a restarted
/// backend's providers show up without restarting the app.
const PROVIDERS_POLL: Duration = Duration::from_secs(5);

pub enum NewRunEvent {
    Started(RunInfo),
}

pub struct NewRun {
    input: Entity<InputState>,
    /// The provider/model picker. Nothing picked = the backend's default.
    picker: Entity<SelectState<Vec<ProviderOption>>>,
    /// What the picker lists, as the backend last sent it.
    providers: Vec<ProviderInfo>,
    busy: bool,
    error: Option<String>,
    _subscription: Subscription,
    _providers: Task<()>,
}

/// One provider in the picker; its value is the provider's name.
#[derive(Clone)]
struct ProviderOption {
    name: String,
    label: SharedString,
}

impl SelectItem for ProviderOption {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &String {
        &self.name
    }
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
        let picker = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        Self {
            input,
            picker,
            providers: Vec::new(),
            busy: false,
            error: None,
            _subscription: subscription,
            _providers: Self::poll_providers(window, cx),
        }
    }

    /// Keeps the picker in step with `GET /api/providers`.
    fn poll_providers(window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        let net = Net::get(cx);
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let providers = net.call(|api| async move { api.providers().await }).await;
                if let Ok(providers) = providers {
                    let updated = this.update_in(cx, |this, window, cx| {
                        this.set_providers(providers, window, cx);
                    });
                    if updated.is_err() {
                        break; // the view is gone
                    }
                }
                cx.background_executor().timer(PROVIDERS_POLL).await;
            }
        })
    }

    fn set_providers(
        &mut self,
        providers: Vec<ProviderInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if providers == self.providers {
            return;
        }
        let kept = self.picker.read(cx).selected_value().cloned();
        let pick = pick(&providers, kept.as_deref());
        let options: Vec<_> = providers
            .iter()
            .map(|provider| ProviderOption {
                name: provider.name.clone(),
                label: format::provider_option(provider).into(),
            })
            .collect();
        self.picker.update(cx, |picker, cx| {
            picker.set_items(options, window, cx);
            match &pick {
                Some(name) => picker.set_selected_value(name, window, cx),
                None => picker.set_selected_index(None, window, cx),
            }
        });
        self.providers = providers;
        cx.notify();
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
            // The picked provider runs its own model, the one the picker shows.
            provider: self.picker.read(cx).selected_value().cloned(),
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
                Select::new(&self.picker)
                    .small()
                    .placeholder(format::DEFAULT_MODEL)
                    .accessibility_label(format::MODEL_PICKER_LABEL)
                    .disabled(self.busy),
            )
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

/// Which provider the picker shows: the one picked before if the backend still
/// has it, else the backend's default.
fn pick(providers: &[ProviderInfo], kept: Option<&str>) -> Option<String> {
    let has = |name: &str| providers.iter().any(|provider| provider.name == name);
    kept.filter(|name| has(name))
        .map(str::to_owned)
        .or_else(|| {
            providers
                .iter()
                .find(|provider| provider.is_default)
                .map(|provider| provider.name.clone())
        })
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's glob import has its own `test` macro.
    use super::{ProviderInfo, pick};

    fn provider(name: &str, is_default: bool) -> ProviderInfo {
        ProviderInfo {
            name: name.into(),
            kind: "mock".into(),
            model: "scripted".into(),
            vision: true,
            native_computer: false,
            is_default,
        }
    }

    #[test]
    fn the_picker_keeps_a_pick_while_the_backend_has_it() {
        let providers = [provider("mock", true), provider("claude", false)];
        assert_eq!(pick(&providers, None).as_deref(), Some("mock"));
        assert_eq!(pick(&providers, Some("claude")).as_deref(), Some("claude"));
        // The backend restarted without it: back to the default.
        assert_eq!(pick(&providers, Some("local")).as_deref(), Some("mock"));
        // No default and nothing picked: leave it to the backend.
        assert_eq!(pick(&[provider("claude", false)], None), None);
        assert_eq!(pick(&[], Some("claude")), None);
    }
}
