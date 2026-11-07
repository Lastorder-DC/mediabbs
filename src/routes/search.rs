//! `GET /search`: topics and comments (see models::search).

use crate::custom_error::CustomError;
use crate::db::DbPool;
use crate::models::search::{self, SearchField, SearchHit, SearchQuery, MAX_QUERY_CHARS};
use actix_web::{
    get, web,
    web::{block, Data, Query},
    HttpResponse, Scope,
};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchParams {
    q: String,
    /// `all` (titles and comments), `title`, `content` or `author`.
    #[serde(default, rename = "in")]
    field: SearchField,
    /// Only this board.
    board: Option<i32>,
    offset: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    results: Vec<SearchHit>,
    has_more: bool,
}

#[get("")]
async fn get_search(
    pool: Data<DbPool>,
    Query(params): Query<SearchParams>,
) -> Result<HttpResponse, CustomError> {
    let text = params.q.trim().to_owned();
    if text.is_empty() {
        return Ok(HttpResponse::BadRequest().body("Empty search"));
    }
    if text.chars().count() > MAX_QUERY_CHARS {
        return Ok(HttpResponse::BadRequest().body("Search is too long"));
    }
    let limit = params.limit.unwrap_or(20).clamp(1, 50);
    let offset = params.offset.unwrap_or(0).max(0);
    let mut conn = pool.get()?;
    let mut results = block(move || {
        search::search(
            &mut conn,
            &SearchQuery {
                text: &text,
                field: params.field,
                board_id: params.board,
                limit,
                offset,
            },
        )
    })
    .await??;
    let has_more = results.len() as i64 > limit;
    results.truncate(limit as usize);
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-cache"))
        .json(SearchResponse { results, has_more }))
}

pub fn scope() -> Scope {
    web::scope("/search").service(get_search)
}
