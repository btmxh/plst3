CREATE TABLE medias (
  id INTEGER NOT NULL PRIMARY KEY,
  title TEXT NOT NULL,
  artist TEXT NOT NULL,
  duration INTEGER,
  url TEXT NOT NULL,
  add_timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  media_type TEXT NOT NULL
);

CREATE TABLE media_lists(
  id INTEGER NOT NULL PRIMARY KEY,
  title TEXT,
  artist TEXT,
  media_ids TEXT NOT NULL,
  url TEXT NOT NULL,
  add_timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  total_duration INTEGER NOT NULL
);

CREATE TABLE playlist_items(
  id INTEGER NOT NULL PRIMARY KEY,
  playlist_id INTEGER NOT NULL,
  media_id INTEGER NOT NULL,
  prev INTEGER,
  next INTEGER,
  add_timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE playlists(
  id INTEGER NOT NULL PRIMARY KEY,
  title TEXT NOT NULL,
  first_playlist_item INTEGER,
  last_playlist_item INTEGER,
  add_timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  current_item INTEGER,
  num_items INTEGER NOT NULL DEFAULT 0,
  total_duration INTEGER NOT NULL DEFAULT 0
);
