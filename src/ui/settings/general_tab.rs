use crate::storage::config::Config;
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Box as GtkBox, CheckButton, Label, Orientation, Separator, SpinButton,
};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn build_general_tab(config: Rc<RefCell<Config>>) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 10);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let autostart_btn = CheckButton::with_label("Automatisch beim Login starten");
    autostart_btn.set_active(config.borrow().autostart);
    {
        let cfg = Rc::clone(&config);
        autostart_btn.connect_toggled(move |btn| {
            cfg.borrow_mut().autostart = btn.is_active();
        });
    }
    vbox.append(&autostart_btn);

    vbox.append(&Separator::new(Orientation::Horizontal));

    let ptt_btn = CheckButton::with_label("Push-to-Talk aktivieren");
    ptt_btn.set_active(config.borrow().push_to_talk_enabled);

    let ptt_desc = Label::new(Some(
        "Beim langen Halten des Shortcuts (länger als die unten eingestellte Schwelle) \
         wechselt vooox in den Push-to-Talk-Modus: Sobald du den Shortcut loslässt, \
         endet die Aufnahme sofort. Ein kurzer Druck schaltet wie gewohnt um \
         (Aufnahme starten/stoppen). Während Push-to-Talk aktiv ist, leuchtet die \
         Statusanzeige lila.",
    ));
    ptt_desc.set_xalign(0.0);
    ptt_desc.set_wrap(true);

    let threshold_lbl = Label::new(Some("Schwelle (ms):"));
    threshold_lbl.set_xalign(0.0);

    let adj = Adjustment::new(
        config.borrow().push_to_talk_threshold_ms as f64,
        100.0, 3000.0, 50.0, 100.0, 0.0,
    );
    let threshold_spin = SpinButton::new(Some(&adj), 1.0, 0);

    let threshold_row = GtkBox::new(Orientation::Horizontal, 8);
    threshold_row.append(&threshold_lbl);
    threshold_row.append(&threshold_spin);

    {
        let cfg = Rc::clone(&config);
        let desc = ptt_desc.clone();
        let row = threshold_row.clone();
        ptt_btn.connect_toggled(move |btn| {
            let enabled = btn.is_active();
            cfg.borrow_mut().push_to_talk_enabled = enabled;
            desc.set_sensitive(enabled);
            row.set_sensitive(enabled);
        });
    }
    {
        let cfg = Rc::clone(&config);
        threshold_spin.connect_value_changed(move |sb| {
            cfg.borrow_mut().push_to_talk_threshold_ms = sb.value() as u32;
        });
    }

    let initial = config.borrow().push_to_talk_enabled;
    ptt_desc.set_sensitive(initial);
    threshold_row.set_sensitive(initial);

    vbox.append(&ptt_btn);
    vbox.append(&ptt_desc);
    vbox.append(&threshold_row);

    vbox.append(&Separator::new(Orientation::Horizontal));

    let paste_lbl = Label::new(Some("Automatisches Einfügen"));
    paste_lbl.set_xalign(0.0);
    paste_lbl.add_css_class("heading");

    let paste_desc = Label::new(Some(
        "Nach der Transkription wird der Text per simuliertem Strg+V direkt \
         im zuvor fokussierten Fenster eingefügt (benötigt xdotool, X11). \
         Du kannst es pro Aufnahme-Modus separat aktivieren.",
    ));
    paste_desc.set_xalign(0.0);
    paste_desc.set_wrap(true);

    let paste_toggle_btn =
        CheckButton::with_label("Im Toggle-Modus (kurzer Druck) automatisch einfügen");
    paste_toggle_btn.set_active(config.borrow().auto_paste_toggle);
    {
        let cfg = Rc::clone(&config);
        paste_toggle_btn.connect_toggled(move |btn| {
            cfg.borrow_mut().auto_paste_toggle = btn.is_active();
        });
    }

    let paste_ptt_btn =
        CheckButton::with_label("Im Push-to-Talk-Modus automatisch einfügen");
    paste_ptt_btn.set_active(config.borrow().auto_paste_ptt);
    {
        let cfg = Rc::clone(&config);
        paste_ptt_btn.connect_toggled(move |btn| {
            cfg.borrow_mut().auto_paste_ptt = btn.is_active();
        });
    }

    vbox.append(&paste_lbl);
    vbox.append(&paste_desc);
    vbox.append(&paste_toggle_btn);
    vbox.append(&paste_ptt_btn);

    vbox
}
