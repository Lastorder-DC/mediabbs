//! Command line interface for administrative tasks.
//!
//! ```text
//! mediabbs [serve]                         start the HTTP server (default)
//! mediabbs migrate                         apply pending database migrations
//! mediabbs board ...                       manage boards (see USAGE)
//! ```
//!
//! Only `DATABASE_URL` (environment or `.env`) is needed for these commands.

use crate::db;
use crate::models::{Board, BoardForm, DeleteBoardError, Permission};
use anyhow::{anyhow, bail, Result};
use diesel::{Connection, MysqlConnection};
use std::env;

pub const USAGE: &str = "\
Usage:
  mediabbs [serve]                          Start the HTTP server (default)
  mediabbs migrate                          Apply pending database migrations
  mediabbs board list                       List boards (in menu order)
  mediabbs board add <name> <display name>  Create a board
  mediabbs board rename <name> <display>    Change a board's display name
  mediabbs board set-name <name> <new name> Change a board's name (its URL)
  mediabbs board disable <name>             Make a board read-only (archived)
  mediabbs board enable <name>              Make a board writable again
  mediabbs board hide <name>                Leave a board out of the menu
  mediabbs board show <name>                List a board in the menu again
  mediabbs board icon <name> <icon|->       Set a Bootstrap Icons name (- removes)
  mediabbs board separator <name> on|off    Draw a separator above the board
  mediabbs board move <name> <position>     Move a board in the menu (1 = first)
  mediabbs board permission <name> write|comment all|users|admins
                                            Who may write topics / comments
  mediabbs board delete <name>              Delete a board without topics
  mediabbs help | version

Board names are used in URLs: lower-case letters, digits, '-' and '_'.
Boards can also be managed in the web client's admin page (/admin).
Commands other than serve only need DATABASE_URL (environment or .env).";

/// Runs a CLI command and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    dotenvy::dotenv().ok();
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

fn check_name(name: &str) -> Result<()> {
    if !Board::is_valid_name(name) {
        bail!(
            "invalid board name '{}': use lower-case letters, digits, '-' and '_', \
             starting with a letter (reserved: auth, admin, api, uploads, config)",
            name
        );
    }
    Ok(())
}

/// Changes some settings of the board called `name`.
fn edit(name: &str, change: impl FnOnce(&mut BoardForm)) -> Result<()> {
    let mut conn = connect()?;
    let board =
        Board::find_by_name(&mut conn, name).map_err(|_| anyhow!("board '{}' not found", name))?;
    let mut form = board.to_form();
    change(&mut form);
    if form.name != board.name && Board::find_by_name(&mut conn, &form.name).is_ok() {
        bail!("board '{}' already exists", form.name);
    }
    Board::update(&mut conn, board.id, &form)?;
    Ok(())
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
            let mut conn = connect()?;
            db::run_migrations(&mut conn, true)?;
            println!("Migrations are up to date.");
            Ok(())
        }
        ["board", "list"] => {
            let mut conn = connect()?;
            let boards = Board::get_all(&mut conn)?;
            if boards.is_empty() {
                println!("No boards. Create one with: mediabbs board add <name> <display name>");
            }
            for b in boards {
                let mut flags = vec![];
                if !b.is_active {
                    flags.push("disabled".to_owned());
                }
                if !b.is_listed {
                    flags.push("hidden".to_owned());
                }
                if b.separator_before {
                    flags.push("separator".to_owned());
                }
                if let Some(icon) = &b.icon {
                    flags.push(format!("icon={}", icon));
                }
                if b.write_permission != "all" {
                    flags.push(format!("write={}", b.write_permission));
                }
                if b.comment_permission != "all" {
                    flags.push(format!("comment={}", b.comment_permission));
                }
                let flags = if flags.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", flags.join(", "))
                };
                println!("{:>4}  {:<20} {}{}", b.id, b.name, b.display_name, flags);
            }
            Ok(())
        }
        ["board", "add", name, rest @ ..] => {
            check_name(name)?;
            let display_name = join_rest(rest)?;
            let mut conn = connect()?;
            if Board::find_by_name(&mut conn, name).is_ok() {
                bail!("board '{}' already exists", name);
            }
            Board::create(&mut conn, name, &display_name)?;
            println!("Created board '{}' ({}).", name, display_name);
            Ok(())
        }
        ["board", "rename", name, rest @ ..] => {
            let display_name = join_rest(rest)?;
            let mut conn = connect()?;
            match Board::set_display_name(&mut conn, name, &display_name)? {
                0 if Board::find_by_name(&mut conn, name).is_err() => {
                    bail!("board '{}' not found", name)
                }
                _ => println!("Board '{}' is now called '{}'.", name, display_name),
            }
            Ok(())
        }
        ["board", action @ ("enable" | "disable"), name] => {
            let active = *action == "enable";
            let mut conn = connect()?;
            if Board::find_by_name(&mut conn, name).is_err() {
                bail!("board '{}' not found", name);
            }
            Board::set_active(&mut conn, name, active)?;
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
        ["board", "set-name", name, new_name] => {
            check_name(new_name)?;
            edit(name, |form| form.name = (*new_name).to_owned())?;
            println!("Board '{}' is now at /{}.", name, new_name);
            Ok(())
        }
        ["board", action @ ("hide" | "show"), name] => {
            let listed = *action == "show";
            edit(name, |form| form.is_listed = listed)?;
            println!(
                "Board '{}' is now {} the menu.",
                name,
                if listed { "listed in" } else { "left out of" }
            );
            Ok(())
        }
        ["board", "icon", name, icon] => {
            let icon = icon.strip_prefix("bi-").unwrap_or(icon);
            let icon = match icon {
                "-" => None,
                icon if Board::is_valid_icon(icon) => Some(icon.to_owned()),
                _ => bail!(
                    "invalid icon '{}': use a Bootstrap Icons name such as chat-dots \
                     (https://icons.getbootstrap.com/)",
                    icon
                ),
            };
            edit(name, |form| form.icon = icon.clone())?;
            match icon {
                Some(icon) => println!("Board '{}' now has the icon '{}'.", name, icon),
                None => println!("Board '{}' has no icon now.", name),
            }
            Ok(())
        }
        ["board", "separator", name, value @ ("on" | "off")] => {
            let on = *value == "on";
            edit(name, |form| form.separator_before = on)?;
            println!(
                "Board '{}' {} a separator above it.",
                name,
                if on { "has" } else { "no longer has" }
            );
            Ok(())
        }
        ["board", "permission", name, kind @ ("write" | "comment"), value] => {
            let Some(permission) = Permission::parse(value) else {
                bail!("permission must be all, users or admins");
            };
            edit(name, |form| {
                let target = if *kind == "write" {
                    &mut form.write_permission
                } else {
                    &mut form.comment_permission
                };
                *target = permission.as_str().to_owned();
            })?;
            println!(
                "Board '{}': {} permission is now '{}'.",
                name, kind, permission
            );
            Ok(())
        }
        ["board", "move", name, position] => {
            let position: usize = position
                .parse()
                .ok()
                .filter(|p| *p >= 1)
                .ok_or_else(|| anyhow!("position must be a number from 1"))?;
            let mut conn = connect()?;
            let boards = Board::get_all(&mut conn)?;
            let Some(index) = boards.iter().position(|b| b.name == *name) else {
                bail!("board '{}' not found", name);
            };
            let mut ids: Vec<i32> = boards.iter().map(|b| b.id).collect();
            let id = ids.remove(index);
            let position = position.min(ids.len() + 1);
            ids.insert(position - 1, id);
            Board::reorder(&mut conn, &ids)?;
            println!("Board '{}' is now at position {}.", name, position);
            Ok(())
        }
        ["board", "delete", name] => {
            let mut conn = connect()?;
            let board = Board::find_by_name(&mut conn, name)
                .map_err(|_| anyhow!("board '{}' not found", name))?;
            match Board::delete(&mut conn, board.id)? {
                Ok(()) => println!("Deleted board '{}'.", name),
                Err(DeleteBoardError::NotFound) => bail!("board '{}' not found", name),
                Err(DeleteBoardError::HasTopics) => bail!(
                    "board '{}' has topics; disable it instead (mediabbs board disable {})",
                    name,
                    name
                ),
            }
            Ok(())
        }
        _ => bail!("unknown command\n\n{}", USAGE),
    }
}
