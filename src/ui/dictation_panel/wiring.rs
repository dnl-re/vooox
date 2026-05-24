use super::helpers::save_window_position;
use crate::storage::config::PanelMode;
use crate::storage::window_state::WindowState;
use crate::ui::tray::AppCommand;
use crossbeam_channel::Sender;
use glib;
use gtk4::prelude::*;
use gtk4::{gio, ApplicationWindow, Box as GtkBox, MenuButton};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use crate::storage::config::Config;

pub(super) fn wire_kebab_actions(
    window: &ApplicationWindow,
    cmd_tx: &Sender<AppCommand>,
    config: &Rc<RefCell<Config>>,
    initial_mode: PanelMode,
) -> gio::SimpleAction {
    let action_group = gio::SimpleActionGroup::new();

    for (name, cmd) in [
        ("history", AppCommand::OpenHistory),
        ("settings", AppCommand::OpenSettings),
        ("close", AppCommand::HidePanel),
        ("quit", AppCommand::Quit),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let tx = cmd_tx.clone();
        let cmd_clone = cmd.clone();
        action.connect_activate(move |_, _| { let _ = tx.send(cmd_clone.clone()); });
        action_group.add_action(&action);
    }

    let model_action = gio::SimpleAction::new_stateful(
        "model",
        Some(glib::VariantTy::STRING),
        &config.borrow().model.to_variant(),
    );
    let tx = cmd_tx.clone();
    model_action.connect_activate(move |action, param| {
        if let Some(s) = param.and_then(|p| p.get::<String>()) {
            action.set_state(&s.to_variant());
            let _ = tx.send(AppCommand::SetModel(s));
        }
    });
    action_group.add_action(&model_action);

    let mode_action = gio::SimpleAction::new_stateful(
        "mode",
        Some(glib::VariantTy::STRING),
        &initial_mode.as_str().to_variant(),
    );
    let tx = cmd_tx.clone();
    mode_action.connect_activate(move |action, param| {
        if let Some(s) = param.and_then(|p| p.get::<String>()) {
            if let Some(m) = PanelMode::from_str(&s) {
                action.set_state(&s.to_variant());
                let _ = tx.send(AppCommand::SetPanelMode(m));
            }
        }
    });
    action_group.add_action(&mode_action);

    window.insert_action_group("panel", Some(&action_group));
    mode_action
}

pub(super) fn wire_close_request(window: &ApplicationWindow, state: Rc<RefCell<WindowState>>) {
    window.connect_close_request(move |win| {
        save_window_position(win, &state);
        win.set_visible(false);
        glib::Propagation::Stop
    });
}

pub(super) fn wire_drag_gestures(
    window: &ApplicationWindow,
    header_box: &GtkBox,
    menu_btn: &MenuButton,
    pill_layout: &GtkBox,
) {
    use super::helpers::begin_move_from_gesture;

    let win = window.clone();
    let header = header_box.clone();
    let menu_btn_for_drag = menu_btn.clone();
    let drag = gtk4::GestureClick::new();
    drag.set_button(1);
    drag.connect_pressed(move |gesture, _n, x, y| {
        if click_landed_on(&header, &menu_btn_for_drag, x, y) {
            return;
        }
        begin_move_from_gesture(&win, gesture, x, y);
    });
    header_box.add_controller(drag);

    let win = window.clone();
    let drag = gtk4::GestureClick::new();
    drag.set_button(1);
    drag.connect_pressed(move |gesture, _n, x, y| {
        begin_move_from_gesture(&win, gesture, x, y);
    });
    pill_layout.add_controller(drag);
}

fn click_landed_on(header: &GtkBox, target: &MenuButton, x: f64, y: f64) -> bool {
    let Some(picked) = header.pick(x, y, gtk4::PickFlags::DEFAULT) else { return false };
    is_target_in_ancestry(picked, target, header)
}

fn is_target_in_ancestry(mut widget: gtk4::Widget, target: &MenuButton, stop_at: &GtkBox) -> bool {
    loop {
        if widget.eq(target) { return true; }
        match widget.parent() {
            Some(p) if p.eq(stop_at) => return false,
            Some(p) => widget = p,
            None => return false,
        }
    }
}
