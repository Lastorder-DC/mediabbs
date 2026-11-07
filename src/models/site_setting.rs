//! Site-wide settings edited from the admin UI, stored as JSON text.

use crate::schema::site_settings;
use anyhow::Result;
use diesel::prelude::*;

pub struct SiteSetting;

impl SiteSetting {
    /// Navigation menus per skin (`{"<skin>|default": [menu items]}`).
    pub const MENUS: &'static str = "menus";

    pub fn get(conn: &mut MysqlConnection, name: &str) -> Result<Option<String>> {
        let value = site_settings::table
            .find(name)
            .select(site_settings::value)
            .first::<String>(conn)
            .optional()?;
        Ok(value)
    }

    pub fn set(conn: &mut MysqlConnection, name: &str, value: &str) -> Result<()> {
        diesel::replace_into(site_settings::table)
            .values((site_settings::name.eq(name), site_settings::value.eq(value)))
            .execute(conn)?;
        Ok(())
    }

    pub fn delete(conn: &mut MysqlConnection, name: &str) -> Result<()> {
        diesel::delete(site_settings::table.find(name)).execute(conn)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_connection;

    #[test]
    fn test_site_setting() {
        let mut conn = create_connection();
        conn.test_transaction::<_, anyhow::Error, _>(|conn| {
            SiteSetting::delete(conn, "zz-test")?;
            assert_eq!(SiteSetting::get(conn, "zz-test")?, None);
            SiteSetting::set(conn, "zz-test", "[1]")?;
            SiteSetting::set(conn, "zz-test", "[2]")?;
            assert_eq!(SiteSetting::get(conn, "zz-test")?.as_deref(), Some("[2]"));
            SiteSetting::delete(conn, "zz-test")?;
            assert_eq!(SiteSetting::get(conn, "zz-test")?, None);
            Ok(())
        });
    }
}
