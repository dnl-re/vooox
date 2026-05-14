use crossbeam_channel::Sender;
use ksni::blocking::{Handle, TrayMethods};
use ksni::{menu, MenuItem, Tray};

pub enum TrayCommand {
    OpenSettings,
    ShowPanel,
    Quit,
}

pub(crate) struct VoooxTray {
    tx: Sender<TrayCommand>,
    recording: bool,
}

impl Tray for VoooxTray {
    fn id(&self) -> String {
        "vooox".into()
    }

    fn title(&self) -> String {
        if self.recording { "vooox — aufnehmen".into() } else { "vooox".into() }
    }

    fn icon_name(&self) -> String {
        if self.recording {
            "audio-input-microphone-high-symbolic".into()
        } else {
            "audio-input-microphone-symbolic".into()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::Standard(menu::StandardItem {
                label: "Einstellungen".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(TrayCommand::OpenSettings);
                }),
                ..Default::default()
            }),
            MenuItem::Standard(menu::StandardItem {
                label: "Diktierfenster".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(TrayCommand::ShowPanel);
                }),
                ..Default::default()
            }),
            MenuItem::Separator,
            MenuItem::Standard(menu::StandardItem {
                label: "Beenden".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(TrayCommand::Quit);
                }),
                ..Default::default()
            }),
        ]
    }
}

pub fn spawn_tray(tx: Sender<TrayCommand>) -> Option<Handle<VoooxTray>> {
    let tray = VoooxTray { tx, recording: false };
    match tray.spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!(
                "[tray] Could not register system tray: {e}\n\
                 → Install 'gnome-shell-extension-appindicator' for GNOME tray support."
            );
            None
        }
    }
}

pub fn set_recording(handle: &Handle<VoooxTray>, recording: bool) {
    handle.update(|t| t.recording = recording);
}
