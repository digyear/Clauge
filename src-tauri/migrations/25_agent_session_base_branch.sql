-- Persist the branch that a manual Agent session should branch from.
-- `worktree_branch` stores the user-editable working branch name even
-- before the lazy worktree creation happens.
ALTER TABLE agent_sessions ADD COLUMN base_branch TEXT;
