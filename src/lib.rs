//! Shared entry point for `magicinput` (command line) and `magicinputw` (Windows background, no window).

mod config;
mod core;
mod geom;
mod keymap;
mod net;
mod platform;
mod proto;
mod service;
mod status;
pub mod ui;

use std::io::IsTerminal;

use clap::{Parser, Subcommand};

use config::Config;
use proto::Side;

/// Use one keyboard and mouse across your Mac and PC: push the pointer off the edge of one
/// screen and it continues on the other.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the settings window (the default)
    Settings {
        /// Only show the menu bar / tray icon (used at login when the icon is turned on)
        #[arg(long, hide = true)]
        background: bool,
    },
    /// Run the background service that shares keyboard and mouse (started at login)
    Run,
    /// Pair computers. Without a code, makes a new one. With a code, uses the one shown on the other computer.
    Pair { code: Option<String> },
    /// Where the other computer's screen is: auto (default), left, right, top or bottom
    Side { side: String },
    /// Connect to a fixed address when automatic discovery doesn't work (use "auto" to clear)
    Peer { addr: String },
    /// Show settings and where they are stored
    Config,
    /// Start magicinput automatically when you log in (and start it now)
    Install,
    /// Stop starting magicinput at login
    Uninstall,
}

/// `settings` opens the settings window. Each program passes how: the window itself (Mac, and
/// `magicinput-app.exe` on Windows), or launching it. Keeping the window code out of the Windows
/// background service means it never loads the web engine.
pub fn main_with(settings: fn(bool)) {
    let cli = Cli::parse();
    let mut cfg = Config::load();
    match cli.cmd.unwrap_or(Cmd::Settings { background: false }) {
        Cmd::Settings { background } => settings(background),
        Cmd::Run => run(cfg),
        Cmd::Pair { code } => {
            match code {
                Some(c) => {
                    let normalized = config::normalize_code(&c);
                    if normalized.len() != 8 {
                        exit(&format!("\"{c}\" doesn't look like a pairing code (expected 8 characters, like K7QP-3MZD)"));
                    }
                    cfg.code = format!("{}-{}", &normalized[..4], &normalized[4..]);
                    cfg.save();
                    println!("Paired with code {}.", cfg.code);
                }
                None => {
                    cfg.code = config::new_code();
                    cfg.save();
                    print_pairing(&cfg);
                }
            }
        }
        Cmd::Side { side } => {
            if side.eq_ignore_ascii_case("auto") {
                cfg.peer_side = None;
                cfg.save();
                println!("Saved: auto. Push firmly past the left or right edge to switch computers.");
                    return;
            }
            let Some(s) = Side::parse(&side) else { exit("Side must be auto, left, right, top or bottom") };
            cfg.peer_side = Some(s);
            cfg.save();
            println!(
                "Saved: the other computer is to the {} of this one. The other computer picks this up automatically.",
                format!("{s:?}").to_lowercase()
            );
        }
        Cmd::Peer { addr } => {
            cfg.peer_addr = if addr == "auto" { None } else { Some(addr) };
            cfg.save();
            println!("Saved peer address: {}", cfg.peer_addr.as_deref().unwrap_or("auto-discover"));
        }
        Cmd::Config => {
            println!("{}\n", Config::path().display());
            print!("{}", toml::to_string_pretty(&cfg).unwrap());
        }
        Cmd::Install => match service::install() {
            Ok(m) => println!("{m}"),
            Err(e) => exit(&e),
        },
        Cmd::Uninstall => match service::uninstall() {
            Ok(m) => println!("{m}"),
            Err(e) => exit(&e),
        },
    }
}

fn run(mut cfg: Config) {
    let mut logger = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    logger.format_target(false);
    // Started from Finder/Explorer: no terminal to print to, so log next to the config.
    if !std::io::stderr().is_terminal() {
        let path = Config::path().with_file_name("magicinput.log");
        if let Ok(f) = std::fs::File::create(&path) {
            logger.target(env_logger::Target::Pipe(Box::new(f)));
        }
    } else {
        logger.format_timestamp(None);
    }
    logger.init();

    if cfg.code.is_empty() {
        cfg.code = config::new_code();
        cfg.save();
        print_pairing(&cfg);
    }
    // Never leave the cursor hidden or frozen, even if something goes badly wrong.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("crashed: {info}");
        platform::cleanup();
        default_hook(info);
    }));

    if let Err(e) = platform::init() {
        exit(&e);
    }

    let (tx, rx) = crossbeam_channel::unbounded();
    let core = platform::install_core(core::Core::new(cfg.clone(), tx));
    std::thread::spawn(|| platform::core().replay_loop());
    let net = match net::start(cfg.clone(), core, rx) {
        Ok(n) => n,
        Err(e) => exit(&e.to_string()),
    };
    std::thread::spawn(|| status::publish_loop(platform::core()));
    std::thread::spawn(move || watch_config(core, net));
    if cfg.show_tray {
        ui::launch(true);
    }
    let _ = ctrlc::set_handler(|| {
        platform::cleanup();
        std::process::exit(0);
    });

    log::info!("magicinput {} running as \"{}\" (code {})", env!("CARGO_PKG_VERSION"), cfg.name, cfg.code);
    log::info!("Looking for your other computer… (Ctrl+Alt+Esc / Control+Option+Esc brings the pointer back)");
    if let Err(e) = platform::run_capture() {
        exit(&e);
    }
}

/// Apply changes the settings window saves to the config file, without restarting.
fn watch_config(core: &'static core::Core, net: std::sync::Arc<net::Net>) {
    let modified = || std::fs::metadata(Config::path()).and_then(|m| m.modified()).ok();
    let mut last = modified();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let now = modified();
        if now == last {
            continue;
        }
        last = now;
        if let Some(cfg) = Config::read() {
            log::info!("Settings changed");
            core.reload(cfg.clone());
            net.reload(cfg);
        }
    }
}

fn print_pairing(cfg: &Config) {
    println!("\n  Pairing code:  {}\n", cfg.code);
    println!("  On your other computer run:   magicinput pair {}\n", cfg.code);
}

fn exit(msg: &str) -> ! {
    log::error!("{msg}");
    eprintln!("{msg}");
    std::process::exit(1);
}
