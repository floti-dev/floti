-- The browser's history, for the address bar's suggestions. It replaces the `browserRecentUrls` setting: its addresses (newest first) become the first rows, a millisecond apart so their order is kept, and the setting goes.
CREATE TABLE `browser_history` (
	`url` text PRIMARY KEY NOT NULL,
	`title` text DEFAULT '' NOT NULL,
	`visits` integer DEFAULT 1 NOT NULL,
	`last_visit` integer NOT NULL
);
--> statement-breakpoint
CREATE INDEX `browser_history_last_visit_idx` ON `browser_history` (`last_visit`);--> statement-breakpoint
INSERT OR IGNORE INTO `browser_history` (`url`, `title`, `visits`, `last_visit`)
SELECT `recent`.`value`, '', 1, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) - `recent`.`key`
FROM `app_settings`, json_each(`app_settings`.`value`) AS `recent`
WHERE `app_settings`.`key` = 'browserRecentUrls' AND json_valid(`app_settings`.`value`) AND json_type(`app_settings`.`value`) = 'array'
AND `recent`.`type` = 'text' AND (`recent`.`value` LIKE 'http://%' OR `recent`.`value` LIKE 'https://%');--> statement-breakpoint
DELETE FROM `app_settings` WHERE `key` = 'browserRecentUrls';
