-- Your SQL goes here
CREATE TABLE boards (
    id INT PRIMARY KEY AUTO_INCREMENT,
    display_name VARCHAR(255) NOT NULL,
    name VARCHAR(255) UNIQUE NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT current_timestamp,
    updated_at TIMESTAMP NOT NULL DEFAULT current_timestamp ON UPDATE current_timestamp,
    INDEX (created_at),
    INDEX (updated_at)
);

-- Boards are created with `mediabbs board add` (librewiki's initial boards
-- moved to contrib/librewiki/seed_boards.sql).
