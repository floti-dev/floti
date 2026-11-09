-- Roles are called agents: a session's `role` is its `agent`, and every row keeps its value.
ALTER TABLE `sessions` RENAME COLUMN `role` TO `agent`;
