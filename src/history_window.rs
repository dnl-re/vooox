use crate::history::History;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow,
};
use std::cell::RefCell;
use std::rc::Rc;

pub fn show_history_window(app: &Application, history: Rc<RefCell<History>>) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("vooox — Verlauf")
        .default_width(500)
        .default_height(400)
        .build();

    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);

    for entry in history.borrow().entries() {
        let row = ListBoxRow::new();
        let hbox = GtkBox::new(Orientation::Horizontal, 8);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);

        let text_lbl = Label::new(Some(&entry.text));
        text_lbl.set_hexpand(true);
        text_lbl.set_xalign(0.0);
        text_lbl.set_wrap(true);
        text_lbl.set_max_width_chars(60);

        let time_lbl = Label::new(Some(&entry.timestamp));
        time_lbl.set_opacity(0.6);

        let copy_btn = Button::with_label("Kopieren");
        {
            let text = entry.text.clone();
            copy_btn.connect_clicked(move |btn| {
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&text);
                    btn.set_label("✓");
                    let b = btn.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
                        b.set_label("Kopieren");
                    });
                }
            });
        }

        hbox.append(&text_lbl);
        hbox.append(&time_lbl);
        hbox.append(&copy_btn);
        row.set_child(Some(&hbox));
        list.append(&row);
    }

    if history.borrow().is_empty() {
        let r = ListBoxRow::new();
        r.set_child(Some(&Label::new(Some("Noch keine Einträge."))));
        list.append(&r);
    }

    let scroll = ScrolledWindow::builder().vexpand(true).build();
    scroll.set_child(Some(&list));

    let close_btn = Button::with_label("Schließen");
    {
        let win = window.clone();
        close_btn.connect_clicked(move |_| win.close());
    }

    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(8);
    vbox.set_margin_bottom(8);
    vbox.set_margin_start(8);
    vbox.set_margin_end(8);
    vbox.append(&scroll);
    vbox.append(&close_btn);
    window.set_child(Some(&vbox));
    window.present();
}
