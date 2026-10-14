fn main() {
    // Give the Windows .exe files the MagicInput icon and a manifest (modern controls, sharp on high-DPI screens).
    println!("cargo:rerun-if-changed=assets");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/magicinput.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
