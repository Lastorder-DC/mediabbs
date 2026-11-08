//! `/admin`: board and menu management for board administrators
//! (`ADMIN_GROUPS` / `ADMIN_RIGHTS`). Every change is written to `logs`.

use crate::auth::{Profile, UserInfo};
use crate::connection_info::ConnectionInfo;
use crate::custom_error::CustomError;
use crate::db::DbPool;
use crate::mediawiki::MwClient;
use crate::models::menu::validate as validate_menus;
use crate::models::{
    Board, BoardForm, BoardPublic, DeleteBoardError, Log, LogContent, LogEntry, LogType, Menus,
    Permission, SiteSetting,
};
use actix_web::{
    delete, get, post, put, web,
    web::{block, Data, Json, Path, Query},
    HttpResponse, Scope,
};
use std::net::IpAddr;

use super::require_admin;

macro_rules! admin {
    ($token:expr, $mw:expr) => {
        match require_admin($token, &$mw).await? {
            Ok(profile) => profile,
            Err(response) => return Ok(response),
        }
    };
}

fn log(
    conn: &mut diesel::MysqlConnection,
    kind: LogType,
    target: i32,
    profile: &Profile,
    ip: &IpAddr,
) -> anyhow::Result<()> {
    Log::add(
        conn,
        &kind,
        &LogContent { target },
        Some(profile.id),
        Some(&profile.username),
        ip,
    )
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BoardRequest {
    name: String,
    display_name: String,
    #[serde(default)]
    icon: Option<String>,
    write_permission: Permission,
    comment_permission: Permission,
    is_listed: bool,
    is_active: bool,
    separator_before: bool,
}

impl BoardRequest {
    /// The validated form, or an error message for the admin.
    fn into_form(self) -> Result<BoardForm, &'static str> {
        let name = self.name.trim().to_owned();
        if !Board::is_valid_name(&name) {
            return Err("Invalid board name");
        }
        let display_name = self.display_name.trim().to_owned();
        if display_name.is_empty() || display_name.chars().count() > 255 {
            return Err("Invalid display name");
        }
        let icon = match self.icon.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(icon) => {
                let icon = icon.strip_prefix("bi-").unwrap_or(icon);
                if !Board::is_valid_icon(icon) {
                    return Err("Invalid icon name");
                }
                Some(icon.to_owned())
            }
        };
        Ok(BoardForm {
            name,
            display_name,
            icon,
            write_permission: self.write_permission.as_str().to_owned(),
            comment_permission: self.comment_permission.as_str().to_owned(),
            is_listed: self.is_listed,
            is_active: self.is_active,
            separator_before: self.separator_before,
        })
    }
}

enum Outcome {
    Ok(Box<BoardPublic>),
    NotFound,
    NameTaken,
}

#[post("/boards")]
async fn create_board(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    ConnectionInfo { ip }: ConnectionInfo,
    Json(req): Json<BoardRequest>,
) -> Result<HttpResponse, CustomError> {
    let profile = admin!(token, mw);
    let form = match req.into_form() {
        Ok(form) => form,
        Err(message) => return Ok(HttpResponse::BadRequest().body(message)),
    };
    let mut conn = pool.get()?;
    let outcome = block(move || -> anyhow::Result<Outcome> {
        if Board::find_by_name(&mut conn, &form.name).is_ok() {
            return Ok(Outcome::NameTaken);
        }
        let board = Board::create_with(&mut conn, &form)?;
        log(&mut conn, LogType::CreateBoard, board.id, &profile, &ip)?;
        Ok(Outcome::Ok(Box::new(board.get_public())))
    })
    .await??;
    Ok(match outcome {
        Outcome::Ok(board) => HttpResponse::Ok().json(board),
        Outcome::NameTaken => HttpResponse::Conflict().body("Board name is taken"),
        Outcome::NotFound => HttpResponse::NotFound().body("Board is not found"),
    })
}

#[put("/boards/{board_id}")]
async fn update_board(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    ConnectionInfo { ip }: ConnectionInfo,
    board_id: Path<i32>,
    Json(req): Json<BoardRequest>,
) -> Result<HttpResponse, CustomError> {
    let board_id = board_id.into_inner();
    let profile = admin!(token, mw);
    let form = match req.into_form() {
        Ok(form) => form,
        Err(message) => return Ok(HttpResponse::BadRequest().body(message)),
    };
    let mut conn = pool.get()?;
    let outcome = block(move || -> anyhow::Result<Outcome> {
        if Board::find_by_id(&mut conn, board_id).is_err() {
            return Ok(Outcome::NotFound);
        }
        if let Ok(other) = Board::find_by_name(&mut conn, &form.name) {
            if other.id != board_id {
                return Ok(Outcome::NameTaken);
            }
        }
        Board::update(&mut conn, board_id, &form)?;
        log(&mut conn, LogType::UpdateBoard, board_id, &profile, &ip)?;
        let board = Board::find_by_id(&mut conn, board_id)?;
        Ok(Outcome::Ok(Box::new(board.get_public())))
    })
    .await??;
    Ok(match outcome {
        Outcome::Ok(board) => HttpResponse::Ok().json(board),
        Outcome::NameTaken => HttpResponse::Conflict().body("Board name is taken"),
        Outcome::NotFound => HttpResponse::NotFound().body("Board is not found"),
    })
}

#[delete("/boards/{board_id}")]
async fn delete_board(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    ConnectionInfo { ip }: ConnectionInfo,
    board_id: Path<i32>,
) -> Result<HttpResponse, CustomError> {
    let board_id = board_id.into_inner();
    let profile = admin!(token, mw);
    let mut conn = pool.get()?;
    let res = block(move || -> anyhow::Result<Result<(), DeleteBoardError>> {
        let res = Board::delete(&mut conn, board_id)?;
        if res.is_ok() {
            log(&mut conn, LogType::DeleteBoard, board_id, &profile, &ip)?;
        }
        Ok(res)
    })
    .await??;
    Ok(match res {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(DeleteBoardError::NotFound) => HttpResponse::NotFound().body("Board is not found"),
        Err(DeleteBoardError::HasTopics) => HttpResponse::Conflict().body("Board has topics"),
    })
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrderRequest {
    ids: Vec<i32>,
}

#[put("/boards-order")]
async fn reorder_boards(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    ConnectionInfo { ip }: ConnectionInfo,
    Json(req): Json<OrderRequest>,
) -> Result<HttpResponse, CustomError> {
    let profile = admin!(token, mw);
    let mut conn = pool.get()?;
    let boards = block(move || -> anyhow::Result<Option<Vec<BoardPublic>>> {
        if !Board::reorder(&mut conn, &req.ids)? {
            return Ok(None);
        }
        log(&mut conn, LogType::ReorderBoards, 0, &profile, &ip)?;
        let boards = Board::get_all(&mut conn)?;
        Ok(Some(boards.iter().map(Board::get_public).collect()))
    })
    .await??;
    Ok(match boards {
        Some(boards) => HttpResponse::Ok().json(boards),
        None => HttpResponse::BadRequest().body("Every board must be listed once"),
    })
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MenusRequest {
    /// `null` removes the stored menus (config.js / skin defaults apply again).
    menus: Option<Menus>,
}

#[put("/menus")]
async fn put_menus(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    ConnectionInfo { ip }: ConnectionInfo,
    Json(req): Json<MenusRequest>,
) -> Result<HttpResponse, CustomError> {
    let profile = admin!(token, mw);
    let value = match &req.menus {
        Some(menus) => {
            if let Err(message) = validate_menus(menus) {
                return Ok(HttpResponse::BadRequest().body(format!("Invalid menu: {}", message)));
            }
            Some(serde_json::to_string(menus).map_err(anyhow::Error::from)?)
        }
        None => None,
    };
    let mut conn = pool.get()?;
    block(move || -> anyhow::Result<()> {
        match value {
            Some(value) => SiteSetting::set(&mut conn, SiteSetting::MENUS, &value)?,
            None => SiteSetting::delete(&mut conn, SiteSetting::MENUS)?,
        }
        log(&mut conn, LogType::UpdateMenus, 0, &profile, &ip)
    })
    .await??;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "menus": req.menus })))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LogsQuery {
    /// Only entries older than this id (the last id of the previous page).
    before: Option<i32>,
    limit: Option<i64>,
    /// Comma-separated `LogType` names.
    types: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LogsResponse {
    logs: Vec<LogEntry>,
    /// Whether older entries exist (`before` = the last id returned).
    has_more: bool,
}

#[get("/logs")]
async fn get_logs(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    Query(query): Query<LogsQuery>,
) -> Result<HttpResponse, CustomError> {
    admin!(token, mw);
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let mut types = Vec::new();
    for name in query.types.as_deref().unwrap_or("").split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        match LogType::from_name(name) {
            Some(kind) => types.push(kind),
            None => return Ok(HttpResponse::BadRequest().body("Invalid log type")),
        }
    }
    let mut conn = pool.get()?;
    let mut logs = block(move || Log::list(&mut conn, query.before, limit + 1, &types)).await??;
    let has_more = logs.len() as i64 > limit;
    logs.truncate(limit as usize);
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(LogsResponse { logs, has_more }))
}

/// The backend version: "0.4.0-1a2b3c4", or "0.4.0" for release packages
/// (see build.rs).
pub const VERSION: &str = env!("MEDIABBS_VERSION");

#[get("/version")]
async fn get_version(
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
) -> Result<HttpResponse, CustomError> {
    admin!(token, mw);
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(serde_json::json!({ "version": VERSION })))
}

pub fn scope() -> Scope {
    web::scope("/admin")
        .service(create_board)
        .service(update_board)
        .service(delete_board)
        .service(reorder_boards)
        .service(put_menus)
        .service(get_logs)
        .service(get_version)
}
