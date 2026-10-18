// The background service on Windows: no console window, and no settings window code.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    // The Microsoft Store package's startup task starts this without arguments.
    #[cfg(windows)]
    if std::env::args().len() == 1 && magicinput::packaged() {
        return magicinput::run_service();
    }
    magicinput::main_with(magicinput::ui::launch);
}
