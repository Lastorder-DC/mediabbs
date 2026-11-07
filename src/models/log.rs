use std::net::IpAddr;

use crate::schema::{boards, comments, logs, topics};
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub enum LogType {
    CloseTopic = 1,
    UncloseTopic = 2,
    HideTopic = 3,
    UnhideTopic = 4,
    SuspendTopic = 5,
    UnsuspendTopic = 6,
    HideComment = 7,
    UnhideComment = 8,
    PinTopic = 9,
    UnpinTopic = 10,
    CreateBoard = 11,
    UpdateBoard = 12,
    DeleteBoard = 13,
    ReorderBoards = 14,
    UpdateMenus = 15,
}

impl LogType {
    pub const ALL: [LogType; 15] = [
        LogType::CloseTopic,
        LogType::UncloseTopic,
        LogType::HideTopic,
        LogType::UnhideTopic,
        LogType::SuspendTopic,
        LogType::UnsuspendTopic,
        LogType::HideComment,
        LogType::UnhideComment,
        LogType::PinTopic,
        LogType::UnpinTopic,
        LogType::CreateBoard,
        LogType::UpdateBoard,
        LogType::DeleteBoard,
        LogType::ReorderBoards,
        LogType::UpdateMenus,
    ];

    pub fn from_id(id: i32) -> Option<LogType> {
        LogType::ALL.into_iter().find(|t| *t as i32 == id)
    }

    /// The name used by the API (`close_topic`, ...).
    pub fn name(self) -> &'static str {
        match self {
            LogType::CloseTopic => "close_topic",
            LogType::UncloseTopic => "unclose_topic",
            LogType::HideTopic => "hide_topic",
            LogType::UnhideTopic => "unhide_topic",
            LogType::SuspendTopic => "suspend_topic",
            LogType::UnsuspendTopic => "unsuspend_topic",
            LogType::HideComment => "hide_comment",
            LogType::UnhideComment => "unhide_comment",
            LogType::PinTopic => "pin_topic",
            LogType::UnpinTopic => "unpin_topic",
            LogType::CreateBoard => "create_board",
            LogType::UpdateBoard => "update_board",
            LogType::DeleteBoard => "delete_board",
            LogType::ReorderBoards => "reorder_boards",
            LogType::UpdateMenus => "update_menus",
        }
    }

    pub fn from_name(name: &str) -> Option<LogType> {
        LogType::ALL.into_iter().find(|t| t.name() == name)
    }

    /// What `LogContent::target` refers to.
    fn target_kind(self) -> TargetKind {
        match self {
            LogType::HideComment | LogType::UnhideComment => TargetKind::Comment,
            LogType::CreateBoard | LogType::UpdateBoard | LogType::DeleteBoard => TargetKind::Board,
            LogType::ReorderBoards | LogType::UpdateMenus => TargetKind::None,
            _ => TargetKind::Topic,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TargetKind {
    Topic,
    Comment,
    Board,
    None,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LogContent {
    pub target: i32,
}

#[derive(Serialize, Deserialize, Queryable, Identifiable, Debug)]
pub struct Log {
    pub id: i32,
    pub log_type_id: i32,
    pub content: String,
    pub user_id: Option<i32>,
    pub user_name: Option<String>,
    pub user_ip: Vec<u8>,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = logs)]
struct NewLog<'a> {
    pub log_type_id: i32,
    pub content: &'a str,
    pub user_id: Option<i32>,
    pub user_name: Option<&'a str>,
    pub user_ip: Vec<u8>,
}

impl Log {
    pub fn add(
        conn: &mut MysqlConnection,
        log_type: &LogType,
        content: &LogContent,
        user_id: Option<i32>,
        user_name: Option<&str>,
        user_ip: &IpAddr,
    ) -> Result<()> {
        let ip_bin: Vec<u8> = match &user_ip {
            IpAddr::V4(ip) => ip.octets().to_vec(),
            IpAddr::V6(ip) => ip.octets().to_vec(),
        };
        let new_comment = NewLog {
            log_type_id: *log_type as i32,
            content: &serde_json::to_string(&content)?,
            user_id,
            user_name,
            user_ip: ip_bin,
        };
        diesel::insert_into(logs::table)
            .values(new_comment)
            .execute(conn)?;
        Ok(())
    }
}

/// A log entry as shown to board administrators, with what its target refers to
/// (the IP address is left out).
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: i32,
    /// `LogType::name`, or `unknown`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub target: i32,
    pub user_id: Option<i32>,
    pub user_name: Option<String>,
    pub created_at: DateTime<Utc>,
    /// The topic for topic and comment entries (when it still exists).
    pub topic_id: Option<i32>,
    pub topic_title: Option<String>,
    /// The board of the topic, or the board itself for board entries.
    pub board_name: Option<String>,
    pub board_display_name: Option<String>,
}

impl Log {
    /// Newest first: at most `limit` entries older than `before` (an id),
    /// optionally only of the given types.
    pub fn list(
        conn: &mut MysqlConnection,
        before: Option<i32>,
        limit: i64,
        types: &[LogType],
    ) -> Result<Vec<LogEntry>> {
        let mut query = logs::table.into_boxed();
        if let Some(before) = before {
            query = query.filter(logs::id.lt(before));
        }
        if !types.is_empty() {
            query = query.filter(logs::log_type_id.eq_any(types.iter().map(|t| *t as i32)));
        }
        let rows: Vec<Log> = query.order(logs::id.desc()).limit(limit).load(conn)?;

        let target = |row: &Log| -> i32 {
            serde_json::from_str::<LogContent>(&row.content)
                .map(|c| c.target)
                .unwrap_or(0)
        };
        let kind_of = |row: &Log| LogType::from_id(row.log_type_id).map(LogType::target_kind);
        let ids = |wanted: TargetKind| -> Vec<i32> {
            rows.iter()
                .filter(|row| kind_of(row) == Some(wanted))
                .map(target)
                .collect()
        };

        let comment_topics: HashMap<i32, i32> = comments::table
            .filter(comments::id.eq_any(ids(TargetKind::Comment)))
            .select((comments::id, comments::topic_id))
            .load::<(i32, i32)>(conn)?
            .into_iter()
            .collect();
        let mut topic_ids = ids(TargetKind::Topic);
        topic_ids.extend(comment_topics.values());
        let topic_info: HashMap<i32, (String, i32)> = topics::table
            .filter(topics::id.eq_any(topic_ids))
            .select((topics::id, topics::title, topics::board_id))
            .load::<(i32, String, i32)>(conn)?
            .into_iter()
            .map(|(id, title, board)| (id, (title, board)))
            .collect();
        let board_info: HashMap<i32, (String, String)> = boards::table
            .select((boards::id, boards::name, boards::display_name))
            .load::<(i32, String, String)>(conn)?
            .into_iter()
            .map(|(id, name, display)| (id, (name, display)))
            .collect();

        Ok(rows
            .iter()
            .map(|row| {
                let kind = LogType::from_id(row.log_type_id);
                let target = target(row);
                let topic_id = match kind.map(LogType::target_kind) {
                    Some(TargetKind::Topic) => Some(target),
                    Some(TargetKind::Comment) => comment_topics.get(&target).copied(),
                    _ => None,
                };
                let topic = topic_id.and_then(|id| topic_info.get(&id));
                let board_id = match kind.map(LogType::target_kind) {
                    Some(TargetKind::Board) => Some(target),
                    _ => topic.map(|(_, board)| *board),
                };
                let board = board_id.and_then(|id| board_info.get(&id));
                LogEntry {
                    id: row.id,
                    kind: kind.map_or("unknown", LogType::name),
                    target,
                    user_id: row.user_id,
                    user_name: row.user_name.clone(),
                    created_at: row.created_at.and_utc(),
                    topic_id: topic.and(topic_id),
                    topic_title: topic.map(|(title, _)| title.clone()),
                    board_name: board.map(|(name, _)| name.clone()),
                    board_display_name: board.map(|(_, display)| display.clone()),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_connection;
    use crate::models::{Board, Topic};
    use std::net::Ipv4Addr;

    #[test]
    fn test_log_type_names() {
        for t in LogType::ALL {
            assert_eq!(
                LogType::from_id(t as i32).map(LogType::name),
                Some(t.name())
            );
            assert_eq!(
                LogType::from_name(t.name()).map(|t| t as i32),
                Some(t as i32)
            );
        }
        assert!(LogType::from_id(0).is_none());
        assert!(LogType::from_name("nope").is_none());
    }

    #[test]
    fn test_list() {
        let mut conn = create_connection();
        conn.test_transaction::<_, anyhow::Error, _>(|conn| {
            let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
            Board::create(conn, "zz-log-test", "Log test")?;
            let board = Board::find_by_name(conn, "zz-log-test")?;
            Topic::create(conn, &board, "Logged topic", Some(1), Some("Admin"), &ip)?;
            let topic = Topic::get_latest(conn)?;
            let add = |conn: &mut MysqlConnection, t: LogType, target: i32| {
                Log::add(
                    conn,
                    &t,
                    &LogContent { target },
                    Some(1),
                    Some("Admin"),
                    &ip,
                )
            };
            add(conn, LogType::CreateBoard, board.id)?;
            add(conn, LogType::PinTopic, topic.id)?;
            add(conn, LogType::UpdateMenus, 0)?;

            let all = Log::list(conn, None, 3, &[])?;
            assert_eq!(all.len(), 3);
            assert_eq!(all[0].kind, "update_menus");
            assert_eq!(all[0].board_name, None);
            assert_eq!(all[1].kind, "pin_topic");
            assert_eq!(all[1].topic_id, Some(topic.id));
            assert_eq!(all[1].topic_title.as_deref(), Some("Logged topic"));
            assert_eq!(all[1].board_name.as_deref(), Some("zz-log-test"));
            assert_eq!(all[2].kind, "create_board");
            assert_eq!(all[2].board_display_name.as_deref(), Some("Log test"));
            assert_eq!(all[2].topic_id, None);

            let older = Log::list(conn, Some(all[0].id), 1, &[])?;
            assert_eq!(older[0].id, all[1].id);
            let pins = Log::list(conn, None, 10, &[LogType::PinTopic])?;
            assert!(pins.iter().all(|e| e.kind == "pin_topic"));
            assert_eq!(pins[0].id, all[1].id);
            Ok(())
        });
    }
}
