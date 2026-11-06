-- Boards used by librewiki (bbs.librewiki.net).
-- These used to be inserted by migrations; fresh mediabbs installs start with
-- no boards. Run this after `mediabbs migrate` to reproduce the librewiki
-- setup (board ids 1-5 are expected by migrate_from_xe.sql):
--
--   mysql mediabbs < contrib/librewiki/seed_boards.sql
INSERT INTO boards (id, display_name, name) VALUES
    (1, '위키방', 'wiki'),
    (2, '자유게시판', 'freeboard'),
    (3, '익명게시판', 'anon'),
    (4, '조합게시판', 'coop'),
    (5, '이슈트래커', 'dev_issue');
