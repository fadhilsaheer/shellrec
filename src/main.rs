use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
struct CommandEntry {
    command: String,
    timestamp: DateTime<Utc>,
    stdout: String,
    stderr: String,
    exit_code: i32,
    duration_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct Session {
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    shell: String,
    commands: Vec<CommandEntry>,
}

// ── Paths ─────────────────────────────────────────────────────────────────────

fn state_dir() -> PathBuf {
    let base = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(base).join("shellrec")
}

fn session_file() -> PathBuf {
    let key = env::var("SHELLREC_SESSION").unwrap_or_else(|_| "default".to_string());
    state_dir().join(format!("{}.json", key))
}

fn active_marker() -> PathBuf {
    let key = env::var("SHELLREC_SESSION").unwrap_or_else(|_| "default".to_string());
    state_dir().join(format!("{}.active", key))
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("start") => cmd_start(),
        Some("stop") => cmd_stop(),
        Some("run") => cmd_run(&args[2..]), // internal: run one command and record it
        Some("shell") => cmd_shell(),       // internal: interactive shell loop
        _ => help(),
    }
}

fn help() {
    eprintln!("shellrec — record your shell session\n");
    eprintln!("  shellrec start    Start a recorded shell session");
    eprintln!("  shellrec stop     Stop recording and save to shellrec_<ts>.json");
    std::process::exit(0);
}

// ── start ─────────────────────────────────────────────────────────────────────
//
// Launches `shellrec shell` as the interactive loop in the current terminal.
// The session key is written to the environment so `stop` can find it.

fn cmd_start() {
    fs::create_dir_all(state_dir()).expect("Cannot create state dir");

    let key = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string();

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    // Write initial session
    let session = Session {
        started_at: Utc::now(),
        ended_at: None,
        shell: shell.clone(),
        commands: vec![],
    };
    let session_path = state_dir().join(format!("{}.json", &key));
    fs::write(
        &session_path,
        serde_json::to_string_pretty(&session).unwrap(),
    )
    .unwrap();

    // Write active marker so `stop` can find us without knowing the key
    let marker = state_dir().join(format!("{}.active", &key));
    fs::write(&marker, &key).unwrap();

    // Also write a "latest" pointer for `stop` when called from within this shell
    let latest = state_dir().join("latest.key");
    fs::write(&latest, &key).unwrap();

    println!("● shellrec started");
    println!("  Commands are recorded with their output.");
    println!("  Type `shellrec stop` or `exit` to finish.\n");

    // Exec into our shell loop, passing the key via env
    let exe = env::current_exe().unwrap();
    let status = Command::new(&exe)
        .arg("shell")
        .env("SHELLREC_SESSION", &key)
        .env("SHELLREC_SHELL", &shell)
        .status()
        .expect("Failed to start shell loop");

    // Shell exited normally (user typed `exit`) — finalize
    finalize_session(&key);
    std::process::exit(status.code().unwrap_or(0));
}

// ── stop ──────────────────────────────────────────────────────────────────────

fn cmd_stop() {
    // Find the session key: prefer SHELLREC_SESSION env var (set by `start`),
    // fall back to the "latest" pointer file
    let key = env::var("SHELLREC_SESSION").ok().or_else(|| {
        let latest = state_dir().join("latest.key");
        fs::read_to_string(&latest)
            .ok()
            .map(|s| s.trim().to_string())
    });

    match key {
        None => {
            eprintln!("No active shellrec session found.");
            std::process::exit(1);
        }
        Some(k) => {
            // Signal the shell loop to stop by writing a stop file
            let stop_flag = state_dir().join(format!("{}.stop", &k));
            fs::write(&stop_flag, "stop").unwrap();

            // Wait briefly for the shell loop to see it
            for _ in 0..30 {
                std::thread::sleep(Duration::from_millis(100));
                if !active_marker_for(&k).exists() {
                    break;
                }
            }

            finalize_session(&k);
        }
    }
}

fn active_marker_for(key: &str) -> PathBuf {
    state_dir().join(format!("{}.active", key))
}

fn finalize_session(key: &str) {
    let session_path = state_dir().join(format!("{}.json", key));
    if !session_path.exists() {
        eprintln!("Session file not found.");
        return;
    }

    let json = fs::read_to_string(&session_path).unwrap();
    let mut session: Session = serde_json::from_str(&json).unwrap();
    session.ended_at = Some(Utc::now());

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out = format!("shellrec_{}.json", ts);
    fs::write(&out, serde_json::to_string_pretty(&session).unwrap()).unwrap();

    println!(
        "● Saved → {} ({} command{})",
        out,
        session.commands.len(),
        if session.commands.len() == 1 { "" } else { "s" }
    );

    // Clean up state files
    fs::remove_file(&session_path).ok();
    fs::remove_file(state_dir().join(format!("{}.active", key))).ok();
    fs::remove_file(state_dir().join(format!("{}.stop", key))).ok();
    fs::remove_file(state_dir().join("latest.key")).ok();
}

// ── shell loop ────────────────────────────────────────────────────────────────
//
// A minimal interactive read-eval-print loop.
// Reads a line from stdin, runs it via `shellrec run`, records the result.

fn cmd_shell() {
    let key = env::var("SHELLREC_SESSION").unwrap_or_else(|_| "default".to_string());
    let shell = env::var("SHELLREC_SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let exe = env::current_exe().unwrap();
    let stop_flag = state_dir().join(format!("{}.stop", &key));
    let marker = state_dir().join(format!("{}.active", &key));

    let stdin = io::stdin();

    loop {
        // Check for stop signal
        if stop_flag.exists() {
            fs::remove_file(&marker).ok();
            break;
        }

        // Print prompt
        let cwd = env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".to_string());
        print!("\x1b[32m(rec)\x1b[0m \x1b[34m{}\x1b[0m $ ", cwd);
        io::stdout().flush().unwrap();

        // Read a line
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Err(_) => break,
            Ok(_) => {}
        }

        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }

        // Built-ins
        match cmd {
            "exit" | "quit" => {
                fs::remove_file(&marker).ok();
                break;
            }
            _ if cmd.starts_with("cd") => {
                // Handle cd specially since it changes the process's cwd
                let dir = cmd[2..].trim();
                let target = if dir.is_empty() {
                    env::var("HOME").unwrap_or_else(|_| "/".to_string())
                } else {
                    dir.to_string()
                };
                if let Err(e) = env::set_current_dir(&target) {
                    eprintln!("cd: {}: {}", target, e);
                    record_command(&key, cmd, "", &format!("cd: {}: {}", target, e), 1, 0);
                } else {
                    record_command(&key, cmd, "", "", 0, 0);
                }
                continue;
            }
            _ => {}
        }

        // Run the command, capturing output AND streaming it live
        let timestamp = Utc::now();
        let start_ms = now_ms();

        let output = Command::new(&shell)
            .args(["-c", cmd])
            .current_dir(env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
            .output();

        let duration_ms = now_ms().saturating_sub(start_ms);

        match output {
            Err(e) => {
                let msg = format!("Failed to run command: {}", e);
                eprintln!("{}", msg);
                record_entry(
                    &key,
                    CommandEntry {
                        command: cmd.to_string(),
                        timestamp,
                        stdout: String::new(),
                        stderr: msg,
                        exit_code: -1,
                        duration_ms,
                    },
                );
            }
            Ok(out) => {
                // Stream stdout and stderr to terminal
                io::stdout().write_all(&out.stdout).ok();
                io::stderr().write_all(&out.stderr).ok();

                let stdout_str = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr_str = String::from_utf8_lossy(&out.stderr).to_string();
                let exit_code = out.status.code().unwrap_or(-1);

                record_entry(
                    &key,
                    CommandEntry {
                        command: cmd.to_string(),
                        timestamp,
                        stdout: stdout_str,
                        stderr: stderr_str,
                        exit_code,
                        duration_ms,
                    },
                );
            }
        }
    }
}

// ── run (internal) ────────────────────────────────────────────────────────────
//
// Runs a single command, prints output to stdout/stderr, and appends the
// entry to the session JSON. Called by `shellrec run <cmd...>`.

fn cmd_run(args: &[String]) {
    if args.is_empty() {
        return;
    }

    let cmd_str = args.join(" ");
    let shell = env::var("SHELLREC_SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let timestamp = Utc::now();
    let start_ms = now_ms();

    let out = Command::new(&shell)
        .args(["-c", &cmd_str])
        .output()
        .expect("Failed to run command");

    let duration_ms = now_ms().saturating_sub(start_ms);

    io::stdout().write_all(&out.stdout).ok();
    io::stderr().write_all(&out.stderr).ok();

    let key = env::var("SHELLREC_SESSION").unwrap_or_else(|_| "default".to_string());
    record_entry(
        &key,
        CommandEntry {
            command: cmd_str,
            timestamp,
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            exit_code: out.status.code().unwrap_or(-1),
            duration_ms,
        },
    );
}

// ── recording helpers ─────────────────────────────────────────────────────────

fn record_command(
    key: &str,
    cmd: &str,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
    duration_ms: u64,
) {
    record_entry(
        key,
        CommandEntry {
            command: cmd.to_string(),
            timestamp: Utc::now(),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            exit_code,
            duration_ms,
        },
    );
}

fn record_entry(key: &str, entry: CommandEntry) {
    let path = state_dir().join(format!("{}.json", key));
    let json = fs::read_to_string(&path).unwrap_or_else(|_| {
        serde_json::to_string(&Session {
            started_at: Utc::now(),
            ended_at: None,
            shell: env::var("SHELLREC_SHELL").unwrap_or_default(),
            commands: vec![],
        })
        .unwrap()
    });

    if let Ok(mut session) = serde_json::from_str::<Session>(&json) {
        session.commands.push(entry);
        fs::write(&path, serde_json::to_string_pretty(&session).unwrap()).ok();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
