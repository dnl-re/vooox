use crate::storage::config::Config;
use crate::system::gpu;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, Label, Orientation, ScrolledWindow, Spinner, TextBuffer,
    TextView,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

pub(super) fn build_gpu_section(config: Rc<RefCell<Config>>) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 6);
    let (status_lbl, detail_lbl) = build_gpu_status_labels();
    let force_cpu_btn = build_force_cpu_checkbox(config.borrow().force_cpu);
    let (install_btn, spinner, action_row) = build_gpu_install_row();
    let (log_buffer, log_view, log_scroll) = build_gpu_log_view();

    let refresh_gpu_ui = {
        let status_lbl = status_lbl.clone();
        let detail_lbl = detail_lbl.clone();
        let install_btn = install_btn.clone();
        let force_cpu_btn = force_cpu_btn.clone();
        move || update_gpu_ui_for_hardware_state(&status_lbl, &detail_lbl, &install_btn, &force_cpu_btn)
    };
    refresh_gpu_ui();

    wire_force_cpu_toggle(&force_cpu_btn, Rc::clone(&config), refresh_gpu_ui.clone());
    wire_cuda_install_button(&install_btn, &spinner, &log_buffer, &log_view, &log_scroll, &detail_lbl, refresh_gpu_ui.clone());

    section.append(&build_gpu_heading());
    section.append(&status_lbl);
    section.append(&detail_lbl);
    section.append(&action_row);
    section.append(&log_scroll);
    section.append(&force_cpu_btn);
    section
}

fn build_gpu_heading() -> Label {
    let heading = Label::new(None);
    heading.set_markup("<b>GPU-Beschleunigung</b>");
    heading.set_xalign(0.0);
    heading
}

fn build_gpu_status_labels() -> (Label, Label) {
    let status_lbl = Label::new(None);
    status_lbl.set_xalign(0.0);
    status_lbl.set_wrap(true);
    status_lbl.set_use_markup(true);
    let detail_lbl = Label::new(None);
    detail_lbl.set_xalign(0.0);
    detail_lbl.set_wrap(true);
    (status_lbl, detail_lbl)
}

fn build_force_cpu_checkbox(currently_active: bool) -> CheckButton {
    let btn = CheckButton::with_label(
        "GPU deaktivieren und immer auf CPU rechnen (z. B. für Akkulaufzeit)",
    );
    btn.set_active(currently_active);
    btn
}

fn build_gpu_install_row() -> (Button, Spinner, GtkBox) {
    let install_btn = Button::with_label(&format!(
        "GPU-Unterstützung installieren ({})",
        gpu::estimated_download_label()
    ));
    let spinner = Spinner::new();
    spinner.set_visible(false);
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.append(&install_btn);
    row.append(&spinner);
    (install_btn, spinner, row)
}

fn build_gpu_log_view() -> (TextBuffer, TextView, ScrolledWindow) {
    let log_buffer = TextBuffer::new(None);
    let log_view = TextView::with_buffer(&log_buffer);
    log_view.set_editable(false);
    log_view.set_monospace(true);
    let log_scroll = ScrolledWindow::builder()
        .height_request(160)
        .child(&log_view)
        .build();
    log_scroll.set_visible(false);
    (log_buffer, log_view, log_scroll)
}

fn update_gpu_ui_for_hardware_state(
    status_lbl: &Label,
    detail_lbl: &Label,
    install_btn: &Button,
    force_cpu_btn: &CheckButton,
) {
    let hw = gpu::detect_hardware();
    let cuda_active = gpu::libs_active_in_venv();
    let cuda_wheels_present = gpu::wheels_installed();

    match (&hw, cuda_active, cuda_wheels_present) {
        (gpu::NvidiaHardware::None, _, _) => {
            status_lbl.set_markup("Verarbeitung: <b>CPU</b>");
            detail_lbl.set_text("Keine NVIDIA-GPU im System gefunden — vooox läuft auf der CPU.");
            install_btn.set_visible(false);
            force_cpu_btn.set_visible(false);
        }
        (gpu::NvidiaHardware::NoDriver, _, _) => {
            status_lbl.set_markup("Verarbeitung: <b>CPU</b> (Treiber fehlt)");
            detail_lbl.set_text(
                "NVIDIA-Karte erkannt, aber der proprietäre Treiber ist nicht installiert. \
                 Installiere ihn über deinen Paketmanager (z. B. `nvidia-driver-535`) \
                 und starte den Rechner neu.",
            );
            install_btn.set_visible(false);
            force_cpu_btn.set_visible(false);
        }
        (gpu::NvidiaHardware::DriverTooOld { driver }, _, _) => {
            status_lbl.set_markup(&format!("Verarbeitung: <b>CPU</b> (Treiber {driver} zu alt)"));
            detail_lbl.set_text(
                "Der NVIDIA-Treiber muss mindestens Version 525 sein, um CUDA 12 zu \
                 unterstützen. Aktualisiere ihn über deinen Paketmanager.",
            );
            install_btn.set_visible(false);
            force_cpu_btn.set_visible(false);
        }
        (gpu::NvidiaHardware::Ok { driver }, true, _) => {
            status_lbl.set_markup(&format!("Verarbeitung: <b>GPU (CUDA)</b> — Treiber {driver}"));
            detail_lbl.set_text(
                "GPU-Beschleunigung ist aktiv. Bei größeren Modellen (medium / large-v3) \
                 macht das einen spürbaren Unterschied.",
            );
            install_btn.set_visible(false);
            force_cpu_btn.set_visible(true);
        }
        (gpu::NvidiaHardware::Ok { driver }, false, true) => {
            status_lbl.set_markup(&format!(
                "Verarbeitung: <b>CPU</b> (Treiber {driver}, CUDA-Libraries installiert)"
            ));
            detail_lbl.set_text(
                "Die CUDA-Pakete sind installiert, vooox nutzt aber gerade die CPU. \
                 Falls die GPU-Deaktivierung aus ist: vooox neu starten — der Sidecar \
                 läuft noch mit der alten Konfiguration.",
            );
            install_btn.set_visible(false);
            force_cpu_btn.set_visible(true);
        }
        (gpu::NvidiaHardware::Ok { driver }, false, false) => {
            status_lbl.set_markup(&format!(
                "Verarbeitung: <b>CPU</b> — GPU verfügbar (Treiber {driver})"
            ));
            detail_lbl.set_text(
                "Du kannst die CUDA-Libraries jetzt nachinstallieren, um Transkriptionen \
                 deutlich zu beschleunigen. Nach der Installation vooox einmal neu starten.",
            );
            install_btn.set_visible(true);
            install_btn.set_sensitive(true);
            force_cpu_btn.set_visible(false);
        }
    }
}

fn wire_force_cpu_toggle(
    btn: &CheckButton,
    config: Rc<RefCell<Config>>,
    refresh: impl Fn() + 'static,
) {
    btn.connect_toggled(move |b| {
        config.borrow_mut().force_cpu = b.is_active();
        refresh();
    });
}

fn wire_cuda_install_button(
    install_btn: &Button,
    spinner: &Spinner,
    log_buffer: &TextBuffer,
    log_view: &TextView,
    log_scroll: &ScrolledWindow,
    detail_lbl: &Label,
    refresh: impl Fn() + Clone + 'static,
) {
    let spinner = spinner.clone();
    let log_buffer = log_buffer.clone();
    let log_view = log_view.clone();
    let log_scroll = log_scroll.clone();
    let detail_lbl = detail_lbl.clone();
    install_btn.connect_clicked(move |btn| {
        btn.set_sensitive(false);
        spinner.set_visible(true);
        spinner.start();
        log_scroll.set_visible(true);
        log_buffer.set_text("");
        detail_lbl.set_text("Lade CUDA- und cuDNN-Libraries herunter — kann ein paar Minuten dauern.");

        let (tx, rx) = mpsc::channel::<gpu::InstallMsg>();
        gpu::spawn_cuda_install_thread(tx);

        let spinner = spinner.clone();
        let log_buffer = log_buffer.clone();
        let log_view = log_view.clone();
        let detail_lbl = detail_lbl.clone();
        let refresh = refresh.clone();
        let btn = btn.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
            poll_cuda_install_progress(&rx, &btn, &spinner, &log_buffer, &log_view, &detail_lbl, &refresh)
        });
    });
}

fn poll_cuda_install_progress(
    rx: &mpsc::Receiver<gpu::InstallMsg>,
    btn: &Button,
    spinner: &Spinner,
    log_buffer: &TextBuffer,
    log_view: &TextView,
    detail_lbl: &Label,
    refresh: &impl Fn(),
) -> glib::ControlFlow {
    let mut had_error = false;
    while let Ok(msg) = rx.try_recv() {
        match msg {
            gpu::InstallMsg::Line(line) => append_gpu_log(log_buffer, log_view, &line),
            gpu::InstallMsg::Done => {
                return finish_cuda_install_successfully(spinner, log_buffer, log_view, detail_lbl, refresh);
            }
            gpu::InstallMsg::Error(e) => {
                append_gpu_log(log_buffer, log_view, &format!("\n✗ FEHLER: {e}"));
                had_error = true;
                break;
            }
        }
    }
    if had_error {
        spinner.stop();
        spinner.set_visible(false);
        btn.set_label("Erneut versuchen");
        btn.set_sensitive(true);
        detail_lbl.set_text("Installation fehlgeschlagen — Details siehe Log oben.");
        return glib::ControlFlow::Break;
    }
    glib::ControlFlow::Continue
}

fn finish_cuda_install_successfully(
    spinner: &Spinner,
    log_buffer: &TextBuffer,
    log_view: &TextView,
    detail_lbl: &Label,
    refresh: &impl Fn(),
) -> glib::ControlFlow {
    spinner.stop();
    spinner.set_visible(false);
    append_gpu_log(log_buffer, log_view, "");
    append_gpu_log(
        log_buffer,
        log_view,
        "✓ CUDA-Libraries installiert. vooox neu starten, damit der Sidecar die GPU benutzt.",
    );
    detail_lbl.set_text("Fertig — vooox einmal neu starten, dann läuft die Transkription auf der GPU.");
    refresh();
    glib::ControlFlow::Break
}

pub(super) fn append_gpu_log(buf: &TextBuffer, view: &TextView, line: &str) {
    let mut iter = buf.end_iter();
    if buf.char_count() > 0 {
        buf.insert(&mut iter, "\n");
    }
    let line_start = buf.create_mark(None, &iter, true);
    buf.insert(&mut iter, line);
    view.scroll_to_mark(&line_start, 0.0, true, 0.0, 1.0);
    buf.delete_mark(&line_start);
}
