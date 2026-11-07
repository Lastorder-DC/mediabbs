//! `GET /search`: topics and comments (see models::search).

use crate::custom_error::CustomError;
use crate::db::DbPool;
use crate::models::search::{self, SearchField, SearchHit, SearchQuery, MAX_QUERY_CHARS};
use actix_web::{
    get, web,
    web::{block, Data, Query},
    HttpResponse, Scope,
};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Searches running at once. Each one scans the tables, so this keeps a burst
/// of searches from taking every database connection.
const MAX_CONCURRENT: usize = 3;
/// Results past this are not offered (use a narrower search).
const MAX_OFFSET: i64 = 1000;
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Holds one of the `MAX_CONCURRENT` slots.
struct Slot;

impl Slot {
    fn take() -> Option<Slot> {
        if RUNNING.fetch_add(1, Ordering::AcqRel) >= MAX_CONCURRENT {
            RUNNING.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::AcqRel);
    }
}

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
    let offset = params.offset.unwrap_or(0).clamp(0, MAX_OFFSET);
    let Some(slot) = Slot::take() else {
        return Ok(HttpResponse::ServiceUnavailable()
            .insert_header(("Retry-After", "2"))
            .body("Search is busy"));
    };
    let mut conn = pool.get()?;
    let mut results = block(move || {
        let _slot = slot;
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
    let has_more = results.len() as i64 > limit && offset + limit < MAX_OFFSET;
    results.truncate(limit as usize);
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-cache"))
        .json(SearchResponse { results, has_more }))
}

pub fn scope() -> Scope {
    web::scope("/search").service(get_search)
}
