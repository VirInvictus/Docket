use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};

const APP_ID: &str = "io.github.virinvictus.Docket";

fn main() -> gtk4::glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Docket")
        .default_width(720)
        .default_height(960)
        .child(&sourceview5::View::new())
        .build();
    window.present();
}
