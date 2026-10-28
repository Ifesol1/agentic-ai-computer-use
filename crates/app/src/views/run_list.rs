//! The runs, newest first. Emits `Selected` when one is clicked.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use proto::RunInfo;

use super::{dot, format, status_color};

pub enum RunListEvent {
    Selected(RunInfo),
}

#[derive(Default)]
pub struct RunList {
    runs: Vec<RunInfo>,
    selected: Option<String>,
}

impl EventEmitter<RunListEvent> for RunList {}

impl RunList {
    pub fn set_runs(&mut self, runs: Vec<RunInfo>, cx: &mut Context<Self>) {
        if self.runs != runs {
            self.runs = runs;
            cx.notify();
        }
    }

    pub fn mark_selected(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_owned());
        cx.notify();
    }

    /// The newest run, when nothing is selected yet (so the app opens on something).
    pub fn first_if_none_selected(&self) -> Option<RunInfo> {
        match self.selected {
            Some(_) => None,
            None => self.runs.first().cloned(),
        }
    }
}

impl Render for RunList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (active, hover, muted) = (theme.list_active, theme.list_hover, theme.muted_foreground);
        let rows = self.runs.iter().enumerate().map(|(ix, run)| {
            let selected = self.selected.as_deref() == Some(run.id.as_str());
            let clicked = run.clone();
            h_flex()
                .id(("run", ix))
                .mx_2()
                .px_2()
                .py_2()
                .gap_2()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |row| row.bg(active))
                .when(!selected, |row| row.hover(move |style| style.bg(hover)))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(RunListEvent::Selected(clicked.clone()));
                }))
                .child(dot(status_color(run.status, cx)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(run.prompt.clone()))
                        .child(div().text_xs().text_color(muted).child(format::run_summary(run))),
                )
        });
        v_flex()
            .id("runs")
            .size_full()
            .overflow_y_scroll()
            .py_2()
            .gap_1()
            .children(rows)
    }
}
