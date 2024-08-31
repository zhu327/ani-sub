CREATE TABLE IF NOT EXISTS anime (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    keywords VARCHAR(128) NOT NULL,
    exclude_keywords VARCHAR(128) DEFAULT '',
    indexer INTEGER DEFAULT 0
);
INSERT INTO anime (keywords) VALUES ("LoliHouse 鬼灭之刃 柱训练篇");