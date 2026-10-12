-- The stored values of the facets with a fixed vocabulary are lowercase keys
-- (`no`, `desktop`, `chrome`, `macos`): what is shown for them is the
-- frontend's business, and a filter on them is not case sensitive. Rows
-- written before this had `NO`, `Chrome`, `macOS`; only those are rewritten.
-- (The `DEFAULT 'Other'` of browser and os in the first migration is never
-- used: every insert gives both. SQLite cannot change a default in place.)
UPDATE hits SET country = lower(country) WHERE country IS NOT NULL AND country <> lower(country);
UPDATE hits SET device = lower(device) WHERE device <> lower(device);
UPDATE hits SET browser = lower(browser) WHERE browser <> lower(browser);
UPDATE hits SET os = lower(os) WHERE os <> lower(os);
