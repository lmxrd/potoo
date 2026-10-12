-- The time format is one flag — whether times are 24-hour — not one of
-- two strings. The flag is copied over from the text column it replaces
-- (SQLite cannot change a column's type in place), which is then dropped.
ALTER TABLE users ADD COLUMN use_24 INTEGER NOT NULL DEFAULT 1;
UPDATE users SET use_24 = CASE WHEN time_format = '12h' THEN 0 ELSE 1 END;
ALTER TABLE users DROP COLUMN time_format;
