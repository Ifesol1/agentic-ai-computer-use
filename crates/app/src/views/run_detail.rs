//! One run, live: its steps arrive over the WebSocket as they happen, each with
//! the screenshot taken after it.

use std::{collections::HashMap, sync::Arc, time::Duration};

use client::Client;
use futures::{StreamExt, channel::mpsc};
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use proto::{RunEvent, RunInfo, Step};
use tokio::task::AbortHandle;

use super::{dot, format, status_color};
use crate::net::Net;

/// How wide a step's screenshot is drawn; the height follows the image.
const SCREENSHOT_WIDTH: f32 = 400.;

#[derive(Default)]
pub struct RunDetail {
    run: Option<RunInfo>,
    steps: Vec<Step>,
    /// By step number. Each is fetched once per run, even across reconnects.
    screenshots: HashMap<u32, Screenshot>,
    error: Option<String>,
    stream: Option<AbortHandle>,
    _feed: Option<Task<()>>,
}

/// A step's screenshot, as far as it got.
enum Screenshot {
    Loading,
    Ready(Arc<Image>),
    Failed(String),
}

/// What the network side sends the view.
enum Feed {
    /// (Re)connected: the history replays from the start.
    Reset,
    Event(RunEvent),
    Error(String),
}

impl RunDetail {
    pub fn show(&mut self, run: RunInfo, cx: &mut Context<Self>) {
        if self.run.as_ref().is_some_and(|shown| shown.id == run.id) {
            return;
        }
        if let Some(stream) = self.stream.take() {
            stream.abort();
        }
        let net = Net::get(cx);
        let (feed, mut feed_rx) = mpsc::unbounded();
        self.stream = Some(net.spawn(follow(net.client(), run.id.clone(), feed)));
        self._feed = Some(cx.spawn(async move |this, cx| {
            while let Some(feed) = feed_rx.next().await {
                let applied = this.update(cx, |this, cx| {
                    this.apply(feed, cx);
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        }));
        self.run = Some(run);
        self.steps.clear();
        self.screenshots.clear();
        self.error = None;
        cx.notify();
    }

    fn apply(&mut self, feed: Feed, cx: &mut Context<Self>) {
        let Some(run) = &mut self.run else {
            return;
        };
        let mut fetch = None;
        match feed {
            Feed::Reset => {
                self.steps.clear();
                self.error = None;
                // Keep what arrived; try the failed ones again on the replay.
                self.screenshots
                    .retain(|_, shot| !matches!(shot, Screenshot::Failed(_)));
            }
            Feed::Error(error) => self.error = Some(error),
            Feed::Event(RunEvent::Status { status }) => run.status = status,
            Feed::Event(RunEvent::Step { step }) => {
                run.steps = step.n;
                fetch = step.screenshot_url.clone().map(|url| (step.n, url));
                self.steps.push(step);
            }
            Feed::Event(RunEvent::Usage { usage }) => run.usage = usage,
            Feed::Event(RunEvent::Finished { status, reply }) => {
                run.status = status;
                run.reply = reply;
            }
            Feed::Event(RunEvent::Error { message }) => self.error = Some(message),
        }
        if let Some((n, url)) = fetch {
            self.fetch_screenshot(n, url, cx);
        }
    }

    /// Fetches step `n`'s screenshot through the client, once.
    fn fetch_screenshot(&mut self, n: u32, url: String, cx: &mut Context<Self>) {
        let Some(run_id) = self.run.as_ref().map(|run| run.id.clone()) else {
            return;
        };
        if self.screenshots.contains_key(&n) {
            return;
        }
        self.screenshots.insert(n, Screenshot::Loading);
        let fetched = Net::get(cx).call(move |api| async move { api.screenshot(&url).await });
        cx.spawn(async move |this, cx| {
            let shot = match fetched.await {
                Ok((content_type, bytes)) => match image_format(&content_type) {
                    Some(format) => Screenshot::Ready(Arc::new(Image::from_bytes(format, bytes))),
                    None => Screenshot::Failed(format::unsupported_image(&content_type)),
                },
                Err(error) => Screenshot::Failed(format!("{error:#}")),
            };
            this.update(cx, |this, cx| {
                // Another run may be on screen by now.
                if this.run.as_ref().is_some_and(|run| run.id == run_id) {
                    this.screenshots.insert(n, shot);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &self.run else {
            return;
        };
        let id = run.id.clone();
        let stopped = Net::get(cx).call(move |api| async move { api.stop_run(&id).await });
        cx.spawn(async move |this, cx| {
            if let Err(error) = stopped.await {
                this.update(cx, |this, cx| {
                    this.error = Some(format!("{error:#}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}

/// Follows a run's events on the network runtime. If the connection drops before
/// the run finishes (say the backend restarted), it reconnects and replays.
async fn follow(api: Client, id: String, feed: mpsc::UnboundedSender<Feed>) {
    loop {
        match api.events(&id).await {
            Ok(events) => {
                if feed.unbounded_send(Feed::Reset).is_err() {
                    return;
                }
                let mut events = std::pin::pin!(events);
                while let Some(event) = events.next().await {
                    let event = match event {
                        Ok(event) => event,
                        Err(error) => {
                            let _ = feed.unbounded_send(Feed::Error(format!("{error:#}")));
                            break;
                        }
                    };
                    let finished = matches!(event, RunEvent::Finished { .. });
                    if feed.unbounded_send(Feed::Event(event)).is_err() || finished {
                        return;
                    }
                }
            }
            Err(error) => {
                if feed
                    .unbounded_send(Feed::Error(format!("{error:#}")))
                    .is_err()
                {
                    return;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// The image format a content type names, e.g. `image/png; charset=binary`.
fn image_format(content_type: &str) -> Option<ImageFormat> {
    let mime = content_type.split(';').next().unwrap_or_default().trim();
    ImageFormat::from_mime_type(&mime.to_ascii_lowercase())
}

/// A step's screenshot, or why there isn't one (yet).
fn screenshot_view(n: u32, shot: &Screenshot, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (muted, danger, border) = (theme.muted_foreground, theme.danger, theme.border);
    let note = |color: Hsla, text: String| div().text_xs().text_color(color).child(text);
    match shot {
        Screenshot::Loading => {
            note(muted, format::SCREENSHOT_LOADING.to_owned()).into_any_element()
        }
        Screenshot::Failed(error) => {
            note(danger, format::screenshot_failed(error)).into_any_element()
        }
        Screenshot::Ready(image) => img(image.clone())
            .w(px(SCREENSHOT_WIDTH))
            .rounded_md()
            .border_1()
            .border_color(border)
            .aria_label(format::screenshot_label(n))
            .with_fallback(move || {
                note(
                    danger,
                    format::screenshot_failed(format::SCREENSHOT_UNREADABLE),
                )
                .into_any_element()
            })
            .into_any_element(),
    }
}

impl Render for RunDetail {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, danger) = (theme.muted_foreground, theme.border, theme.danger);
        let Some(run) = &self.run else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child(format::NO_RUN_SELECTED)
                .into_any_element();
        };

        let header = v_flex()
            .p_4()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(run.prompt.clone()),
                    )
                    .when(!run.status.is_final(), |row| {
                        row.child(
                            Button::new("stop-run")
                                .danger()
                                .small()
                                .label(format::STOP)
                                .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(dot(status_color(run.status, cx)))
                            .child(format::status(run.status)),
                    )
                    .child(format::provider_model(run))
                    .child(format::steps(run.steps))
                    .child(format::tokens(&run.usage))
                    .child(format::cost(run.usage.cost_usd)),
            );

        let steps = self.steps.iter().map(|step| {
            h_flex()
                .gap_3()
                .items_start()
                .child(
                    div()
                        .w(px(20.))
                        .flex_none()
                        .text_color(muted)
                        .child(step.n.to_string()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(step.note.clone())
                        .child(
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .text_color(muted)
                                .child(format::action(&step.action))
                                .when_some(step.refused, |row, refused| {
                                    row.child(
                                        div().text_color(danger).child(format::refused(refused)),
                                    )
                                }),
                        )
                        .when_some(self.screenshots.get(&step.n), |column, shot| {
                            column.child(screenshot_view(step.n, shot, cx))
                        }),
                )
        });

        v_flex()
            .size_full()
            .child(header)
            .child(
                v_flex()
                    .id("steps")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .gap_3()
                    .children(steps)
                    .when_some(run.reply.clone(), |list, reply| {
                        list.child(
                            div()
                                .mt_2()
                                .p_3()
                                .rounded_md()
                                .border_1()
                                .border_color(border)
                                .child(reply),
                        )
                    })
                    .when_some(self.error.clone(), |list, error| {
                        list.child(div().text_color(danger).child(error))
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's glob import has its own `test` macro.
    use super::{ImageFormat, image_format};

    #[test]
    fn content_types_pick_the_image_format() {
        assert_eq!(image_format("image/png"), Some(ImageFormat::Png));
        assert_eq!(image_format("image/svg+xml"), Some(ImageFormat::Svg));
        assert_eq!(
            image_format("Image/JPEG; charset=binary"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(image_format("application/json"), None);
        assert_eq!(image_format(""), None);
    }
}
