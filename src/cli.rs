//! Command line interface for administrative tasks.
//!
//! ```text
//! mediabbs [serve]                         start the HTTP server (default)
//! mediabbs migrate                         apply pending database migrations
//! mediabbs board list                      list boards
//! mediabbs board add <name> <display name> create a board
//! mediabbs board rename <name> <display>   change a board's display name
//! mediabbs board disable <name>            make a board read-only
//! mediabbs board enable <name>             make a board writable again
//! ```
//!
//! Only `DATABASE_URL` (environment or `.env`) is needed for these commands.

use crate::db;
use crate::models::Board;
use anyhow::{anyhow, bail, Result};
use diesel::{Connection, MysqlConnection};
use std::env;

pub const USAGE: &str = "\
Usage:
  mediabbs [serve]                          Start the HTTP server (default)
  mediabbs migrate                          Apply pending database migrations
  mediabbs board list                       List boards
  mediabbs board add <name> <display name>  Create a board
  mediabbs board rename <name> <display>    Change a board's display name
  mediabbs board disable <name>             Make a board read-only
  mediabbs board enable <name>              Make a board writable again
  mediabbs help | version

Board names are used in URLs: lower-case letters, digits, '-' and '_'.
Commands other than serve only need DATABASE_URL (environment or .env).";

/// Runs a CLI command and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    dotenv::dotenv().ok();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match execute(&args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

fn connect() -> Result<MysqlConnection> {
    let url = env::var("DATABASE_URL").map_err(|_| anyhow!("DATABASE_URL is not set"))?;
    MysqlConnection::establish(&url).map_err(|e| anyhow!("cannot connect to database: {}", e))
}

fn join_rest(rest: &[&str]) -> Result<String> {
    let s = rest.join(" ").trim().to_owned();
    if s.is_empty() {
        bail!("display name is required\n\n{}", USAGE);
    }
    if s.chars().count() > 255 {
        bail!("display name is too long (max 255 characters)");
    }
    Ok(s)
}

fn execute(args: &[&str]) -> Result<()> {
    match args {
        ["help"] | ["-h"] | ["--help"] => {
            println!("{}", USAGE);
            Ok(())
        }
        ["version"] | ["-V"] | ["--version"] => {
            println!("mediabbs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["migrate"] => {
            let conn = connect()?;
            db::run_migrations(&conn, true)?;
            println!("Migrations are up to date.");
            Ok(())
        }
        ["board", "list"] => {
            let conn = connect()?;
            let boards = Board::get_all(&conn)?;
            if boards.is_empty() {
                println!("No boards. Create one with: mediabbs board add <name> <display name>");
            }
            for b in boards {
                println!(
                    "{:>4}  {:<20} {}{}",
                    b.id,
                    b.name,
                    b.display_name,
                    if b.is_active { "" } else { "  (disabled)" }
                );
            }
            Ok(())
        }
        ["board", "add", name, rest @ ..] => {
            if !Board::is_valid_name(name) {
                bail!(
                    "invalid board name '{}': use lower-case letters, digits, '-' and '_', \
                     starting with a letter (reserved: auth, admin, api, uploads, config)",
                    name
                );
            }
            let display_name = join_rest(rest)?;
            let conn = connect()?;
            if Board::find_by_name(&conn, name).is_ok() {
                bail!("board '{}' already exists", name);
            }
            Board::create(&conn, name, &display_name)?;
            println!("Created board '{}' ({}).", name, display_name);
            Ok(())
        }
        ["board", "rename", name, rest @ ..] => {
            let display_name = join_rest(rest)?;
            let conn = connect()?;
            match Board::set_display_name(&conn, name, &display_name)? {
                0 if Board::find_by_name(&conn, name).is_err() => {
                    bail!("board '{}' not found", name)
                }
                _ => println!("Board '{}' is now called '{}'.", name, display_name),
            }
            Ok(())
        }
        ["board", action @ ("enable" | "disable"), name] => {
            let active = *action == "enable";
            let conn = connect()?;
            if Board::find_by_name(&conn, name).is_err() {
                bail!("board '{}' not found", name);
            }
            Board::set_active(&conn, name, active)?;
            println!(
                "Board '{}' is now {}.",
                name,
                if active {
                    "enabled"
                } else {
                    "disabled (read-only)"
                }
            );
            Ok(())
        }
        _ => bail!("unknown command\n\n{}", USAGE),
    }
}
