use std::net::IpAddr;

use super::{check_show_hidden, no_store};
use crate::auth::UserInfo;
use crate::connection_info::ConnectionInfo;
use crate::custom_error::CustomError;
use crate::db::DbPool;
use crate::events::{EventKind, Events};
use crate::mediawiki::MwClient;
use crate::models::{Comment, CommentForm, Log, LogContent, LogType, Topic};
use actix_web::{
    get, put, web,
    web::{block, Data, Path, Query},
    HttpResponse, Scope,
};
use actix_web_validator::Json;
use derive_more::Display;
use diesel::{Connection, MysqlConnection};
use validator::Validate;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GetCommentQuery {
    show_hidden: Option<bool>,
}

#[get("{comment_id}")]
async fn get_comment(
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    comment_id: Path<i32>,
    query: Query<GetCommentQuery>,
) -> Result<HttpResponse, CustomError> {
    let comment_id = comment_id.into_inner();
    let show_hidden = query.show_hidden.unwrap_or(false);
    if let Some(response) = check_show_hidden(show_hidden, token, &mw).await? {
        return Ok(response);
    }
    let mut conn = pool.get()?;
    let found = block(move || -> Option<(Comment, bool)> {
        let comment = Comment::find_by_id(&mut conn, comment_id).ok()?;
        let topic_hidden = Topic::find_by_id(&mut conn, comment.topic_id)
            .map(|t| t.is_hidden)
            .unwrap_or(true);
        Some((comment, topic_hidden))
    })
    .await?;
    match found {
        // Comments of a hidden topic are hidden with it (admins: `showHidden`).
        Some((_, true)) if !show_hidden => Ok(HttpResponse::Forbidden().body("Topic is hidden")),
        Some((comment, _)) => {
            let response = HttpResponse::Ok().json(comment.get_public(show_hidden));
            Ok(if show_hidden {
                no_store(response)
            } else {
                response
            })
        }
        None => Ok(HttpResponse::NotFound().body("Comment is not found")),
    }
}

#[derive(Deserialize, Validate, Debug)]
#[serde(rename_all = "camelCase")]
struct PutCommentStatusRequest {
    is_hidden: Option<bool>,
}

fn log_put_comment_status(
    conn: &mut MysqlConnection,
    comment_id: i32,
    user_id: Option<i32>,
    user_name: Option<&str>,
    user_ip: &IpAddr,
    req_status: PutCommentStatusRequest,
) -> anyhow::Result<()> {
    let log_type = match req_status {
        PutCommentStatusRequest {
            is_hidden: Some(true),
            ..
        } => LogType::HideComment,
        PutCommentStatusRequest {
            is_hidden: Some(false),
            ..
        } => LogType::UnhideComment,
        _ => return Ok(()),
    };
    Log::add(
        conn,
        &log_type,
        &LogContent { target: comment_id },
        user_id,
        user_name,
        user_ip,
    )?;
    Ok(())
}

#[put("{comment_id}/status")]
async fn put_comment_status(
    pool: Data<DbPool>,
    events: Data<Events>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    comment_id: Path<i32>,
    Json(req_status): Json<PutCommentStatusRequest>,
    ConnectionInfo { ip }: ConnectionInfo,
) -> Result<HttpResponse, CustomError> {
    let comment_id = comment_id.into_inner();
    #[derive(Debug, Display)]
    enum ErrorKind {
        CommentNotFound,
        OtherError(anyhow::Error),
    }
    impl From<diesel::result::Error> for ErrorKind {
        fn from(error: diesel::result::Error) -> Self {
            ErrorKind::OtherError(error.into())
        }
    }

    let profile = match token {
        Some(token) => mw.profile(&token).await?,
        None => return Ok(HttpResponse::Unauthorized().body("TokenMissing")),
    };

    if !profile.is_admin() {
        return Ok(HttpResponse::Forbidden().finish());
    }

    let mut conn = pool.get()?;

    let res = block(move || {
        conn.transaction::<Comment, _, _>(|conn| {
            Comment::find_by_id(conn, comment_id).map_err(|_| ErrorKind::CommentNotFound)?;
            let comment_changes = CommentForm {
                id: comment_id,
                is_hidden: req_status.is_hidden,
            };
            let changed = comment_changes.save(conn).map_err(ErrorKind::OtherError)?;
            log_put_comment_status(
                conn,
                comment_id,
                Some(profile.id),
                Some(&profile.username),
                &ip,
                req_status,
            )
            .map_err(ErrorKind::OtherError)?;
            Ok(changed)
        })
    })
    .await?;

    match res {
        Ok(comment) => {
            events.publish(comment.topic_id, EventKind::CommentUpdated, comment.id);
            Ok(HttpResponse::Ok().json(comment.get_public(true)))
        }
        Err(ErrorKind::CommentNotFound) => {
            Ok(HttpResponse::NotFound().body("Comment is not found"))
        }
        Err(ErrorKind::OtherError(e)) => Err(e.into()),
    }
}

pub fn scope() -> Scope {
    web::scope("/comments")
        .service(get_comment)
        .service(put_comment_status)
}
