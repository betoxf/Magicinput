mod config;
mod core;
mod geom;
mod keymap;
mod net;
mod platform;
mod proto;

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
    /// Start sharing (the default)
    Run,
    /// Pair computers. Without a code, makes a new one. With a code, uses the one shown on the other computer.
    Pair { code: Option<String> },
    /// Where the other computer's screen is: auto (default), left, right, top or bottom
    Side { side: String },
    /// Connect to a fixed address when automatic discovery doesn't work (use "auto" to clear)
    Peer { addr: String },
    /// Show settings and where they are stored
    Config,
}

fn main() {
    let cli = Cli::parse();
    let mut cfg = Config::load();
    match cli.cmd.unwrap_or(Cmd::Run) {
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
            run(cfg);
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
    if let Err(e) = platform::init() {
        exit(&e);
    }

    let (tx, rx) = crossbeam_channel::unbounded();
    let core = platform::install_core(core::Core::new(cfg.clone(), tx));
    if let Err(e) = net::start(cfg.clone(), core, rx) {
        exit(&e.to_string());
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

fn print_pairing(cfg: &Config) {
    println!("\n  Pairing code:  {}\n", cfg.code);
    println!("  On your other computer run:   magicinput pair {}\n", cfg.code);
}

fn exit(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}
