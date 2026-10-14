fn main() {
    #[cfg(target_os = "macos")]
    magicinput::main_with(magicinput::ui::run);
    #[cfg(not(target_os = "macos"))]
    magicinput::main_with(magicinput::ui::launch);
}
