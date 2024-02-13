// @generated automatically by Diesel CLI.

diesel::table! {
    media_lists (id) {
        id -> Integer,
        title -> Nullable<Text>,
        artist -> Nullable<Text>,
        media_ids -> Text,
        url -> Text,
        add_timestamp -> Timestamp,
        total_duration -> Integer,
    }
}

diesel::table! {
    medias (id) {
        id -> Integer,
        title -> Text,
        artist -> Text,
        duration -> Nullable<Integer>,
        url -> Text,
        add_timestamp -> Timestamp,
        media_type -> Text,
        views -> Integer,
        alt_title -> Nullable<Text>,
        alt_artist -> Nullable<Text>,
    }
}

diesel::table! {
    playlist_items (id) {
        id -> Integer,
        playlist_id -> Integer,
        media_id -> Integer,
        prev -> Nullable<Integer>,
        next -> Nullable<Integer>,
        add_timestamp -> Timestamp,
    }
}

diesel::table! {
    playlists (id) {
        id -> Integer,
        title -> Text,
        first_playlist_item -> Nullable<Integer>,
        last_playlist_item -> Nullable<Integer>,
        add_timestamp -> Timestamp,
        current_item -> Nullable<Integer>,
        num_items -> Integer,
        total_duration -> Integer,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    media_lists,
    medias,
    playlist_items,
    playlists,
);
