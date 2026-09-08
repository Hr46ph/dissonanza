mod core_bridge;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    core_bridge::spawn(ui.as_weak());
    ui.run()
}
