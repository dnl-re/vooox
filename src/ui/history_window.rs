use crate::storage::history::{History, HistoryEntry};
use crate::system::x11_window;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation,
    ScrolledWindow,
};
use std::cell::RefCell;
use std::rc::Rc;

pub fn open(app: &Application, history: Rc<RefCell<History>>) {
    let list = build_entry_list(&history);
    let scroll = ScrolledWindow::builder().vexpand(true).build();
    scroll.set_child(Some(&list));
    let window = build_history_window(app, &scroll);
    window.present();
    let w = window.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
        x11_window::center_window_on_cursor_monitor(&w);
    });
}

fn build_history_window(app: &Application, scroll: &ScrolledWindow) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("vooox — Verlauf")
        .default_width(540)
        .default_height(480)
        .build();
    window.set_child(Some(scroll));
    window
}

fn build_entry_list(history: &Rc<RefCell<History>>) -> ListBox {
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let entries: Vec<HistoryEntry> = history.borrow().entries().cloned().collect();
    if entries.is_empty() {
        let empty = Label::new(Some("Verlauf ist leer."));
        empty.set_margin_top(16);
        empty.set_margin_bottom(16);
        list.append(&empty);
    } else {
        for entry in entries.iter().rev() {
            list.append(&make_row(entry, &list, Rc::clone(history)));
        }
    }
    list
}

fn make_row(entry: &HistoryEntry, list: &ListBox, history: Rc<RefCell<History>>) -> ListBoxRow {
    let row = ListBoxRow::new();
    let hbox = build_row_layout_box();
    hbox.append(&build_timestamp_label(&entry.timestamp));
    hbox.append(&build_entry_text_label(&entry.text));
    hbox.append(&build_copy_button(&entry.text));
    hbox.append(&build_delete_button(&entry.timestamp, &row, list, history));
    row.set_child(Some(&hbox));
    row
}

fn build_row_layout_box() -> GtkBox {
    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);
    hbox
}

fn build_timestamp_label(timestamp: &str) -> Label {
    let display = timestamp.get(11..16).unwrap_or(timestamp).to_string();
    let lbl = Label::new(Some(&display));
    lbl.add_css_class("dim-label");
    lbl.set_valign(gtk4::Align::Start);
    lbl
}

fn build_entry_text_label(text: &str) -> Label {
    let lbl = Label::new(Some(text));
    lbl.set_hexpand(true);
    lbl.set_xalign(0.0);
    lbl.set_wrap(true);
    lbl.set_max_width_chars(60);
    lbl
}

fn build_copy_button(text: &str) -> Button {
    let btn = Button::with_label("📋");
    btn.set_valign(gtk4::Align::Start);
    let text = text.to_string();
    btn.connect_clicked(move |btn| {
        let Some(display) = gtk4::gdk::Display::default() else { return };
        display.clipboard().set_text(&text);
        btn.set_label("✓");
        let b = btn.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
            b.set_label("📋");
        });
    });
    btn
}

fn build_delete_button(
    timestamp: &str,
    row: &ListBoxRow,
    list: &ListBox,
    history: Rc<RefCell<History>>,
) -> Button {
    let btn = Button::with_label("🗑");
    btn.set_valign(gtk4::Align::Start);
    let timestamp = timestamp.to_string();
    let row_ref = row.clone();
    let list_ref = list.clone();
    btn.connect_clicked(move |_| {
        history.borrow_mut().remove_by_timestamp(&timestamp);
        list_ref.remove(&row_ref);
    });
    btn
}
