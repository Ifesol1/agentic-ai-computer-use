//! The window: a sidebar (new run, run list, connection) beside the selected run.

use std::time::Duration;

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use proto::{Health, RunInfo};

use super::{
    dot, format,
    new_run::{NewRun, NewRunEvent},
    run_detail::RunDetail,
    run_list::{RunList, RunListEvent},
};
use crate::net::Net;

/// How often the sidebar refreshes the run list and the connection.
const POLL: Duration = Duration::from_secs(1);

pub struct Workspace {
    new_run: Entity<NewRun>,
    list: Entity<RunList>,
    detail: Entity<RunDetail>,
    connection: Connection,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

enum Connection {
    Connecting,
    Up(Health),
    Down,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let new_run = cx.new(|cx| NewRun::new(window, cx));
        let list = cx.new(|_| RunList::default());
        let detail = cx.new(|_| RunDetail::default());
        let subscriptions = vec![
            cx.subscribe(&new_run, |this, _, event: &NewRunEvent, cx| match event {
                NewRunEvent::Started(run) => this.select(run.clone(), cx),
            }),
            cx.subscribe(&list, |this, _, event: &RunListEvent, cx| match event {
                RunListEvent::Selected(run) => this.select(run.clone(), cx),
            }),
        ];
        Self {
            new_run,
            list,
            detail,
            connection: Connection::Connecting,
            _poll: Self::poll(cx),
            _subscriptions: subscriptions,
        }
    }

    /// Refreshes the connection and the run list every [`POLL`]. The backend can
    /// restart at any time (backend devs do, constantly); the app just catches up.
    fn poll(cx: &mut Context<Self>) -> Task<()> {
        let net = Net::get(cx);
        cx.spawn(async move |this, cx| {
            loop {
                let health = net.call(|api| async move { api.health().await }).await;
                let runs = match health {
                    Ok(_) => net.call(|api| async move { api.runs().await }).await.ok(),
                    Err(_) => None,
                };
                let updated = this.update(cx, |this, cx| {
                    this.connection = match health {
                        Ok(health) => Connection::Up(health),
                        Err(_) => Connection::Down,
                    };
                    if let Some(runs) = runs {
                        let first = this.list.update(cx, |list, cx| {
                            list.set_runs(runs, cx);
                            list.first_if_none_selected()
                        });
                        if let Some(run) = first {
                            this.select(run, cx);
                        }
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break; // the window is gone
                }
                cx.background_executor().timer(POLL).await;
            }
        })
    }

    fn select(&mut self, run: RunInfo, cx: &mut Context<Self>) {
        self.list
            .update(cx, |list, cx| list.mark_selected(&run.id, cx));
        self.detail.update(cx, |detail, cx| detail.show(run, cx));
    }

    fn connection_bar(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let url = Net::get(cx).url().to_owned();
        let (color, text) = match &self.connection {
            Connection::Connecting => (theme.muted_foreground, format::connecting(&url)),
            Connection::Up(health) => (theme.success, format::connected(&url, health.mock)),
            Connection::Down => (theme.danger, format::unreachable(&url)),
        };
        let hint = match &self.connection {
            Connection::Down => Some(format::START_BACKEND_HINT.to_owned()),
            Connection::Up(health) => format::contract_mismatch(health.contract),
            Connection::Connecting => None,
        };
        v_flex()
            .p_3()
            .gap_1()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .text_xs()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(dot(color))
                    .child(text),
            )
            .when_some(hint, |bar, hint| {
                bar.child(div().text_color(theme.muted_foreground).child(hint))
            })
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(
                v_flex()
                    .w(px(340.))
                    .h_full()
                    .flex_none()
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.sidebar_border)
                    .child(self.new_run.clone())
                    .child(div().flex_1().min_h_0().child(self.list.clone()))
                    .child(self.connection_bar(cx)),
            )
            .child(div().flex_1().h_full().min_w_0().child(self.detail.clone()))
    }
}
