use crate::audio;
use crate::storage::config::Config;
use glib;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, CheckButton, Label, LevelBar, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, ToggleButton,
};
use std::cell::RefCell;
use std::rc::Rc;

type MeterCell = Rc<RefCell<Option<audio::LevelMeter>>>;

pub(super) fn build_microphone_tab(window: &ApplicationWindow, config: Rc<RefCell<Config>>) -> GtkBox {
    let vbox = build_tab_root();
    let devices = audio::list_input_devices();
    if devices.is_empty() {
        vbox.append(&Label::new(Some("Keine Eingabegeräte gefunden.")));
        return vbox;
    }
    let effective = determine_effective_device(&devices, &config.borrow().microphone);
    set_config_default_if_unset(&config, &effective);
    let (list, scroll) = build_device_list_container();
    let active_meters: Rc<RefCell<Vec<MeterCell>>> = Rc::new(RefCell::new(Vec::new()));
    let mut group_leader: Option<CheckButton> = None;
    for dev_info in devices {
        let row = build_device_row(&dev_info, &effective, Rc::clone(&config), Rc::clone(&active_meters), &mut group_leader);
        list.append(&row);
    }
    wire_window_cleanup(window, Rc::clone(&active_meters));
    scroll.set_child(Some(&list));
    vbox.append(&scroll);
    vbox
}

fn build_tab_root() -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 6);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    vbox
}

fn build_device_list_container() -> (ListBox, ScrolledWindow) {
    let list = ListBox::new();
    let scroll = ScrolledWindow::builder().vexpand(true).build();
    (list, scroll)
}

fn determine_effective_device(devices: &[audio::DeviceInfo], configured: &Option<String>) -> Option<String> {
    configured.clone().or_else(|| {
        if devices.iter().any(|d| d.name == "pulse") {
            Some("pulse".into())
        } else {
            devices.first().map(|d| d.name.clone())
        }
    })
}

fn set_config_default_if_unset(config: &Rc<RefCell<Config>>, effective: &Option<String>) {
    if config.borrow().microphone.is_none() {
        if let Some(ref name) = effective {
            config.borrow_mut().microphone = Some(name.clone());
        }
    }
}

fn build_device_row(
    dev_info: &audio::DeviceInfo,
    effective: &Option<String>,
    config: Rc<RefCell<Config>>,
    active_meters: Rc<RefCell<Vec<MeterCell>>>,
    group_leader: &mut Option<CheckButton>,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    let hbox = build_device_row_box();
    let check = build_device_checkbox(&dev_info.name, effective, config, group_leader);
    let name_lbl = build_device_name_label(&dev_info.display);
    let level_bar = build_device_level_bar();
    let test_btn = ToggleButton::with_label("Pegel testen");
    let meter_cell: MeterCell = Rc::new(RefCell::new(None));
    active_meters.borrow_mut().push(Rc::clone(&meter_cell));
    wire_test_button(&test_btn, &dev_info.name, meter_cell, level_bar.clone());
    hbox.append(&check);
    hbox.append(&name_lbl);
    hbox.append(&test_btn);
    hbox.append(&level_bar);
    row.set_child(Some(&hbox));
    row
}

fn build_device_row_box() -> GtkBox {
    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);
    hbox
}

fn build_device_checkbox(
    name: &str,
    effective: &Option<String>,
    config: Rc<RefCell<Config>>,
    group_leader: &mut Option<CheckButton>,
) -> CheckButton {
    let check = CheckButton::new();
    if let Some(ref leader) = group_leader {
        check.set_group(Some(leader));
    } else {
        *group_leader = Some(check.clone());
    }
    check.set_active(effective.as_deref() == Some(name));
    let n = name.to_string();
    check.connect_toggled(move |btn| {
        if btn.is_active() { config.borrow_mut().microphone = Some(n.clone()); }
    });
    check
}

fn build_device_name_label(display: &str) -> Label {
    let lbl = Label::new(Some(display));
    lbl.set_hexpand(true);
    lbl.set_xalign(0.0);
    lbl
}

fn build_device_level_bar() -> LevelBar {
    let bar = LevelBar::new();
    bar.set_min_value(0.0);
    bar.set_max_value(1.0);
    bar.set_size_request(120, -1);
    bar
}

fn wire_test_button(btn: &ToggleButton, device_name: &str, meter_cell: MeterCell, level_bar: LevelBar) {
    let name = device_name.to_string();
    btn.connect_toggled(move |btn| {
        if btn.is_active() {
            start_level_meter_test(&name, Rc::clone(&meter_cell), level_bar.clone(), btn);
        } else {
            *meter_cell.borrow_mut() = None;
            level_bar.set_value(0.0);
        }
    });
}

fn start_level_meter_test(name: &str, meter_cell: MeterCell, level_bar: LevelBar, btn: &ToggleButton) {
    let Some(device) = audio::find_device_by_name(name) else {
        eprintln!("[settings] device gone: {name}");
        btn.set_active(false);
        return;
    };
    match audio::LevelMeter::start(&device) {
        Ok(meter) => { *meter_cell.borrow_mut() = Some(meter); poll_level_meter_updates(meter_cell, level_bar); }
        Err(e) => { eprintln!("[settings] level meter {name}: {e}"); btn.set_active(false); }
    }
}

fn poll_level_meter_updates(meter_cell: MeterCell, level_bar: LevelBar) {
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match meter_cell.borrow().as_ref() {
            Some(m) => { level_bar.set_value((m.get() as f64 * 8.0).min(1.0)); glib::ControlFlow::Continue }
            None => { level_bar.set_value(0.0); glib::ControlFlow::Break }
        }
    });
}

fn wire_window_cleanup(window: &ApplicationWindow, active_meters: Rc<RefCell<Vec<MeterCell>>>) {
    window.connect_close_request(move |_| {
        for cell in active_meters.borrow().iter() {
            *cell.borrow_mut() = None;
        }
        glib::Propagation::Proceed
    });
}
