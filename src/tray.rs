use crate::config::PanelMode;
use crossbeam_channel::Sender;
use ksni::blocking::{Handle, TrayMethods};
use ksni::{menu, MenuItem, Tray};

#[derive(Clone)]
pub enum AppCommand {
    OpenSettings,
    ShowPanel,
    OpenHistory,
    HidePanel,
    SetModel(String),
    SetPanelMode(PanelMode),
    Quit,
}

pub const WHISPER_MODELS: &[&str] = &[
    "tiny", "base", "small", "medium", "large-v2", "large-v3",
];

pub(crate) struct VoooxTray {
    tx: Sender<AppCommand>,
    recording: bool,
    panel_mode: PanelMode,
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
        let mode = self.panel_mode;
        vec![
            MenuItem::Standard(menu::StandardItem {
                label: "Einstellungen".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(AppCommand::OpenSettings);
                }),
                ..Default::default()
            }),
            MenuItem::Standard(menu::StandardItem {
                label: "Diktierfenster".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(AppCommand::ShowPanel);
                }),
                ..Default::default()
            }),
            MenuItem::SubMenu(menu::SubMenu {
                label: "Modus".into(),
                submenu: vec![
                    MenuItem::Checkmark(menu::CheckmarkItem {
                        label: "Diktierfenster".into(),
                        checked: mode == PanelMode::Window,
                        activate: Box::new(|t: &mut Self| {
                            let _ = t.tx.send(AppCommand::SetPanelMode(PanelMode::Window));
                        }),
                        ..Default::default()
                    }),
                    MenuItem::Checkmark(menu::CheckmarkItem {
                        label: "Nur Icon".into(),
                        checked: mode == PanelMode::Icon,
                        activate: Box::new(|t: &mut Self| {
                            let _ = t.tx.send(AppCommand::SetPanelMode(PanelMode::Icon));
                        }),
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            }),
            MenuItem::Separator,
            MenuItem::Standard(menu::StandardItem {
                label: "Beenden".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(AppCommand::Quit);
                }),
                ..Default::default()
            }),
        ]
    }
}

pub fn spawn_tray(tx: Sender<AppCommand>, initial_mode: PanelMode) -> Option<Handle<VoooxTray>> {
    let tray = VoooxTray { tx, recording: false, panel_mode: initial_mode };
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

pub fn set_panel_mode(handle: &Handle<VoooxTray>, mode: PanelMode) {
    handle.update(|t| t.panel_mode = mode);
}
