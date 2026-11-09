ALTER TABLE `sessions` DROP COLUMN `child_class`;--> statement-breakpoint
-- A limit notice the heavy limit left refers to a limit there no longer is.
DELETE FROM `notices` WHERE `limit_kind` = 'heavy';