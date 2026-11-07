diesel::table! {
    boards (id) {
        id -> Integer,
        display_name -> Varchar,
        name -> Varchar,
        is_active -> Bool,
        sort_order -> Integer,
        icon -> Nullable<Varchar>,
        write_permission -> Varchar,
        comment_permission -> Varchar,
        is_listed -> Bool,
        separator_before -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    site_settings (name) {
        name -> Varchar,
        value -> Mediumtext,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    comments (id) {
        id -> Integer,
        topic_id -> Integer,
        content -> Mediumtext,
        author_id -> Nullable<Integer>,
        author_name -> Nullable<Varchar>,
        author_ip -> Varbinary,
        is_hidden -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    logs (id) {
        id -> Integer,
        log_type_id -> Integer,
        content -> Varchar,
        user_id -> Nullable<Integer>,
        user_name -> Nullable<Varchar>,
        user_ip -> Varbinary,
        created_at -> Timestamp,
    }
}

diesel::table! {
    log_types (id) {
        id -> Integer,
        name -> Nullable<Varchar>,
    }
}

diesel::table! {
    topics (id) {
        id -> Integer,
        board_id -> Integer,
        title -> Varchar,
        author_id -> Nullable<Integer>,
        author_name -> Nullable<Varchar>,
        author_ip -> Varbinary,
        is_closed -> Bool,
        is_suspended -> Bool,
        is_hidden -> Bool,
        is_pinned -> Bool,
        comment_count -> Integer,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::joinable!(comments -> topics (topic_id));
diesel::joinable!(logs -> log_types (log_type_id));
diesel::joinable!(topics -> boards (board_id));

diesel::allow_tables_to_appear_in_same_query!(
    boards,
    comments,
    logs,
    log_types,
    site_settings,
    topics,
);
