CREATE TABLE `agent_versions` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`folder` text NOT NULL,
	`name` text NOT NULL,
	`file` text NOT NULL,
	`text` text,
	`after_hash` text,
	`renamed_to` text,
	`renamed_file` text,
	`renamed_hash` text,
	`reason` text NOT NULL,
	`at` text DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')) NOT NULL
);
--> statement-breakpoint
CREATE INDEX `agent_versions_name_idx` ON `agent_versions` (`folder`,`name`);--> statement-breakpoint
CREATE INDEX `agent_versions_renamed_idx` ON `agent_versions` (`folder`,`renamed_to`);