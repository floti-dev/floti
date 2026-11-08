CREATE TABLE `agent_messages` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`child_session_id` integer NOT NULL,
	`parent_session_id` integer NOT NULL,
	`text` text NOT NULL,
	`interrupt` integer DEFAULT false NOT NULL,
	`at` text DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')) NOT NULL,
	`delivered_at` text,
	`dropped_at` text
);
--> statement-breakpoint
CREATE INDEX `agent_messages_child_idx` ON `agent_messages` (`child_session_id`,`delivered_at`,`dropped_at`);