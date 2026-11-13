-- Harnesses are called workspaces: a project's `harness_path` is its `workspace_path`, its index is renamed with it, and a workspace's row says `workspace`. Nothing in the user's folders is touched.
ALTER TABLE `projects` RENAME COLUMN `harness_path` TO `workspace_path`;--> statement-breakpoint
DROP INDEX IF EXISTS `projects_harness_idx`;--> statement-breakpoint
CREATE INDEX `projects_workspace_idx` ON `projects` (`workspace_path`);--> statement-breakpoint
UPDATE `projects` SET `kind` = 'workspace' WHERE `kind` = 'harness';
