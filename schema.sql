CREATE TABLE IF NOT EXISTS anime (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    keywords VARCHAR(128) NOT NULL,
    exclude_keywords VARCHAR(128) NOT NULL DEFAULT '',
    indexer INTEGER
);
INSERT INTO anime (keywords, indexer) VALUES ("Doomdos 凡人修仙传 1080", 5);