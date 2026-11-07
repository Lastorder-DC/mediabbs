-- Board settings managed from the admin UI: order, icon, who may write,
-- whether the board is listed in the menu, and a separator above it.
ALTER TABLE boards
    ADD COLUMN sort_order INT NOT NULL DEFAULT 0 AFTER is_active,
    ADD COLUMN icon VARCHAR(64) NULL AFTER sort_order,
    ADD COLUMN write_permission VARCHAR(16) NOT NULL DEFAULT 'all' AFTER icon,
    ADD COLUMN comment_permission VARCHAR(16) NOT NULL DEFAULT 'all' AFTER write_permission,
    ADD COLUMN is_listed BOOLEAN NOT NULL DEFAULT TRUE AFTER comment_permission,
    ADD COLUMN separator_before BOOLEAN NOT NULL DEFAULT FALSE AFTER is_listed;
UPDATE boards SET sort_order = id;

-- Site-wide settings edited from the admin UI (JSON values), e.g. `menus`.
CREATE TABLE site_settings (
    name VARCHAR(64) PRIMARY KEY,
    value MEDIUMTEXT NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT current_timestamp ON UPDATE current_timestamp
);

INSERT INTO log_types (id, name) VALUES (11, "CREATE_BOARD"),
                                        (12, "UPDATE_BOARD"),
                                        (13, "DELETE_BOARD"),
                                        (14, "REORDER_BOARDS"),
                                        (15, "UPDATE_MENUS");
