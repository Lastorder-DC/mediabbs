use crate::models::Topic;
use crate::schema::{boards, topics};
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;

/// Who may write in a board (new topics or comments).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Everyone, including users who are not logged in.
    All,
    /// Logged-in users.
    Users,
    /// Board administrators (`ADMIN_GROUPS` / `ADMIN_RIGHTS`).
    Admins,
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Permission {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "all" => Some(Self::All),
            "users" => Some(Self::Users),
            "admins" => Some(Self::Admins),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Users => "users",
            Self::Admins => "admins",
        }
    }
    /// `user`: `None` for anonymous users, else whether the user is an admin.
    pub fn allows(self, user: Option<bool>) -> bool {
        match self {
            Self::All => true,
            Self::Users => user.is_some(),
            Self::Admins => user == Some(true),
        }
    }
}

#[derive(Serialize, Deserialize, Queryable, Identifiable, Debug)]
pub struct Board {
    pub id: i32,
    pub display_name: String,
    pub name: String,
    pub is_active: bool,
    pub sort_order: i32,
    pub icon: Option<String>,
    pub write_permission: String,
    pub comment_permission: String,
    pub is_listed: bool,
    pub separator_before: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// The editable settings of a board (admin UI / `PUT /admin/boards/{id}`).
#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = boards, treat_none_as_null = true)]
pub struct BoardForm {
    pub name: String,
    pub display_name: String,
    pub icon: Option<String>,
    pub write_permission: String,
    pub comment_permission: String,
    pub is_listed: bool,
    pub is_active: bool,
    pub separator_before: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeleteBoardError {
    NotFound,
    HasTopics,
}

impl Board {
    /// All boards in menu order.
    pub fn get_all(conn: &mut MysqlConnection) -> Result<Vec<Self>> {
        let results = boards::table
            .order_by(boards::sort_order.asc())
            .then_order_by(boards::id.asc())
            .load::<Self>(conn)?;
        Ok(results)
    }
    pub fn find_by_name(conn: &mut MysqlConnection, name: &str) -> Result<Self> {
        let board = boards::table
            .filter(boards::name.eq(name))
            .first::<Self>(conn)?;
        Ok(board)
    }
    fn next_sort_order(conn: &mut MysqlConnection) -> Result<i32> {
        let max: Option<i32> = boards::table
            .select(diesel::dsl::max(boards::sort_order))
            .first(conn)?;
        Ok(max.unwrap_or(0) + 1)
    }
    /// Creates a board with default settings at the end of the menu.
    pub fn create(conn: &mut MysqlConnection, name: &str, display_name: &str) -> Result<()> {
        let sort_order = Self::next_sort_order(conn)?;
        diesel::insert_into(boards::table)
            .values((
                boards::name.eq(name),
                boards::display_name.eq(display_name),
                boards::sort_order.eq(sort_order),
            ))
            .execute(conn)?;
        Ok(())
    }
    /// Creates a board at the end of the menu and returns it.
    pub fn create_with(conn: &mut MysqlConnection, form: &BoardForm) -> Result<Self> {
        conn.transaction(|conn| {
            let sort_order = Self::next_sort_order(conn)?;
            diesel::insert_into(boards::table)
                .values((form, boards::sort_order.eq(sort_order)))
                .execute(conn)?;
            Self::find_by_name(conn, &form.name)
        })
    }
    pub fn update(conn: &mut MysqlConnection, id: i32, form: &BoardForm) -> Result<usize> {
        let n = diesel::update(boards::table.find(id))
            .set(form)
            .execute(conn)?;
        Ok(n)
    }
    /// Deletes a board that has no topics (hidden ones included).
    pub fn delete(
        conn: &mut MysqlConnection,
        id: i32,
    ) -> Result<std::result::Result<(), DeleteBoardError>> {
        conn.transaction(|conn| {
            if Self::find_by_id(conn, id).is_err() {
                return Ok(Err(DeleteBoardError::NotFound));
            }
            let topics: i64 = topics::table
                .filter(topics::board_id.eq(id))
                .count()
                .get_result(conn)?;
            if topics > 0 {
                return Ok(Err(DeleteBoardError::HasTopics));
            }
            diesel::delete(boards::table.find(id)).execute(conn)?;
            Ok(Ok(()))
        })
    }
    /// Sets the menu order. `ids` must list every board exactly once; returns
    /// `false` (and changes nothing) otherwise.
    pub fn reorder(conn: &mut MysqlConnection, ids: &[i32]) -> Result<bool> {
        conn.transaction(|conn| {
            let mut current: Vec<i32> = boards::table.select(boards::id).load(conn)?;
            let mut wanted = ids.to_vec();
            current.sort_unstable();
            wanted.sort_unstable();
            if current != wanted {
                return Ok(false);
            }
            for (index, id) in ids.iter().enumerate() {
                diesel::update(boards::table.find(id))
                    .set(boards::sort_order.eq(index as i32 + 1))
                    .execute(conn)?;
            }
            Ok(true)
        })
    }
    pub fn set_display_name(
        conn: &mut MysqlConnection,
        name: &str,
        display_name: &str,
    ) -> Result<usize> {
        let n = diesel::update(boards::table.filter(boards::name.eq(name)))
            .set(boards::display_name.eq(display_name))
            .execute(conn)?;
        Ok(n)
    }
    pub fn set_active(conn: &mut MysqlConnection, name: &str, is_active: bool) -> Result<usize> {
        let n = diesel::update(boards::table.filter(boards::name.eq(name)))
            .set(boards::is_active.eq(is_active))
            .execute(conn)?;
        Ok(n)
    }
    /// The current settings, to change some of them with `update`.
    pub fn to_form(&self) -> BoardForm {
        BoardForm {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            icon: self.icon.clone(),
            write_permission: self.write_permission.clone(),
            comment_permission: self.comment_permission.clone(),
            is_listed: self.is_listed,
            is_active: self.is_active,
            separator_before: self.separator_before,
        }
    }
    pub fn write_permission(&self) -> Permission {
        Permission::parse(&self.write_permission).unwrap_or(Permission::Admins)
    }
    pub fn comment_permission(&self) -> Permission {
        Permission::parse(&self.comment_permission).unwrap_or(Permission::Admins)
    }
    /// Bootstrap Icons name (`chat-dots`, no `bi-` prefix): lower-case letters,
    /// digits and single dashes.
    pub fn is_valid_icon(icon: &str) -> bool {
        !icon.is_empty()
            && icon.len() <= 64
            && !icon.starts_with('-')
            && !icon.ends_with('-')
            && !icon.contains("--")
            && icon
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }
    /// Board names appear in client URLs (`/<name>/<topic id>`): lower-case ASCII
    /// letters, digits, `-` and `_`, not starting with a digit or symbol, and not a
    /// reserved client route.
    pub fn is_valid_name(name: &str) -> bool {
        const RESERVED: &[&str] = &["auth", "admin", "api", "uploads", "config"];
        let first_ok = name
            .chars()
            .next()
            .map(|c| c.is_ascii_lowercase())
            .unwrap_or(false);
        first_ok
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
            && !RESERVED.contains(&name)
    }
    pub fn find_by_id(conn: &mut MysqlConnection, id: i32) -> Result<Self> {
        let post = boards::table.find(id).first::<Self>(conn)?;
        Ok(post)
    }
    pub fn get_topics(
        &self,
        conn: &mut MysqlConnection,
        limit: i32,
        offset: i32,
        include_hidden: bool,
    ) -> Result<Vec<Topic>> {
        let mut query = topics::table.into_boxed();
        query = query.filter(topics::board_id.eq(self.id));
        if !include_hidden {
            query = query.filter(topics::is_hidden.eq(false));
        }
        let topics = query
            .order_by(topics::is_pinned.desc())
            .then_order_by(topics::updated_at.desc())
            .limit(limit.into())
            .offset(offset.into())
            .load::<Topic>(conn)?;
        Ok(topics)
    }
    pub fn get_public(&self) -> BoardPublic {
        BoardPublic {
            id: self.id,
            display_name: self.display_name.clone(),
            name: self.name.clone(),
            is_active: self.is_active,
            sort_order: self.sort_order,
            icon: self.icon.clone(),
            write_permission: self.write_permission(),
            comment_permission: self.comment_permission(),
            is_listed: self.is_listed,
            separator_before: self.separator_before,
            created_at: self.created_at.and_utc(),
            updated_at: self.updated_at.and_utc(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Hash)]
#[serde(rename_all = "camelCase")]
pub struct BoardPublic {
    pub id: i32,
    pub display_name: String,
    pub name: String,
    pub is_active: bool,
    pub sort_order: i32,
    pub icon: Option<String>,
    pub write_permission: Permission,
    pub comment_permission: Permission,
    pub is_listed: bool,
    pub separator_before: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_name() {
        assert!(Board::is_valid_name("wiki"));
        assert!(Board::is_valid_name("dev_issue"));
        assert!(Board::is_valid_name("free-board2"));
        assert!(!Board::is_valid_name(""));
        assert!(!Board::is_valid_name("Wiki"));
        assert!(!Board::is_valid_name("2board"));
        assert!(!Board::is_valid_name("위키방"));
        assert!(!Board::is_valid_name("a/b"));
        assert!(!Board::is_valid_name("auth"));
    }

    #[test]
    fn test_is_valid_icon() {
        assert!(Board::is_valid_icon("chat-dots"));
        assert!(Board::is_valid_icon("1-circle"));
        assert!(Board::is_valid_icon("megaphone"));
        assert!(!Board::is_valid_icon(""));
        assert!(!Board::is_valid_icon("bi bi-x"));
        assert!(!Board::is_valid_icon("Chat"));
        assert!(!Board::is_valid_icon("-x"));
        assert!(!Board::is_valid_icon("x--y"));
        assert!(!Board::is_valid_icon("x\"><script>"));
    }

    #[test]
    fn test_permission() {
        assert!(Permission::All.allows(None));
        assert!(!Permission::Users.allows(None));
        assert!(Permission::Users.allows(Some(false)));
        assert!(!Permission::Admins.allows(Some(false)));
        assert!(Permission::Admins.allows(Some(true)));
        assert_eq!(Permission::parse("users"), Some(Permission::Users));
        assert_eq!(Permission::parse("nobody"), None);
        assert_eq!(Permission::Admins.as_str(), "admins");
    }

    #[test]
    fn test_board_admin() {
        use crate::db::create_connection;
        let mut conn = create_connection();
        conn.test_transaction::<_, anyhow::Error, _>(|conn| {
            let form = BoardForm {
                name: "zz-test-board".into(),
                display_name: "Test".into(),
                icon: Some("megaphone".into()),
                write_permission: "admins".into(),
                comment_permission: "users".into(),
                is_listed: false,
                is_active: true,
                separator_before: true,
            };
            let board = Board::create_with(conn, &form)?;
            let all = Board::get_all(conn)?;
            assert_eq!(
                all.last().map(|b| b.id),
                Some(board.id),
                "new board goes last"
            );
            assert_eq!(board.write_permission(), Permission::Admins);
            assert_eq!(board.comment_permission(), Permission::Users);
            assert_eq!(board.icon.as_deref(), Some("megaphone"));

            let mut ids: Vec<i32> = all.iter().map(|b| b.id).collect();
            ids.rotate_right(1);
            assert!(Board::reorder(conn, &ids)?);
            assert_eq!(Board::get_all(conn)?[0].id, board.id, "moved to the top");
            assert!(
                !Board::reorder(conn, &ids[1..])?,
                "every board must be listed"
            );

            let changed = BoardForm {
                icon: None,
                display_name: "Renamed".into(),
                ..form
            };
            Board::update(conn, board.id, &changed)?;
            let reloaded = Board::find_by_id(conn, board.id)?;
            assert_eq!(reloaded.icon, None);
            assert_eq!(reloaded.display_name, "Renamed");

            let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();
            Topic::create(conn, &reloaded, "t", None, None, &ip)?;
            assert_eq!(
                Board::delete(conn, board.id)?,
                Err(DeleteBoardError::HasTopics)
            );
            diesel::delete(topics::table.filter(topics::board_id.eq(board.id))).execute(conn)?;
            assert_eq!(Board::delete(conn, board.id)?, Ok(()));
            assert_eq!(
                Board::delete(conn, board.id)?,
                Err(DeleteBoardError::NotFound)
            );
            Ok(())
        });
    }
}
