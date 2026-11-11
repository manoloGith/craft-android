//! A strip of document tabs, for apps that show several documents in one window (as they do on
//! Android, where an activity has one window): one tab per open document with its name, a dot
//! while it has unsaved changes and a close button, the shown one highlighted, and a button for a
//! new document. The strip only draws and reports what was clicked ([`TabAction`]); the app does
//! it with its own commands (show, close — asking about unsaved changes —, new).
//!
//! Platform independent, tested on any machine.
//!
//! ```ignore
//! let tabs: Vec<Tab> = app.documents().map(|d| Tab::new(d.id, d.title, d.dirty, d.active)).collect();
//! if let Some(action) = TabStrip::new(&tabs).new_tooltip("New document").close_tooltip("Close").show_panel(ui).action {
//!     match action { TabAction::Activate(id) => …, TabAction::Close(id) => …, TabAction::New => … }
//! }
//! ```

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Stroke, Ui, pos2, vec2};

/// An open document, as its tab shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    /// The app's id for the document, handed back in [`TabAction`].
    pub id: u64,
    pub title: String,
    /// Unsaved changes: the tab shows a dot (a close button while the pointer is on it).
    pub dirty: bool,
    /// The document the window shows.
    pub active: bool,
}

impl Tab {
    pub fn new(id: u64, title: impl Into<String>, dirty: bool, active: bool) -> Tab {
        Tab { id, title: title.into(), dirty, active }
    }
}

/// What the person asked for in the strip, for the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAction {
    /// Show this document (a click on its tab).
    Activate(u64),
    /// Close this document (its close button, or a middle click on the tab). The app asks about
    /// unsaved changes as its own Close does.
    Close(u64),
    /// Start a new document (the + button).
    New,
}

/// Colours and sizes, in points. [`TabStyle::from_visuals`] follows the app's light or dark theme.
#[derive(Clone, Debug, PartialEq)]
pub struct TabStyle {
    pub height: f32,
    pub min_tab_width: f32,
    pub max_tab_width: f32,
    pub font: FontId,
    /// The strip behind the tabs.
    pub bar: Color32,
    /// The shown document's tab (the page side of the window).
    pub active: Color32,
    pub hover: Color32,
    pub text: Color32,
    pub text_inactive: Color32,
    /// The line under the shown tab, and the unsaved dot.
    pub accent: Color32,
    pub separator: Color32,
}

impl TabStyle {
    pub fn from_visuals(visuals: &egui::Visuals) -> TabStyle {
        TabStyle {
            height: 30.0,
            min_tab_width: 96.0,
            max_tab_width: 220.0,
            font: FontId::proportional(13.0),
            bar: visuals.faint_bg_color,
            active: visuals.panel_fill,
            hover: visuals.widgets.hovered.weak_bg_fill,
            text: visuals.strong_text_color(),
            text_inactive: visuals.weak_text_color(),
            accent: visuals.selection.bg_fill,
            separator: visuals.widgets.noninteractive.bg_stroke.color,
        }
    }
}

/// Where each part of the strip was drawn on the last frame, for tests and for apps that place
/// something next to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabStripResponse {
    pub action: Option<TabAction>,
    /// Each tab's id with its whole rectangle and its close button's.
    pub tabs: Vec<(u64, Rect, Rect)>,
    /// The + button, when the strip has one.
    pub new_button: Option<Rect>,
}

/// The strip. Build it each frame from the open documents.
pub struct TabStrip<'a> {
    tabs: &'a [Tab],
    style: Option<TabStyle>,
    new_tooltip: Option<String>,
    close_tooltip: Option<String>,
    id: Id,
}

const PAD_X: f32 = 10.0;
const CLOSE: f32 = 18.0;
const GAP: f32 = 2.0;

impl<'a> TabStrip<'a> {
    pub fn new(tabs: &'a [Tab]) -> TabStrip<'a> {
        TabStrip { tabs, style: None, new_tooltip: None, close_tooltip: None, id: Id::unique("craft_android_tabs") }
    }

    /// Colours and sizes; by default [`TabStyle::from_visuals`].
    pub fn style(mut self, style: TabStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Show the + button, with this tooltip (in the app's language). Without it there is no +.
    pub fn new_tooltip(mut self, text: impl Into<String>) -> Self {
        self.new_tooltip = Some(text.into());
        self
    }

    /// The close buttons' tooltip, in the app's language.
    pub fn close_tooltip(mut self, text: impl Into<String>) -> Self {
        self.close_tooltip = Some(text.into());
        self
    }

    /// Another id, for a second strip in the same window.
    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id = Id::unique(("craft_android_tabs", salt));
        self
    }

    /// Draw the strip as a panel along the top of what is left of `ui` (put it where it goes:
    /// after the app's ribbon or toolbar, before its page).
    pub fn show_panel(self, ui: &mut Ui) -> TabStripResponse {
        let style = self.style.clone().unwrap_or_else(|| TabStyle::from_visuals(ui.visuals()));
        egui::Panel::top(self.id.with("panel"))
            .exact_size(style.height)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(style.bar))
            .show(ui, |ui| self.show(ui))
            .inner
    }

    /// Draw the strip in `ui`, which should be `style.height` tall.
    pub fn show(self, ui: &mut Ui) -> TabStripResponse {
        let style = self.style.clone().unwrap_or_else(|| TabStyle::from_visuals(ui.visuals()));
        let mut out = TabStripResponse::default();
        let full = ui.max_rect();
        ui.painter().hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0, style.separator));
        let active = self.tabs.iter().find(|t| t.active).map(|t| t.id);
        // Bring the shown tab into view when it changes (a new or switched document).
        let last_id = self.id.with("last_active");
        let changed = ui.data_mut(|d| {
            let last = d.get_temp::<Option<u64>>(last_id).flatten();
            d.insert_temp(last_id, active);
            last != active
        });
        egui::ScrollArea::horizontal()
            .id_salt(self.id.with("scroll"))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    ui.add_space(4.0);
                    for tab in self.tabs {
                        let (rect, close, action) = self.tab(ui, tab, &style);
                        if tab.active && changed {
                            ui.scroll_to_rect(rect, None);
                        }
                        out.tabs.push((tab.id, rect, close));
                        out.action = out.action.or(action);
                    }
                    if let Some(tip) = &self.new_tooltip {
                        let (rect, response) = ui.allocate_exact_size(vec2(style.height, style.height), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(rect.shrink(3.0), CornerRadius::same(4), style.hover);
                        }
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, "+", FontId::proportional(style.font.size + 5.0), style.text);
                        if response.on_hover_text(tip.as_str()).clicked() {
                            out.action = out.action.or(Some(TabAction::New));
                        }
                        out.new_button = Some(rect);
                    }
                });
            });
        out
    }

    /// One tab: its rectangle, its close button's, and what was clicked on it.
    fn tab(&self, ui: &mut Ui, tab: &Tab, style: &TabStyle) -> (Rect, Rect, Option<TabAction>) {
        let color = if tab.active { style.text } else { style.text_inactive };
        let title = title_galley(ui, &tab.title, style, color);
        let width = (PAD_X + title.size().x + 6.0 + CLOSE + PAD_X * 0.6).clamp(style.min_tab_width, style.max_tab_width);
        let (rect, response) = ui.allocate_exact_size(vec2(width, style.height), Sense::click());
        let close = Rect::from_center_size(pos2(rect.right() - PAD_X * 0.6 - CLOSE / 2.0, rect.center().y), vec2(CLOSE, CLOSE));
        let close_response = ui.interact(close, self.id.with(("close", tab.id)), Sense::click());
        let painter = ui.painter_at(rect);
        let body = rect.shrink2(vec2(0.0, 3.0)).with_max_y(rect.bottom());
        if tab.active {
            painter.rect_filled(body, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, style.active);
            painter.hline(body.x_range(), body.top() + 1.0, Stroke::new(2.0, style.accent));
        } else if response.hovered() || close_response.hovered() {
            painter.rect_filled(body, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, style.hover);
        }
        let text_pos = pos2(rect.left() + PAD_X, rect.center().y - title.size().y / 2.0 + 1.5);
        painter.galley(text_pos, title, color);
        // The unsaved dot gives way to the close button under the pointer.
        if close_response.hovered() {
            painter.rect_filled(close, CornerRadius::same(4), style.separator.gamma_multiply(0.6));
        }
        if tab.dirty && !close_response.hovered() && !response.hovered() {
            painter.circle_filled(close.center() + vec2(0.0, 1.0), 3.5, style.accent);
        } else {
            painter.text(close.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, "×", FontId::proportional(style.font.size + 3.0), color);
        }
        let mut action = None;
        if close_response.clicked() || response.middle_clicked() {
            action = Some(TabAction::Close(tab.id));
        } else if response.clicked() && !tab.active {
            action = Some(TabAction::Activate(tab.id));
        }
        // The full name, for a title cut with "…".
        response.on_hover_text(tab.title.as_str());
        if let Some(tip) = &self.close_tooltip {
            close_response.on_hover_text(tip.as_str());
        }
        (rect, close, action)
    }
}

/// The title on one line, cut with "…" when it doesn't fit the widest tab.
fn title_galley(ui: &Ui, title: &str, style: &TabStyle, color: Color32) -> std::sync::Arc<egui::Galley> {
    let room = (style.max_tab_width - PAD_X - 6.0 - CLOSE - PAD_X * 0.6).max(8.0);
    let mut job = LayoutJob::single_section(title.to_owned(), TextFormat { font_id: style.font.clone(), color, ..Default::default() });
    job.wrap = TextWrapping::truncate_at_width(room);
    ui.ctx().fonts_mut(|f| f.layout_job(job))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs() -> Vec<Tab> {
        vec![Tab::new(7, "Report", false, true), Tab::new(9, "Letter to the editor", true, false)]
    }

    /// Runs one frame with `input` and the strip in a top panel, returning what it reported.
    fn frame(ctx: &egui::Context, tabs: &[Tab], input: egui::RawInput) -> TabStripResponse {
        let mut out = TabStripResponse::default();
        let mut output = ctx.run_ui(input, |ui| {
            out = TabStrip::new(tabs).new_tooltip("New").close_tooltip("Close").show_panel(ui);
        });
        // No renderer here: the fonts' texture is dropped (egui checks that it was taken).
        output.textures_delta.clear();
        out
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    fn press(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
    }

    /// A click at `pos` with `button`, over three frames (move, press, release); the action of the
    /// release frame.
    fn click(ctx: &egui::Context, tabs: &[Tab], pos: egui::Pos2, button: egui::PointerButton) -> Option<TabAction> {
        frame(ctx, tabs, input(vec![egui::Event::PointerMoved(pos)]));
        frame(ctx, tabs, input(vec![press(pos, button, true)]));
        frame(ctx, tabs, input(vec![press(pos, button, false)])).action
    }

    fn laid_out(ctx: &egui::Context, tabs: &[Tab]) -> TabStripResponse {
        frame(ctx, tabs, input(Vec::new()));
        frame(ctx, tabs, input(Vec::new()))
    }

    #[test]
    fn every_document_gets_a_tab_in_order_and_a_new_button_after_them() {
        let ctx = egui::Context::default();
        let out = laid_out(&ctx, &tabs());
        let ids: Vec<u64> = out.tabs.iter().map(|(id, ..)| *id).collect();
        assert_eq!(ids, [7, 9]);
        let (first, second) = (out.tabs[0].1, out.tabs[1].1);
        assert!(first.right() <= second.left(), "{first:?} {second:?}");
        assert!(out.new_button.is_some_and(|b| b.left() >= second.right()));
        assert!(first.height() > 20.0 && first.top() >= 0.0);
        assert_eq!(out.action, None);
    }

    #[test]
    fn a_click_on_another_tab_shows_it_and_on_the_shown_one_does_nothing() {
        let ctx = egui::Context::default();
        let t = tabs();
        let out = laid_out(&ctx, &t);
        let left_of = |r: Rect| pos2(r.left() + 12.0, r.center().y);
        assert_eq!(click(&ctx, &t, left_of(out.tabs[1].1), egui::PointerButton::Primary), Some(TabAction::Activate(9)));
        assert_eq!(click(&ctx, &t, left_of(out.tabs[0].1), egui::PointerButton::Primary), None);
    }

    #[test]
    fn the_close_button_or_a_middle_click_closes_that_tab() {
        let ctx = egui::Context::default();
        let t = tabs();
        let out = laid_out(&ctx, &t);
        assert_eq!(click(&ctx, &t, out.tabs[1].2.center(), egui::PointerButton::Primary), Some(TabAction::Close(9)));
        assert_eq!(click(&ctx, &t, out.tabs[0].2.center(), egui::PointerButton::Primary), Some(TabAction::Close(7)));
        let body = pos2(out.tabs[1].1.left() + 12.0, out.tabs[1].1.center().y);
        assert_eq!(click(&ctx, &t, body, egui::PointerButton::Middle), Some(TabAction::Close(9)));
    }

    #[test]
    fn the_plus_button_asks_for_a_new_document() {
        let ctx = egui::Context::default();
        let t = tabs();
        let out = laid_out(&ctx, &t);
        let plus = out.new_button.map(|r| r.center()).unwrap_or_default();
        assert_eq!(click(&ctx, &t, plus, egui::PointerButton::Primary), Some(TabAction::New));
    }

    #[test]
    fn a_long_title_keeps_the_tab_at_its_widest_and_no_plus_without_a_tooltip() {
        let ctx = egui::Context::default();
        let long = vec![Tab::new(1, "A".repeat(400), false, true)];
        let mut out = TabStripResponse::default();
        for _ in 0..2 {
            ctx.run_ui(input(Vec::new()), |ui| out = TabStrip::new(&long).show_panel(ui)).textures_delta.clear();
        }
        let style = TabStyle::from_visuals(&egui::Visuals::light());
        assert!(out.tabs[0].1.width() <= style.max_tab_width + 0.5, "{:?}", out.tabs[0].1);
        assert_eq!(out.new_button, None);
    }

    #[test]
    fn no_documents_draws_an_empty_strip() {
        let ctx = egui::Context::default();
        let out = laid_out(&ctx, &[]);
        assert!(out.tabs.is_empty());
        assert_eq!(out.action, None);
    }
}
