mod general_tab;
mod gpu_section;
mod microphone_tab;
mod shortcut_tab;
mod whisper_tab;

use self::general_tab::build_general_tab;
use self::microphone_tab::build_microphone_tab;
use self::shortcut_tab::build_shortcut_tab;
use self::whisper_tab::build_whisper_tab;

use crate::storage::config::Config;
use crate::system::x11_window;
use glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, Button, Label, Notebook, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

pub struct SettingsWindow {
    window: ApplicationWindow,
}

impl SettingsWindow {
    pub fn new(app: &Application, config: Rc<RefCell<Config>>, whisper_port: u16) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("vooox — Einstellungen")
            .default_width(840)
            .default_height(620)
            .build();

        let notebook = Notebook::new();
        notebook.append_page(
            &build_general_tab(Rc::clone(&config)),
            Some(&Label::new(Some("Allgemein"))),
        );
        notebook.append_page(
            &build_microphone_tab(&window, Rc::clone(&config)),
            Some(&Label::new(Some("Mikrofon"))),
        );
        notebook.append_page(
            &build_whisper_tab(Rc::clone(&config), whisper_port),
            Some(&Label::new(Some("Whisper"))),
        );
        notebook.append_page(
            &build_shortcut_tab(Rc::clone(&config)),
            Some(&Label::new(Some("Tastenkürzel"))),
        );

        let save_btn = Button::with_label("Speichern & schließen");
        {
            let config = Rc::clone(&config);
            let win = window.clone();
            save_btn.connect_clicked(move |_| {
                if let Err(e) = config.borrow().save() {
                    eprintln!("[settings] save error: {e}");
                }
                win.close();
            });
        }

        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(8);
        vbox.set_margin_bottom(8);
        vbox.set_margin_start(8);
        vbox.set_margin_end(8);
        vbox.append(&notebook);
        vbox.append(&save_btn);
        window.set_child(Some(&vbox));

        SettingsWindow { window }
    }

    pub fn show(&self) {
        self.window.present();
        let win = self.window.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            x11_window::center_window_on_cursor_monitor(&win);
        });
    }
}
