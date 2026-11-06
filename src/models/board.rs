use crate::models::Topic;
use crate::schema::{boards, topics};
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;

#[derive(Serialize, Deserialize, Queryable, Identifiable, Debug)]
pub struct Board {
    pub id: i32,
    pub display_name: String,
    pub name: String,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl Board {
    pub fn get_all(conn: &MysqlConnection) -> Result<Vec<Self>> {
        let results = boards::table.load::<Self>(conn)?;
        Ok(results)
    }
    pub fn find_by_name(conn: &MysqlConnection, name: &str) -> Result<Self> {
        let board = boards::table
            .filter(boards::name.eq(name))
            .first::<Self>(conn)?;
        Ok(board)
    }
    pub fn create(conn: &MysqlConnection, name: &str, display_name: &str) -> Result<()> {
        diesel::insert_into(boards::table)
            .values((boards::name.eq(name), boards::display_name.eq(display_name)))
            .execute(conn)?;
        Ok(())
    }
    pub fn set_display_name(
        conn: &MysqlConnection,
        name: &str,
        display_name: &str,
    ) -> Result<usize> {
        let n = diesel::update(boards::table.filter(boards::name.eq(name)))
            .set(boards::display_name.eq(display_name))
            .execute(conn)?;
        Ok(n)
    }
    pub fn set_active(conn: &MysqlConnection, name: &str, is_active: bool) -> Result<usize> {
        let n = diesel::update(boards::table.filter(boards::name.eq(name)))
            .set(boards::is_active.eq(is_active))
            .execute(conn)?;
        Ok(n)
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
    pub fn find_by_id(conn: &MysqlConnection, id: i32) -> Result<Self> {
        let post = boards::table.find(id).first::<Self>(conn)?;
        Ok(post)
    }
    pub fn get_topics(
        &self,
        conn: &MysqlConnection,
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
            created_at: DateTime::<Utc>::from_utc(self.created_at, Utc),
            updated_at: DateTime::<Utc>::from_utc(self.updated_at, Utc),
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
}
