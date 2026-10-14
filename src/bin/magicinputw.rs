// Same program without a console window, for starting at login on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    magicinput::main();
}
