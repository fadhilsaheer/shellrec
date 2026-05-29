use std::env;
use std::process::Command;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        help();
    }

    match args[1].as_str() {
        "start" => shell_start(),
        _ => help(),
    }
}

fn help() {
    println!("usage: shellrec <start | stop>");
    std::process::exit(0);
}

fn shell_start() {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    println!("STARTED SHELL RECORDING");

    Command::new(&shell)
        .env("PS1", "(record) $ ")
        .status()
        .expect("Failed to run shell");
}
