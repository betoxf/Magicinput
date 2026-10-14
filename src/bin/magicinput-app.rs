// The settings window and tray icon on Windows (magicinput-app.exe), opened from the Start menu.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    magicinput::main_with(magicinput::ui::run);
}
