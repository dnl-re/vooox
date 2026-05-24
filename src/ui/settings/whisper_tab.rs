use crate::storage::config::Config;
use crate::transcription::whisper_client::WhisperClient;
use crate::transcription::whisper_models;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, ComboBoxText, Label, Orientation, Separator, Spinner,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use super::gpu_section::build_gpu_section;

pub(super) fn build_whisper_tab(config: Rc<RefCell<Config>>, whisper_port: u16) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 10);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let model_lbl = Label::new(Some("Modell:"));
    model_lbl.set_xalign(0.0);
    let model_combo = ComboBoxText::new();
    for m in whisper_models::MODELS {
        model_combo.append(Some(m.id), m.id);
    }
    model_combo.set_active_id(Some(&config.borrow().model));

    let status_lbl = Label::new(None);
    status_lbl.set_xalign(0.0);
    status_lbl.set_wrap(true);

    let size_lbl = Label::new(None);
    size_lbl.set_xalign(0.0);

    let download_btn = Button::with_label("Modell herunterladen");
    let delete_btn = Button::with_label("Modell löschen");
    let spinner = Spinner::new();
    spinner.set_visible(false);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    action_row.append(&download_btn);
    action_row.append(&delete_btn);
    action_row.append(&spinner);

    let refresh_state = {
        let status_lbl = status_lbl.clone();
        let size_lbl = size_lbl.clone();
        let download_btn = download_btn.clone();
        let delete_btn = delete_btn.clone();
        let model_combo = model_combo.clone();
        move || {
            let Some(id) = model_combo.active_id() else { return };
            let id = id.to_string();
            size_lbl.set_text(&format!("Größe: {}", whisper_models::size_label(&id)));
            if whisper_models::is_downloaded(&id) {
                status_lbl.set_markup("Status: <b>✓ heruntergeladen</b>");
                download_btn.set_sensitive(false);
                delete_btn.set_sensitive(true);
            } else {
                status_lbl.set_markup("Status: <b>✗ nicht vorhanden</b>");
                download_btn.set_sensitive(true);
                delete_btn.set_sensitive(false);
            }
        }
    };
    refresh_state();

    {
        let cfg = Rc::clone(&config);
        let refresh_state = refresh_state.clone();
        model_combo.connect_changed(move |cb| {
            if let Some(m) = cb.active_id() {
                cfg.borrow_mut().model = m.to_string();
                refresh_state();
            }
        });
    }

    wire_model_download_button(
        &download_btn,
        &delete_btn,
        &spinner,
        &status_lbl,
        &model_combo,
        whisper_port,
        refresh_state.clone(),
    );
    wire_model_delete_button(&delete_btn, &model_combo, &status_lbl, refresh_state.clone());

    let lang_lbl = Label::new(Some("Sprache:"));
    lang_lbl.set_xalign(0.0);
    let lang_combo = ComboBoxText::new();
    for (id, label) in &[
        ("auto", "Automatisch erkennen"),
        ("de", "Deutsch"),
        ("en", "Englisch"),
        ("fr", "Französisch"),
        ("es", "Spanisch"),
        ("it", "Italienisch"),
        ("pt", "Portugiesisch"),
        ("nl", "Niederländisch"),
        ("pl", "Polnisch"),
        ("ru", "Russisch"),
        ("zh", "Chinesisch"),
        ("ja", "Japanisch"),
    ] {
        lang_combo.append(Some(id), label);
    }
    lang_combo.set_active_id(Some(&config.borrow().language));
    {
        let cfg = Rc::clone(&config);
        lang_combo.connect_changed(move |cb| {
            if let Some(l) = cb.active_id() {
                cfg.borrow_mut().language = l.to_string();
            }
        });
    }

    vbox.append(&model_lbl);
    vbox.append(&model_combo);
    vbox.append(&status_lbl);
    vbox.append(&size_lbl);
    vbox.append(&action_row);
    vbox.append(&Separator::new(Orientation::Horizontal));
    vbox.append(&build_gpu_section(Rc::clone(&config)));
    vbox.append(&Separator::new(Orientation::Horizontal));
    vbox.append(&lang_lbl);
    vbox.append(&lang_combo);
    vbox
}

fn wire_model_download_button(
    download_btn: &Button,
    delete_btn: &Button,
    spinner: &Spinner,
    status_lbl: &Label,
    model_combo: &ComboBoxText,
    whisper_port: u16,
    refresh_state: impl Fn() + Clone + 'static,
) {
    let (spinner, delete_btn, status_lbl, model_combo) =
        (spinner.clone(), delete_btn.clone(), status_lbl.clone(), model_combo.clone());
    download_btn.connect_clicked(move |btn| {
        let Some(id) = model_combo.active_id() else { return };
        let id = id.to_string();
        show_download_in_progress(btn, &delete_btn, &spinner, &status_lbl, &id);
        let rx = start_model_download_thread(&id, whisper_port);
        poll_download_result_until_done(rx, btn.clone(), spinner.clone(), status_lbl.clone(), refresh_state.clone());
    });
}

fn show_download_in_progress(btn: &Button, delete_btn: &Button, spinner: &Spinner, status_lbl: &Label, model_id: &str) {
    btn.set_sensitive(false);
    delete_btn.set_sensitive(false);
    spinner.set_visible(true);
    spinner.start();
    status_lbl.set_text(&format!("Lade {model_id} herunter — kann ein paar Minuten dauern…"));
}

fn start_model_download_thread(model_id: &str, whisper_port: u16) -> mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let id = model_id.to_string();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => { let _ = tx.send(Err(format!("Tokio-Runtime: {e}"))); return; }
        };
        let _ = tx.send(rt.block_on(async { WhisperClient::new(whisper_port).ensure_model(&id).await }));
    });
    rx
}

fn poll_download_result_until_done(
    rx: mpsc::Receiver<Result<(), String>>,
    btn: Button,
    spinner: Spinner,
    status_lbl: Label,
    refresh_state: impl Fn() + 'static,
) {
    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        match rx.try_recv() {
            Ok(result) => finish_download_and_refresh(&btn, &spinner, &status_lbl, result, &refresh_state),
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                finish_download_and_refresh(&btn, &spinner, &status_lbl, Err("Download-Thread abgebrochen.".into()), &refresh_state)
            }
        }
    });
}

fn finish_download_and_refresh(btn: &Button, spinner: &Spinner, status_lbl: &Label, result: Result<(), String>, refresh: &impl Fn()) -> glib::ControlFlow {
    spinner.stop();
    spinner.set_visible(false);
    btn.set_sensitive(true);
    match result {
        Ok(()) => status_lbl.set_markup("<b>✓ Download abgeschlossen.</b>"),
        Err(e) => status_lbl.set_text(&format!("✗ {e}")),
    }
    refresh();
    glib::ControlFlow::Break
}

fn wire_model_delete_button(
    delete_btn: &Button,
    model_combo: &ComboBoxText,
    status_lbl: &Label,
    refresh_state: impl Fn() + 'static,
) {
    let model_combo = model_combo.clone();
    let status_lbl = status_lbl.clone();
    delete_btn.connect_clicked(move |_| {
        let Some(id) = model_combo.active_id() else { return };
        match whisper_models::delete_cache(&id) {
            Ok(()) => status_lbl.set_markup("<b>✓ Modell gelöscht.</b>"),
            Err(e) => status_lbl.set_text(&format!("✗ Löschen fehlgeschlagen: {e}")),
        }
        refresh_state();
    });
}
