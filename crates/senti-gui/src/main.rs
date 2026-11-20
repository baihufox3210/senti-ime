//! SentiIME interactive demo.
//!
//! ```text
//! cargo run -p senti-gui --release
//! ```

mod app;
mod fonts;
mod keys;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SentiIME — 情境感知智慧輸入法 Demo")
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "SentiIME",
        options,
        Box::new(|cc| Ok(Box::new(app::SentiApp::new(cc)))),
    )
}
