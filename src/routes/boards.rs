use super::{check_show_hidden, no_store};
use crate::auth::UserInfo;
use crate::config::Config;
use crate::custom_error::CustomError;
use crate::db::DbPool;
use crate::mediawiki::MwClient;
use crate::models::{Board, BoardPublic, TopicPublic};
use actix_web::{
    get, web,
    web::{block, Data, Path, Query},
    HttpResponse, Scope,
};
use derive_more::Display;

#[get("")]
async fn get_boards(pool: Data<DbPool>, config: Data<Config>) -> Result<HttpResponse, CustomError> {
    let mut conn = pool.get()?;
    let boards = block(move || Board::get_all(&mut conn)).await??;
    let boards = boards
        .iter()
        .map(|x| x.get_public())
        .collect::<Vec<BoardPublic>>();

    Ok(HttpResponse::Ok()
        .insert_header((
            "Cache-Control",
            format!("max-age={}", config.site.boards_cache_seconds),
        ))
        .json(boards))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GetTopicsQuery {
    limit: Option<i32>,
    offset: Option<i32>,
    /// Include hidden topics (board administrators only).
    show_hidden: Option<bool>,
}

#[get("{board_id}/topics")]
async fn get_board_topics(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    board_id: Path<i32>,
    query: Query<GetTopicsQuery>,
) -> Result<HttpResponse, CustomError> {
    let board_id = board_id.into_inner();
    let show_hidden = query.show_hidden.unwrap_or(false);
    if let Some(response) = check_show_hidden(show_hidden, token, &mw).await? {
        return Ok(response);
    }
    #[derive(Debug, Display)]
    enum ErrorKind {
        BoardNotFound,
        OtherError(anyhow::Error),
    }

    let limit = query.limit.unwrap_or(10).clamp(1, 20);
    let offset = query.offset.unwrap_or(0).max(0);

    let mut conn = pool.get()?;
    let res = block(move || {
        let board = Board::find_by_id(&mut conn, board_id).map_err(|_| ErrorKind::BoardNotFound)?;
        let topics = board
            .get_topics(&mut conn, limit, offset, show_hidden)
            .map_err(ErrorKind::OtherError)?;
        Ok(topics)
    })
    .await?;
    match res {
        Ok(topics) => {
            let topics = topics
                .iter()
                .map(|x| x.get_public())
                .collect::<Vec<TopicPublic>>();
            let response = HttpResponse::Ok().json(topics);
            Ok(if show_hidden {
                no_store(response)
            } else {
                response
            })
        }
        Err(ErrorKind::BoardNotFound) => Ok(HttpResponse::NotFound().body("Board is not found")),
        Err(ErrorKind::OtherError(e)) => Err(e.into()),
    }
}

pub fn scope() -> Scope {
    web::scope("/boards")
        .service(get_boards)
        .service(get_board_topics)
}
