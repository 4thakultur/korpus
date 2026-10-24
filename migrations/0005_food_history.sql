-- The Foods page lists foods recently logged in the diary. Clearing that
-- history only records when it was cleared: diary entries logged at or
-- before this UTC timestamp are left out of the list, nothing is deleted.
ALTER TABLE profile ADD COLUMN history_cleared_at TEXT;
