use std::env;
use std::fs;
use std::path;
use std::path::Path;
use std::process::Command;

const PID_FILE: &str = "/tmp/shellrec.pid";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        help();
    }

    match args[1].as_str() {
        "start" => start(&args),
        "stop" => stop(),
        "--daemon" => daemon(),
        _ => help(),
    }
}

fn help() {
    println!("usage: shellrec <start | stop>");
    std::process::exit(0);
}

fn start(args: &Vec<String>) {
    if path::Path::new(PID_FILE).exists() {
        let pid = fs::read_to_string(PID_FILE)
            .unwrap_or_default()
            .trim()
            .to_string();
        eprintln!("Already running (PID: {}). Run 'stop' first", pid);
        std::process::exit(1);
    }

    // spawn itself as background process
    let exe = &args[0];
    Command::new(exe)
        .arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn shellrec daemon");

    println!("Started shellrec");
}

fn stop() {
    if !Path::new(PID_FILE).exists() {
        eprintln!("shellrec not running. Start it first using 'start'");
        std::process::exit(1);
    }

    fs::remove_file(PID_FILE).expect("Failed to remove PID file");
    println!("daemon stopped");
}

fn daemon() {
    let pid = std::process::id();
    fs::write(PID_FILE, pid.to_string()).expect("Failed to write PID file");

    loop {}
}
