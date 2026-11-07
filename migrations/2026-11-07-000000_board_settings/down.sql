DELETE FROM logs WHERE log_type_id BETWEEN 11 AND 15;
DELETE FROM log_types WHERE id BETWEEN 11 AND 15;
DROP TABLE site_settings;
ALTER TABLE boards
    DROP COLUMN separator_before,
    DROP COLUMN is_listed,
    DROP COLUMN comment_permission,
    DROP COLUMN write_permission,
    DROP COLUMN icon,
    DROP COLUMN sort_order;
