//! Topic and comment search.
//!
//! `LIKE '%term%'` on titles, comment contents and author names: MariaDB's
//! FULLTEXT parser splits on spaces only (no ngram parser), which is useless
//! for Korean. Every term must match (AND). Fine for a forum-sized database;
//! each search scans the tables.

use crate::models::{Comment, Topic, TopicPublic};
use crate::schema::{boards, comments, topics};
use anyhow::Result;
use chrono::{DateTime, Utc};
use diesel::mysql::Mysql;
use diesel::prelude::*;
use diesel::sql_types::Bool;
use std::collections::HashMap;
use std::net::IpAddr;

/// What to search in.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SearchField {
    /// Titles and comment contents.
    #[default]
    All,
    Title,
    Content,
    /// The author name (or the IP address of an anonymous author), exactly.
    Author,
}

pub struct SearchQuery<'a> {
    pub text: &'a str,
    pub field: SearchField,
    pub board_id: Option<i32>,
    pub limit: i64,
    pub offset: i64,
}

/// A comment that matched, with an excerpt around the first match.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommentHit {
    pub id: i32,
    pub author_name: String,
    pub created_at: DateTime<Utc>,
    pub snippet: String,
}

/// A topic whose title (or author) matched, or that has matching comments.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub topic: TopicPublic,
    pub board_name: String,
    pub board_display_name: String,
    pub title_match: bool,
    /// The number of matching comments; the first few are in `comments`.
    pub comment_matches: usize,
    pub comments: Vec<CommentHit>,
}

/// Comments shown per topic.
const COMMENTS_PER_HIT: usize = 3;
/// Characters in a snippet.
const SNIPPET_CHARS: usize = 140;
/// Only the start of a long comment is searched for the snippet.
const SNIPPET_SCAN_CHARS: usize = 20_000;

pub const MAX_QUERY_CHARS: usize = 100;
const MAX_TERMS: usize = 5;

/// The search terms of `text` (at most a few), or none if it is empty.
pub fn terms(text: &str) -> Vec<String> {
    text.split_whitespace()
        .take(MAX_TERMS)
        .map(str::to_owned)
        .collect()
}

/// `%term%` with LIKE wildcards escaped (`\` is the default escape character).
fn like_pattern(term: &str) -> String {
    let mut out = String::with_capacity(term.len() + 2);
    out.push('%');
    for c in term.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

fn ip_bytes(text: &str) -> Option<Vec<u8>> {
    match text.parse::<IpAddr>().ok()? {
        IpAddr::V4(ip) => Some(ip.octets().to_vec()),
        IpAddr::V6(ip) => Some(ip.octets().to_vec()),
    }
}

type TopicCond<'a> = Box<dyn BoxableExpression<topics::table, Mysql, SqlType = Bool> + 'a>;
type CommentCond<'a> = Box<dyn BoxableExpression<comments::table, Mysql, SqlType = Bool> + 'a>;

fn title_cond<'a>(terms: &[String]) -> TopicCond<'a> {
    let mut cond: TopicCond = Box::new(topics::id.eq(topics::id));
    for term in terms {
        cond = Box::new(cond.and(topics::title.like(like_pattern(term))));
    }
    cond
}

fn content_cond<'a>(terms: &[String]) -> CommentCond<'a> {
    let mut cond: CommentCond = Box::new(comments::is_hidden.eq(false));
    for term in terms {
        cond = Box::new(cond.and(comments::content.like(like_pattern(term))));
    }
    cond
}

fn topic_author_cond<'a>(name: &str) -> TopicCond<'a> {
    match ip_bytes(name) {
        Some(ip) => Box::new(topics::author_name.is_null().and(topics::author_ip.eq(ip))),
        None => Box::new(topics::author_name.eq(name.to_owned()).assume_not_null()),
    }
}

fn comment_author_cond<'a>(name: &str) -> CommentCond<'a> {
    let base = comments::is_hidden.eq(false);
    match ip_bytes(name) {
        Some(ip) => Box::new(
            base.and(comments::author_name.is_null())
                .and(comments::author_ip.eq(ip)),
        ),
        None => Box::new(base.and(comments::author_name.eq(name.to_owned()).assume_not_null())),
    }
}

/// Plain text of Markdown/HTML, good enough for an excerpt.
fn plain_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut in_tag = false;
    for c in markdown.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            '*' | '_' | '`' | '#' | '>' | '|' | '~' | '!' | '[' | ']' => out.push(' '),
            _ => out.push(c),
        }
    }
    // Drop link targets: "(https://...)".
    let mut cleaned = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(start) = rest.find("(http") {
        cleaned.push_str(&rest[..start]);
        rest = match rest[start..].find(')') {
            Some(end) => &rest[start + end + 1..],
            None => "",
        };
    }
    cleaned.push_str(rest);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// About `SNIPPET_CHARS` characters around the first occurrence of a term.
pub fn snippet(content: &str, terms: &[String]) -> String {
    let head = match content.char_indices().nth(SNIPPET_SCAN_CHARS * 2) {
        Some((end, _)) => &content[..end],
        None => content,
    };
    let text: Vec<char> = plain_text(head).chars().take(SNIPPET_SCAN_CHARS).collect();
    let lower: Vec<char> = text
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let position = terms
        .iter()
        .filter_map(|term| {
            let term: Vec<char> = term.chars().flat_map(char::to_lowercase).collect();
            if term.is_empty() || term.len() > lower.len() {
                return None;
            }
            (0..=lower.len() - term.len()).find(|&i| lower[i..i + term.len()] == term[..])
        })
        .min()
        .unwrap_or(0);
    let start = position.saturating_sub(SNIPPET_CHARS / 3);
    let end = (start + SNIPPET_CHARS).min(text.len());
    let mut out: String = text[start..end].iter().collect();
    if start > 0 {
        out.insert(0, '…');
    }
    if end < text.len() {
        out.push('…');
    }
    out
}

/// Newest activity first. Hidden topics and hidden comments are left out.
/// Returns at most `limit + 1` hits so the caller can tell whether more exist.
pub fn search(conn: &mut MysqlConnection, query: &SearchQuery) -> Result<Vec<SearchHit>> {
    let terms = terms(query.text);
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let author = query.text.trim();

    let comment_cond = || -> Option<CommentCond> {
        match query.field {
            SearchField::All | SearchField::Content => Some(content_cond(&terms)),
            SearchField::Author => Some(comment_author_cond(author)),
            SearchField::Title => None,
        }
    };
    let topic_cond = || -> Option<TopicCond> {
        match query.field {
            SearchField::All | SearchField::Title => Some(title_cond(&terms)),
            SearchField::Author => Some(topic_author_cond(author)),
            SearchField::Content => None,
        }
    };

    let mut matched: TopicCond = Box::new(topics::id.ne(topics::id));
    if let Some(cond) = topic_cond() {
        matched = Box::new(matched.or(cond));
    }
    if let Some(cond) = comment_cond() {
        let with_comments = comments::table
            .select(comments::topic_id)
            .filter(cond)
            .into_boxed();
        matched = Box::new(matched.or(topics::id.eq_any(with_comments)));
    }

    let mut q = topics::table
        .filter(topics::is_hidden.eq(false))
        .filter(matched)
        .into_boxed();
    if let Some(board_id) = query.board_id {
        q = q.filter(topics::board_id.eq(board_id));
    }
    let found: Vec<Topic> = q
        .order((topics::updated_at.desc(), topics::id.desc()))
        .limit(query.limit + 1)
        .offset(query.offset)
        .load(conn)?;
    if found.is_empty() {
        return Ok(vec![]);
    }

    let ids: Vec<i32> = found.iter().map(|t| t.id).collect();
    let mut by_topic: HashMap<i32, Vec<Comment>> = HashMap::new();
    if let Some(cond) = comment_cond() {
        let hits: Vec<Comment> = comments::table
            .filter(comments::topic_id.eq_any(&ids))
            .filter(cond)
            .order(comments::id.asc())
            .limit(1000)
            .load(conn)?;
        for comment in hits {
            by_topic.entry(comment.topic_id).or_default().push(comment);
        }
    }
    let title_ids: Vec<i32> = match topic_cond() {
        Some(cond) => topics::table
            .select(topics::id)
            .filter(topics::id.eq_any(&ids))
            .filter(cond)
            .load(conn)?,
        None => vec![],
    };
    let board_names: HashMap<i32, (String, String)> = boards::table
        .select((boards::id, boards::name, boards::display_name))
        .load::<(i32, String, String)>(conn)?
        .into_iter()
        .map(|(id, name, display)| (id, (name, display)))
        .collect();

    Ok(found
        .iter()
        .map(|topic| {
            let comments = by_topic.remove(&topic.id).unwrap_or_default();
            let (board_name, board_display_name) = board_names
                .get(&topic.board_id)
                .cloned()
                .unwrap_or_default();
            SearchHit {
                topic: topic.get_public(),
                board_name,
                board_display_name,
                title_match: title_ids.contains(&topic.id),
                comment_matches: comments.len(),
                comments: comments
                    .iter()
                    .take(COMMENTS_PER_HIT)
                    .map(|c| {
                        let public = c.get_public(false);
                        CommentHit {
                            id: c.id,
                            author_name: public.author_name,
                            created_at: public.created_at,
                            snippet: snippet(&c.content, &terms),
                        }
                    })
                    .collect(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_connection;
    use crate::models::Board;
    use std::str::FromStr;

    #[test]
    fn test_like_pattern() {
        assert_eq!(like_pattern("a%b_c\\"), "%a\\%b\\_c\\\\%");
    }

    #[test]
    fn test_snippet() {
        let long = format!(
            "{} 찾는말 **굵게** [링크](https://example.org/x) {}",
            "가".repeat(100),
            "나".repeat(200)
        );
        let s = snippet(&long, &["찾는말".to_owned()]);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.contains("찾는말 굵게 링크 나"));
        assert!(!s.contains("https"));
        assert_eq!(
            snippet("<b>Hello</b> World", &["world".to_owned()]),
            "Hello World"
        );
    }

    #[test]
    fn test_search() {
        let mut conn = create_connection();
        conn.test_transaction::<_, anyhow::Error, _>(|conn| {
            let ip = IpAddr::from_str("10.9.8.7")?;
            Board::create(conn, "zz-search", "Search test")?;
            let board = Board::find_by_name(conn, "zz-search")?;
            let q = |text: &'static str, field| SearchQuery {
                text,
                field,
                board_id: Some(board.id),
                limit: 10,
                offset: 0,
            };

            Topic::create(conn, &board, "사과 이야기", Some(5), Some("Alice"), &ip)?;
            let apple = Topic::get_latest(conn)?;
            Comment::create(
                conn,
                &apple,
                "빨간 사과는 맛있다",
                Some(5),
                Some("Alice"),
                &ip,
            )?;
            Topic::create(conn, &board, "바나나", None, None, &ip)?;
            let banana = Topic::get_latest(conn)?;
            Comment::create(
                conn,
                &banana,
                "노란 바나나와 빨간 사과 100%",
                None,
                None,
                &ip,
            )?;
            Comment::create(conn, &banana, "숨길 사과", None, None, &ip)?;
            let hidden = Comment::get_all(conn, 1, 0)?.remove(0);
            diesel::update(comments::table.find(hidden.id))
                .set(comments::is_hidden.eq(true))
                .execute(conn)?;

            let all = search(conn, &q("사과", SearchField::All))?;
            assert_eq!(all.len(), 2);
            let b = all.iter().find(|h| h.topic.id == banana.id).unwrap();
            assert!(!b.title_match);
            assert_eq!(b.comment_matches, 1, "hidden comments are left out");
            assert!(b.comments[0].snippet.contains("빨간 사과"));
            let a = all.iter().find(|h| h.topic.id == apple.id).unwrap();
            assert!(a.title_match);

            let titles = search(conn, &q("사과", SearchField::Title))?;
            assert_eq!(titles.len(), 1);
            assert!(titles[0].comments.is_empty());

            let both = search(conn, &q("빨간  바나나", SearchField::Content))?;
            assert_eq!(both.len(), 1, "every term must match");
            assert_eq!(both[0].topic.id, banana.id);

            assert_eq!(search(conn, &q("100%", SearchField::All))?.len(), 1);
            assert_eq!(search(conn, &q("10_", SearchField::All))?.len(), 0);

            let alice = search(conn, &q("alice", SearchField::Author))?;
            assert_eq!(alice.len(), 1);
            assert!(alice[0].title_match && alice[0].comment_matches == 1);
            let anon = search(conn, &q("10.9.8.7", SearchField::Author))?;
            assert_eq!(anon.len(), 1);
            assert_eq!(anon[0].topic.id, banana.id);

            diesel::update(topics::table.find(apple.id))
                .set(topics::is_hidden.eq(true))
                .execute(conn)?;
            assert!(search(conn, &q("사과", SearchField::Title))?.is_empty());
            assert!(search(conn, &q("   ", SearchField::All))?.is_empty());
            Ok(())
        });
    }
}
