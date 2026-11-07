use std::net::IpAddr;

use crate::auth::{Profile, UserInfo};
use crate::connection_info::ConnectionInfo;
use crate::custom_error::CustomError;
use crate::db::DbPool;
use crate::events::{EventKind, Events};
use crate::mediawiki::MwClient;
use crate::models::{
    Board, Comment, CommentPublic, Log, LogContent, LogType, Permission, PublicEntity, Topic,
    TopicForm,
};
use crate::rate_limit::{too_many, PostLimiter};
use actix_web::{
    get, post, put, web,
    web::{block, Data, Path, Query},
    HttpRequest, HttpResponse, Scope,
};
use actix_web_validator::Json;
use derive_more::Display;
use diesel::{Connection, MysqlConnection};
use validator::Validate;

#[get("{topic_id}")]
async fn get_topic(
    pool: Data<DbPool>,
    topic_id: Path<i32>,
    request: HttpRequest,
) -> Result<HttpResponse, CustomError> {
    let topic_id = topic_id.into_inner();
    let mut conn = pool.get()?;
    match block(move || Topic::find_by_id(&mut conn, topic_id)).await? {
        Ok(topic) => {
            if topic.is_hidden {
                Ok(HttpResponse::Forbidden().body("Topic is hidden"))
            } else {
                Ok(topic.get_public().cache_response(&request))
            }
        }
        Err(_) => Ok(HttpResponse::NotFound().body("Topic is not found")),
    }
}

#[derive(Deserialize, Validate, Debug)]
#[serde(rename_all = "camelCase")]
struct PutTopicStatusRequest {
    is_closed: Option<bool>,
    is_suspended: Option<bool>,
    is_hidden: Option<bool>,
    is_pinned: Option<bool>,
}

fn log_put_topic_status(
    conn: &mut MysqlConnection,
    topic_id: i32,
    user_id: Option<i32>,
    user_name: Option<&str>,
    user_ip: &IpAddr,
    req_status: PutTopicStatusRequest,
) -> anyhow::Result<()> {
    let log_type = match req_status {
        PutTopicStatusRequest {
            is_closed: Some(true),
            ..
        } => LogType::CloseTopic,
        PutTopicStatusRequest {
            is_closed: Some(false),
            ..
        } => LogType::UncloseTopic,
        PutTopicStatusRequest {
            is_hidden: Some(true),
            ..
        } => LogType::HideTopic,
        PutTopicStatusRequest {
            is_hidden: Some(false),
            ..
        } => LogType::UnhideTopic,
        PutTopicStatusRequest {
            is_suspended: Some(true),
            ..
        } => LogType::SuspendTopic,
        PutTopicStatusRequest {
            is_suspended: Some(false),
            ..
        } => LogType::UnsuspendTopic,
        PutTopicStatusRequest {
            is_pinned: Some(true),
            ..
        } => LogType::PinTopic,
        PutTopicStatusRequest {
            is_pinned: Some(false),
            ..
        } => LogType::UnpinTopic,
        _ => return Ok(()),
    };
    Log::add(
        conn,
        &log_type,
        &LogContent { target: topic_id },
        user_id,
        user_name,
        user_ip,
    )?;
    Ok(())
}

#[put("{topic_id}/status")]
async fn put_topic_status(
    pool: Data<DbPool>,
    events: Data<Events>,
    mw: Data<MwClient>,
    UserInfo { token, .. }: UserInfo,
    topic_id: Path<i32>,
    Json(req_status): Json<PutTopicStatusRequest>,
    ConnectionInfo { ip }: ConnectionInfo,
) -> Result<HttpResponse, CustomError> {
    let topic_id = topic_id.into_inner();
    #[derive(Debug, Display)]
    enum ErrorKind {
        TopicNotFound,
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
        conn.transaction::<Topic, _, _>(|conn| {
            Topic::find_by_id(conn, topic_id).map_err(|_| ErrorKind::TopicNotFound)?;
            let topic_changes = TopicForm {
                id: topic_id,
                is_closed: req_status.is_closed,
                is_suspended: req_status.is_suspended,
                is_hidden: req_status.is_hidden,
                is_pinned: req_status.is_pinned,
            };
            let changed = topic_changes.save(conn).map_err(ErrorKind::OtherError)?;
            log_put_topic_status(
                conn,
                topic_id,
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
        Ok(topic) => {
            events.publish(topic.id, EventKind::Topic, topic.id);
            Ok(HttpResponse::Ok().json(topic.get_public()))
        }
        Err(ErrorKind::TopicNotFound) => Ok(HttpResponse::NotFound().body("Topic is not found")),
        Err(ErrorKind::OtherError(e)) => Err(e.into()),
    }
}

#[derive(Deserialize, Debug)]
struct GetCommentsQuery {
    limit: Option<i32>,
    offset: Option<i32>,
}

#[get("{topic_id}/comments")]
async fn get_topic_comments(
    pool: Data<DbPool>,
    topic_id: Path<i32>,
    query: Query<GetCommentsQuery>,
    request: HttpRequest,
) -> Result<HttpResponse, CustomError> {
    let topic_id = topic_id.into_inner();
    #[derive(Debug, Display)]
    enum ErrorKind {
        TopicNotFound,
        TopicIsHidden,
        OtherError(anyhow::Error),
    }

    let limit = query.limit.unwrap_or(10).clamp(1, 20);
    let offset = query.offset.unwrap_or(0).max(0);

    let mut conn = pool.get()?;
    let res = block(move || {
        if let Ok(topic) = Topic::find_by_id(&mut conn, topic_id) {
            if topic.is_hidden {
                return Err(ErrorKind::TopicIsHidden);
            }
            let comments = topic
                .get_comments(&mut conn, limit, offset)
                .map_err(ErrorKind::OtherError)?;
            Ok(comments)
        } else {
            Err(ErrorKind::TopicNotFound)
        }
    })
    .await?;
    match res {
        Ok(comments) => {
            let comments = comments
                .iter()
                .map(|x| x.get_public(false))
                .collect::<Vec<CommentPublic>>();
            Ok(comments.cache_response(&request))
        }
        Err(ErrorKind::TopicNotFound) => Ok(HttpResponse::NotFound().body("Topic is not found")),
        Err(ErrorKind::TopicIsHidden) => Ok(HttpResponse::Forbidden().body("Topic is hidden")),
        Err(ErrorKind::OtherError(e)) => Err(e.into()),
    }
}

#[derive(Deserialize, Validate, Debug)]
#[serde(rename_all = "camelCase")]
struct PostTopicRequest {
    board_id: i32,
    #[validate(length(min = 1, max = 100))]
    title: String,
    #[validate(length(min = 1, max = 100000))]
    content: String,
}

#[post("")]
async fn post_topic(
    ConnectionInfo { ip }: ConnectionInfo,
    Json(PostTopicRequest {
        board_id,
        title,
        content,
    }): Json<PostTopicRequest>,
    UserInfo { token, .. }: UserInfo,
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    limiter: Data<PostLimiter>,
) -> Result<HttpResponse, CustomError> {
    // Before anything that costs (wiki lookups, the database).
    if let Err(wait) = limiter.check(ip) {
        return Ok(too_many(wait));
    }
    #[derive(Debug, Display)]
    enum ErrorKind {
        BoardNotFound,
        BoardIsInactive,
        NotPermitted(Permission),
        OtherError(anyhow::Error),
    }

    let profile = match token {
        Some(token) => Some(mw.profile(&token).await?),
        None => None,
    };

    if let Some(profile) = &profile {
        if profile.blocked {
            return Ok(HttpResponse::Forbidden().body("You are blocked"));
        }
    } else if mw.is_ip_blocked(&ip).await? {
        return Ok(HttpResponse::Forbidden().body("You are blocked"));
    }

    let mut conn = pool.get()?;
    let res = block(move || -> Result<Topic, ErrorKind> {
        let board = Board::find_by_id(&mut conn, board_id).map_err(|_| ErrorKind::BoardNotFound)?;
        if !board.is_active {
            return Err(ErrorKind::BoardIsInactive);
        }
        let permission = board.write_permission();
        if !permission.allows(profile.as_ref().map(Profile::is_admin)) {
            return Err(ErrorKind::NotPermitted(permission));
        }
        let topic = conn
            .transaction::<Topic, _, _>(|conn| match profile {
                Some(Profile { id, username, .. }) => {
                    let topic =
                        Topic::create(conn, &board, &title, Some(id), Some(&username), &ip)?;
                    Comment::create(conn, &topic, &content, Some(id), Some(&username), &ip)?;
                    Ok(topic)
                }
                None => {
                    let topic = Topic::create(conn, &board, &title, None, None, &ip)?;
                    Comment::create(conn, &topic, &content, None, None, &ip)?;
                    Ok(topic)
                }
            })
            .map_err(ErrorKind::OtherError)?;
        Ok(topic)
    })
    .await?;
    match res {
        Ok(topic) => Ok(HttpResponse::Ok().json(topic.get_public())),
        Err(ErrorKind::BoardNotFound) => Ok(HttpResponse::NotFound().body("Board is not found")),
        Err(ErrorKind::BoardIsInactive) => {
            Ok(HttpResponse::Forbidden().body("Board is not active"))
        }
        Err(ErrorKind::NotPermitted(p)) => Ok(not_permitted(p)),
        Err(ErrorKind::OtherError(e)) => Err(e.into()),
    }
}

/// 403 for a board whose write/comment permission excludes this user.
fn not_permitted(permission: Permission) -> HttpResponse {
    match permission {
        Permission::Users => HttpResponse::Forbidden().body("LoginRequired"),
        _ => HttpResponse::Forbidden().body("Admin only"),
    }
}

#[derive(Deserialize, Validate, Debug)]
#[serde(rename_all = "camelCase")]
struct PostCommentRequest {
    #[validate(length(min = 1, max = 100000))]
    content: String,
}

#[post("{topic_id}/comments")]
#[allow(clippy::too_many_arguments)] // actix extractors
async fn post_topic_comments(
    ConnectionInfo { ip }: ConnectionInfo,
    Json(PostCommentRequest { content }): Json<PostCommentRequest>,
    topic_id: Path<i32>,
    UserInfo { token, .. }: UserInfo,
    pool: Data<DbPool>,
    mw: Data<MwClient>,
    events: Data<Events>,
    limiter: Data<PostLimiter>,
) -> Result<HttpResponse, CustomError> {
    if let Err(wait) = limiter.check(ip) {
        return Ok(too_many(wait));
    }
    let topic_id = topic_id.into_inner();
    #[derive(Debug, Display)]
    enum ErrorKind {
        TopicNotFound,
        TopicIsHidden,
        TopicIsSuspended,
        TopicIsClosed,
        BoardIsInactive,
        NotPermitted(Permission),
        OtherError(anyhow::Error),
    }

    let profile = match token {
        Some(token) => Some(mw.profile(&token).await?),
        None => None,
    };

    if let Some(profile) = &profile {
        if profile.blocked {
            return Ok(HttpResponse::Forbidden().body("You are blocked"));
        }
    } else if mw.is_ip_blocked(&ip).await? {
        return Ok(HttpResponse::Forbidden().body("You are blocked"));
    }

    let user = profile.as_ref().map(Profile::is_admin);
    let mut conn = pool.get()?;
    let res = block(move || -> Result<i32, ErrorKind> {
        let topic = Topic::find_by_id(&mut conn, topic_id).map_err(|_| ErrorKind::TopicNotFound)?;
        if topic.is_hidden {
            return Err(ErrorKind::TopicIsHidden);
        } else if topic.is_closed {
            return Err(ErrorKind::TopicIsClosed);
        } else if topic.is_suspended {
            return Err(ErrorKind::TopicIsSuspended);
        }
        match Board::find_by_id(&mut conn, topic.board_id) {
            Ok(board) if !board.is_active => return Err(ErrorKind::BoardIsInactive),
            Ok(board) if !board.comment_permission().allows(user) => {
                return Err(ErrorKind::NotPermitted(board.comment_permission()))
            }
            Ok(_) => {}
            Err(e) => return Err(ErrorKind::OtherError(e)),
        }
        let (author_id, author_name) = match &profile {
            Some(Profile { id, username, .. }) => (Some(*id), Some(username.as_str())),
            None => (None, None),
        };
        Comment::create(&mut conn, &topic, &content, author_id, author_name, &ip)
            .map_err(ErrorKind::OtherError)
    })
    .await?;
    match res {
        Ok(comment_id) => {
            events.publish(topic_id, EventKind::Comment, comment_id);
            Ok(HttpResponse::NoContent().finish())
        }
        Err(ErrorKind::TopicNotFound) => Ok(HttpResponse::NotFound().body("Topic is not found")),
        Err(ErrorKind::TopicIsHidden) => Ok(HttpResponse::Forbidden().body("Topic is hidden")),
        Err(ErrorKind::TopicIsClosed) => Ok(HttpResponse::Forbidden().body("Topic is closed")),
        Err(ErrorKind::TopicIsSuspended) => {
            Ok(HttpResponse::Forbidden().body("Topic is suspended"))
        }
        Err(ErrorKind::BoardIsInactive) => {
            Ok(HttpResponse::Forbidden().body("Board is not active"))
        }
        Err(ErrorKind::NotPermitted(p)) => Ok(not_permitted(p)),
        Err(ErrorKind::OtherError(e)) => Err(e.into()),
    }
}

/// Live updates for a topic page (Server-Sent Events, see `events`):
/// `comment`, `comment_updated` and `topic` events with `{"id": …}`, `resync`
/// when events were missed. `503` when live updates are off or full; clients
/// then poll.
#[get("{topic_id}/events")]
async fn get_topic_events(
    pool: Data<DbPool>,
    events: Data<Events>,
    topic_id: Path<i32>,
    ConnectionInfo { ip }: ConnectionInfo,
) -> Result<HttpResponse, CustomError> {
    let topic_id = topic_id.into_inner();
    let mut conn = pool.get()?;
    let hidden = block(move || Topic::find_by_id(&mut conn, topic_id).map(|t| t.is_hidden)).await?;
    match hidden {
        Err(_) => return Ok(HttpResponse::NotFound().body("Topic is not found")),
        Ok(true) => return Ok(HttpResponse::Forbidden().body("Topic is hidden")),
        Ok(false) => {}
    }
    match events.stream(topic_id, ip) {
        Some(stream) => Ok(HttpResponse::Ok()
            .content_type("text/event-stream")
            .insert_header(("Cache-Control", "no-cache, no-transform"))
            // nginx: pass events through instead of buffering the response.
            .insert_header(("X-Accel-Buffering", "no"))
            .streaming(stream)),
        None => Ok(HttpResponse::ServiceUnavailable().body("Live updates are unavailable")),
    }
}

pub fn scope() -> Scope {
    web::scope("/topics")
        .service(post_topic)
        .service(get_topic)
        .service(put_topic_status)
        .service(get_topic_comments)
        .service(post_topic_comments)
        .service(get_topic_events)
}
