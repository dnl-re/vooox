use super::{PillPhase, PILL_H, PILL_W, WAVE_H, WAVE_W, WIN_H, WIN_W, CSS};
use gtk4::cairo;
use crate::storage::config::PanelMode;
use crate::ui::tray::WHISPER_MODELS;
use gtk4::prelude::*;
use gtk4::{
    gio, Application, ApplicationWindow, Box as GtkBox, CssProvider, DrawingArea, Label, LevelBar,
    MenuButton, Orientation, ScrolledWindow, Separator, TextView,
};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

pub(super) fn install_css() {
    let provider = CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub(super) fn build_status_label() -> Label {
    let lbl = Label::new(Some("○ Bereit"));
    lbl.add_css_class("status-idle");
    lbl
}

pub(super) fn build_timer_label() -> Label {
    let lbl = Label::new(Some(""));
    lbl.set_hexpand(true);
    lbl.set_xalign(1.0);
    lbl
}

pub(super) fn build_level_bar() -> LevelBar {
    let bar = LevelBar::new();
    bar.set_min_value(0.0);
    bar.set_max_value(1.0);
    bar.set_size_request(100, -1);
    bar.set_valign(gtk4::Align::Center);
    bar
}

pub(super) fn build_menu_button() -> MenuButton {
    let menu_model = build_menu_model();
    let btn = MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&menu_model)
        .valign(gtk4::Align::Center)
        .build();
    btn.add_css_class("flat");
    btn
}

fn build_menu_model() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Verlauf"), Some("panel.history"));
    menu.append(Some("Einstellungen"), Some("panel.settings"));

    let models = gio::Menu::new();
    for m in WHISPER_MODELS {
        let item = gio::MenuItem::new(Some(m), None);
        item.set_action_and_target_value(Some("panel.model"), Some(&m.to_variant()));
        models.append_item(&item);
    }
    menu.append_section(Some("Modell"), &models);

    let modes = gio::Menu::new();
    for (label, value) in [("Diktierfenster", "window"), ("Nur Icon", "icon")] {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some("panel.mode"), Some(&value.to_variant()));
        modes.append_item(&item);
    }
    menu.append_section(Some("Modus"), &modes);

    let actions = gio::Menu::new();
    actions.append(Some("Fenster schließen"), Some("panel.close"));
    actions.append(Some("App beenden"), Some("panel.quit"));
    menu.append_section(None, &actions);

    menu
}

pub(super) fn build_header_box(
    status: &Label,
    timer: &Label,
    level: &LevelBar,
    menu_btn: &MenuButton,
) -> GtkBox {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.set_margin_top(8);
    header.set_margin_bottom(8);
    header.set_margin_start(12);
    header.set_margin_end(12);
    header.append(status);
    header.append(timer);
    header.append(level);
    header.append(menu_btn);
    header
}

pub(super) fn build_text_view() -> TextView {
    let tv = TextView::new();
    tv.set_editable(true);
    tv.set_wrap_mode(gtk4::WrapMode::WordChar);
    tv.set_left_margin(12);
    tv.set_right_margin(12);
    tv.set_top_margin(8);
    tv.set_bottom_margin(8);
    tv
}

pub(super) fn build_toast_label() -> Label {
    let lbl = Label::new(None);
    lbl.add_css_class("toast");
    lbl.set_hexpand(true);
    lbl.set_xalign(0.5);
    lbl.set_margin_top(4);
    lbl.set_margin_bottom(4);
    lbl
}

pub(super) fn build_window_layout(header: &GtkBox, scroll: &ScrolledWindow, toast: &Label) -> GtkBox {
    let layout = GtkBox::new(Orientation::Vertical, 0);
    layout.add_css_class("background");
    layout.add_css_class("panel-root");
    layout.append(header);
    layout.append(&Separator::new(Orientation::Horizontal));
    layout.append(scroll);
    layout.append(toast);
    layout
}

pub(super) fn build_pill_dot() -> Label {
    let dot = Label::new(Some("●"));
    dot.add_css_class("pill-dot");
    dot.set_valign(gtk4::Align::Center);
    dot
}

pub(super) fn build_pill_timer() -> Label {
    let lbl = Label::new(Some("00:00"));
    lbl.add_css_class("pill-timer");
    lbl.set_valign(gtk4::Align::Center);
    lbl
}

pub(super) fn build_pill_layout(dot: &Label, waveform: &DrawingArea, timer: &Label) -> GtkBox {
    let layout = GtkBox::new(Orientation::Horizontal, 10);
    layout.add_css_class("panel-pill");
    layout.set_valign(gtk4::Align::Center);
    layout.set_halign(gtk4::Align::Center);
    layout.append(dot);
    layout.append(waveform);
    layout.append(timer);
    layout
}

pub(super) fn build_waveform_area(
    history: Rc<RefCell<VecDeque<f32>>>,
    phase: Rc<Cell<PillPhase>>,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(WAVE_W);
    area.set_content_height(WAVE_H);
    area.set_size_request(WAVE_W, WAVE_H);
    area.set_valign(gtk4::Align::Center);
    area.set_draw_func(move |_, cr, w, h| {
        draw_waveform_bars(cr, w, h, &history.borrow(), phase.get());
    });
    area
}

fn draw_waveform_bars(cr: &cairo::Context, w: i32, h: i32, hist: &VecDeque<f32>, phase: PillPhase) {
    let n = hist.len().max(1);
    let bar_w = (w as f64 / n as f64) * 0.55;
    let gap = (w as f64 / n as f64) - bar_w;
    let center_y = h as f64 / 2.0;
    set_waveform_color_for_phase(cr, phase);
    for (i, &lvl) in hist.iter().enumerate() {
        draw_bar_at(cr, i, lvl, bar_w, gap, center_y, h);
    }
}

fn set_waveform_color_for_phase(cr: &cairo::Context, phase: PillPhase) {
    match phase {
        PillPhase::Recording    => cr.set_source_rgba(1.0,   0.32,  0.32,  0.95),
        PillPhase::RecordingPtt => cr.set_source_rgba(0.788, 0.235, 1.0,   0.95),
        PillPhase::Processing   => cr.set_source_rgba(1.0,   0.68,  0.18,  0.95),
        PillPhase::Done         => cr.set_source_rgba(0.15,  0.75,  0.40,  0.95),
    }
}

fn draw_bar_at(cr: &cairo::Context, i: usize, lvl: f32, bar_w: f64, gap: f64, center_y: f64, h: i32) {
    let l = (lvl as f64).clamp(0.0, 1.0);
    let bar_h = ((l.sqrt() * 2.2).min(1.0) * h as f64 * 0.9).max(2.0);
    let x = i as f64 * (bar_w + gap) + gap * 0.5;
    let y = center_y - bar_h / 2.0;
    let r = (bar_w / 2.0).min(bar_h / 2.0);
    draw_rounded_rect(cr, x, y, bar_w, bar_h, r);
    let _ = cr.fill();
}

fn draw_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.move_to(x + r, y);
    cr.line_to(x + w - r, y);
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.line_to(x + w, y + h - r);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.line_to(x + r, y + h);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.line_to(x, y + r);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, std::f64::consts::PI * 1.5);
    cr.close_path();
}

pub(super) fn build_window(app: &Application, mode: PanelMode, child: &GtkBox) -> ApplicationWindow {
    let (w, h) = if mode == PanelMode::Icon { (PILL_W, PILL_H) } else { (WIN_W, WIN_H) };
    let win = ApplicationWindow::builder()
        .application(app)
        .title("vooox")
        .default_width(w)
        .default_height(h)
        .decorated(false)
        .build();
    win.add_css_class("dictation-window");
    win.set_child(Some(child));
    win
}

pub(super) fn set_initial_layout_visibility(mode: PanelMode, window_layout: &GtkBox, pill_layout: &GtkBox) {
    let (window_visible, pill_visible) = match mode {
        PanelMode::Window => (true, false),
        PanelMode::Icon => (false, true),
    };
    window_layout.set_visible(window_visible);
    pill_layout.set_visible(pill_visible);
}
