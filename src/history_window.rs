use crate::history::{History, HistoryEntry};
use crate::x11_window;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation,
    ScrolledWindow,
};
use std::cell::RefCell;
use std::rc::Rc;

pub fn open(app: &Application, history: Rc<RefCell<History>>) {
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
            list.append(&make_row(entry, &list, Rc::clone(&history)));
        }
    }

    let scroll = ScrolledWindow::builder().vexpand(true).build();
    scroll.set_child(Some(&list));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("vooox — Verlauf")
        .default_width(540)
        .default_height(480)
        .build();
    window.set_child(Some(&scroll));
    window.present();
    let w = window.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
        x11_window::center_window_on_cursor_monitor(&w);
    });
}

fn make_row(entry: &HistoryEntry, list: &ListBox, history: Rc<RefCell<History>>) -> ListBoxRow {
    let row = ListBoxRow::new();
    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);

    let ts_display = entry
        .timestamp
        .get(11..16)
        .unwrap_or(&entry.timestamp)
        .to_string();
    let time_lbl = Label::new(Some(&ts_display));
    time_lbl.add_css_class("dim-label");
    time_lbl.set_valign(gtk4::Align::Start);

    let text_lbl = Label::new(Some(&entry.text));
    text_lbl.set_hexpand(true);
    text_lbl.set_xalign(0.0);
    text_lbl.set_wrap(true);
    text_lbl.set_max_width_chars(60);

    let copy_btn = Button::with_label("📋");
    copy_btn.set_valign(gtk4::Align::Start);
    {
        let text = entry.text.clone();
        copy_btn.connect_clicked(move |btn| {
            if let Some(display) = gtk4::gdk::Display::default() {
                display.clipboard().set_text(&text);
                btn.set_label("✓");
                let b = btn.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
                    b.set_label("📋");
                });
            }
        });
    }

    let del_btn = Button::with_label("🗑");
    del_btn.set_valign(gtk4::Align::Start);
    {
        let timestamp = entry.timestamp.clone();
        let row_ref = row.clone();
        let list_ref = list.clone();
        del_btn.connect_clicked(move |_| {
            history.borrow_mut().remove_by_timestamp(&timestamp);
            list_ref.remove(&row_ref);
        });
    }

    hbox.append(&time_lbl);
    hbox.append(&text_lbl);
    hbox.append(&copy_btn);
    hbox.append(&del_btn);
    row.set_child(Some(&hbox));
    row
}
