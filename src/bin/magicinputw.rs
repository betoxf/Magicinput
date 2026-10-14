// The background service on Windows: no console window, and no settings window code.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    magicinput::main_with(magicinput::ui::launch);
}
