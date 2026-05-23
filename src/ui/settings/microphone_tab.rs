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

pub(super) fn build_microphone_tab(window: &ApplicationWindow, config: Rc<RefCell<Config>>) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 6);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let devices = audio::list_input_devices();
    if devices.is_empty() {
        vbox.append(&Label::new(Some("Keine Eingabegeräte gefunden.")));
        return vbox;
    }

    let list = ListBox::new();
    let scroll = ScrolledWindow::builder().vexpand(true).build();

    let configured = config.borrow().microphone.clone();
    let effective = configured.clone().or_else(|| {
        if devices.iter().any(|d| d.name == "pulse") {
            Some("pulse".into())
        } else {
            devices.first().map(|d| d.name.clone())
        }
    });
    if configured.is_none() {
        if let Some(ref name) = effective {
            config.borrow_mut().microphone = Some(name.clone());
        }
    }

    type MeterCell = Rc<RefCell<Option<audio::LevelMeter>>>;
    let active_meters: Rc<RefCell<Vec<MeterCell>>> = Rc::new(RefCell::new(Vec::new()));

    let mut group_leader: Option<CheckButton> = None;

    for dev_info in devices {
        let name = dev_info.name.clone();
        let row = ListBoxRow::new();
        let hbox = GtkBox::new(Orientation::Horizontal, 8);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);

        let check = CheckButton::new();
        if let Some(ref leader) = group_leader {
            check.set_group(Some(leader));
        } else {
            group_leader = Some(check.clone());
        }
        check.set_active(effective.as_deref() == Some(&name));
        {
            let cfg = Rc::clone(&config);
            let n = name.clone();
            check.connect_toggled(move |btn| {
                if btn.is_active() {
                    cfg.borrow_mut().microphone = Some(n.clone());
                }
            });
        }

        let name_lbl = Label::new(Some(&dev_info.display));
        name_lbl.set_hexpand(true);
        name_lbl.set_xalign(0.0);

        let level_bar = LevelBar::new();
        level_bar.set_min_value(0.0);
        level_bar.set_max_value(1.0);
        level_bar.set_size_request(120, -1);

        let test_btn = ToggleButton::with_label("Pegel testen");
        let meter_cell: MeterCell = Rc::new(RefCell::new(None));
        active_meters.borrow_mut().push(Rc::clone(&meter_cell));
        {
            let n = name.clone();
            let meter_cell = Rc::clone(&meter_cell);
            let bar = level_bar.clone();
            test_btn.connect_toggled(move |btn| {
                if btn.is_active() {
                    let Some(device) = audio::find_device_by_name(&n) else {
                        eprintln!("[settings] device gone: {n}");
                        btn.set_active(false);
                        return;
                    };
                    match audio::LevelMeter::start(&device) {
                        Ok(meter) => {
                            *meter_cell.borrow_mut() = Some(meter);
                            let mc = Rc::clone(&meter_cell);
                            let b = bar.clone();
                            glib::timeout_add_local(
                                std::time::Duration::from_millis(50),
                                move || match mc.borrow().as_ref() {
                                    Some(m) => {
                                        b.set_value((m.get() as f64 * 8.0).min(1.0));
                                        glib::ControlFlow::Continue
                                    }
                                    None => {
                                        b.set_value(0.0);
                                        glib::ControlFlow::Break
                                    }
                                },
                            );
                        }
                        Err(e) => {
                            eprintln!("[settings] level meter {n}: {e}");
                            btn.set_active(false);
                        }
                    }
                } else {
                    *meter_cell.borrow_mut() = None;
                    bar.set_value(0.0);
                }
            });
        }

        hbox.append(&check);
        hbox.append(&name_lbl);
        hbox.append(&test_btn);
        hbox.append(&level_bar);
        row.set_child(Some(&hbox));
        list.append(&row);
    }

    {
        let am = Rc::clone(&active_meters);
        window.connect_close_request(move |_| {
            for cell in am.borrow().iter() {
                *cell.borrow_mut() = None;
            }
            glib::Propagation::Proceed
        });
    }

    scroll.set_child(Some(&list));
    vbox.append(&scroll);
    vbox
}
