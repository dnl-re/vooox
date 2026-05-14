use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Label};

const CSS: &str = r#"
.overlay-box {
    background-color: rgba(30, 30, 30, 0.85);
    border-radius: 12px;
    padding: 8px 14px;
}
.overlay-label {
    color: #ff4444;
    font-size: 18px;
    font-weight: bold;
}
@keyframes pulse {
    0%   { opacity: 1.0; }
    50%  { opacity: 0.35; }
    100% { opacity: 1.0; }
}
.recording {
    animation: pulse 1.2s ease-in-out infinite;
}
"#;

pub struct OverlayWindow {
    window: ApplicationWindow,
    label: Label,
}

impl OverlayWindow {
    pub fn new(app: &Application) -> Self {
        let provider = CssProvider::new();
        provider.load_from_data(CSS);
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let window = ApplicationWindow::builder()
            .application(app)
            .title("vooox-overlay")
            .decorated(false)
            .resizable(false)
            .build();

        // prevent focus: GTK4 way
        window.set_focusable(false);
        window.set_can_focus(false);

        let label = Label::new(Some("🎙 Zuhören…"));
        label.add_css_class("overlay-label");

        let bx = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        bx.add_css_class("overlay-box");
        bx.append(&label);
        window.set_child(Some(&bx));

        OverlayWindow { window, label }
    }

    pub fn show_recording(&self) {
        self.label.set_text("🎙 Zuhören…");
        self.label.add_css_class("recording");
        self.window.present();
    }

    pub fn show_processing(&self) {
        self.label.set_text("⚙ Verarbeite…");
        self.label.remove_css_class("recording");
        self.window.present();
    }

    pub fn hide(&self) {
        self.label.remove_css_class("recording");
        self.window.hide();
    }
}
