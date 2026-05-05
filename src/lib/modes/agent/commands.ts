import { invoke } from '@tauri-apps/api/core';
import type {
  AgentSession,
  AgentContext,
  DiscoveredSession,
  ContextUsage,
  TokenUsage,
  GitFileChange,
  UsageAnalytics,
  ClaudePlugin,
  MarketplacePlugin,
} from './types';

// Session CRUD
export const agentListSessions = () => invoke<AgentSession[]>('agent_list_sessions');
export const agentCreateSession = (params: {
  title: string;
  purpose: string;
  projectPath: string;
  skipPermissions?: boolean;
  customPrompt?: string;
  gitName?: string;
  gitEmail?: string;
}) => invoke<AgentSession>('agent_create_session', params);
export const agentUpdateSession = (params: {
  id: string;
  title?: string;
  skipPermissions?: boolean;
  gitName?: string;
  gitEmail?: string;
  contextPrompt?: string;
}) => invoke<void>('agent_update_session', params);
export const agentDeleteSession = (id: string) => invoke<void>('agent_delete_session', { id });
export const agentUpdateSessionId = (id: string, claudeSessionId: string) => invoke<void>('agent_update_session_id', { id, claudeSessionId });
export const agentUpdateLastUsed = (id: string) => invoke<void>('agent_update_last_used', { id });
export const agentUpdateWorktree = (id: string, worktreePath: string | null, worktreeBranch: string | null) => invoke<void>('agent_update_worktree', { id, worktreePath, worktreeBranch });

// Context CRUD
export const agentListContexts = () => invoke<AgentContext[]>('agent_list_contexts');
export const agentSaveContext = (params: { id?: string; name: string; content: string }) => invoke<AgentContext>('agent_save_context', params);
export const agentDeleteContext = (id: string) => invoke<void>('agent_delete_context', { id });
export const agentGetSessionContexts = (sessionId: string) => invoke<AgentContext[]>('agent_get_session_contexts', { sessionId });
export const agentAttachContext = (sessionId: string, contextId: string) => invoke<void>('agent_attach_context', { sessionId, contextId });
export const agentDetachContext = (sessionId: string, contextId: string) => invoke<void>('agent_detach_context', { sessionId, contextId });
export const agentInjectContexts = (projectPath: string, contextIds: string[]) => invoke<void>('agent_inject_contexts', { projectPath, contextIds });
export const agentRemoveInjectedContexts = (projectPath: string) => invoke<void>('agent_remove_injected_contexts', { projectPath });

// Terminal
export const agentSpawnTerminal = (params: {
  sessionId?: string;
  projectPath: string;
  contextPrompt?: string;
  skipPermissions?: boolean;
  gitName?: string;
  gitEmail?: string;
  onOutput: any;
}) => invoke<string>('agent_spawn_terminal', params);
export const agentSpawnShell = (projectPath: string, onOutput: any) => invoke<string>('agent_spawn_shell', { projectPath, onOutput });
export const agentWriteToTerminal = (terminalId: string, data: string) => invoke<void>('agent_write_to_terminal', { terminalId, data });
export const agentResizeTerminal = (terminalId: string, cols: number, rows: number) => invoke<void>('agent_resize_terminal', { terminalId, cols, rows });
export const agentKillTerminal = (terminalId: string) => invoke<void>('agent_kill_terminal', { terminalId });

// Worktree
export const agentIsGitRepo = (path: string) => invoke<boolean>('agent_is_git_repo', { path });
export const agentCreateWorktree = (projectPath: string, branchName: string) => invoke<string>('agent_create_worktree', { projectPath, branchName });
export const agentRemoveWorktree = (projectPath: string, worktreePath: string) => invoke<void>('agent_remove_worktree', { projectPath, worktreePath });

// Git — all use projectPath (camelCase for Tauri v2 auto-conversion to project_path)
export const agentGitStatus = (projectPath: string) => invoke<GitFileChange[]>('agent_git_status', { projectPath });
export const agentGitBranch = (projectPath: string) => invoke<string>('agent_git_branch', { projectPath });
export const agentGitAheadBehind = (projectPath: string) => invoke<[number, number]>('agent_git_ahead_behind', { projectPath });
export const agentGitCommit = (projectPath: string, message: string) => invoke<string>('agent_git_commit', { projectPath, message });
export const agentGitPush = (projectPath: string) => invoke<string>('agent_git_push', { projectPath });
export const agentGitPull = (projectPath: string) => invoke<string>('agent_git_pull', { projectPath });
export const agentGitDiffFile = (projectPath: string, filePath: string) => invoke<string>('agent_git_diff_file', { projectPath, filePath });
export const agentGitStageFile = (projectPath: string, filePath: string) => invoke<void>('agent_git_stage_file', { projectPath, filePath });
export const agentGitUnstageFile = (projectPath: string, filePath: string) => invoke<void>('agent_git_unstage_file', { projectPath, filePath });
export const agentGitLog = (projectPath: string, limit?: number) => invoke<any[]>('agent_git_log', { projectPath, limit });
export const agentGitStash = (projectPath: string) => invoke<string>('agent_git_stash', { projectPath });
export const agentGitStashPop = (projectPath: string) => invoke<string>('agent_git_stash_pop', { projectPath });
export const agentGitListBranches = (projectPath: string) => invoke<any[]>('agent_git_list_branches', { projectPath });
export const agentGitSwitchBranch = (projectPath: string, branchName: string) => invoke<void>('agent_git_switch_branch', { projectPath, branchName });

// Plugins
export const agentGetPlugins = () => invoke<ClaudePlugin[]>('agent_get_plugins');
export const agentTogglePlugin = (pluginKey: string, enabled: boolean) => invoke<void>('agent_toggle_plugin', { pluginKey, enabled });
export const agentGetMarketplacePlugins = () => invoke<MarketplacePlugin[]>('agent_get_marketplace_plugins');
export const agentInstallPlugin = (name: string, marketplace: string) => invoke<void>('agent_install_plugin', { name, marketplace });
export const agentUninstallPlugin = (name: string, marketplace: string) => invoke<void>('agent_uninstall_plugin', { name, marketplace });

// Usage
export const agentGetUsageAnalytics = (days?: number) => invoke<UsageAnalytics>('agent_get_usage_analytics', { days });
export const agentFetchUsageLimits = (sessionKey: string) => invoke<any>('agent_fetch_usage_limits', { sessionKey });
export const agentDiscoverSessions = (projectPath: string) => invoke<DiscoveredSession[]>('agent_discover_sessions', { projectPath });
export const agentGetSessionTokens = (projectPath: string, sessionId?: string) => invoke<TokenUsage>('agent_get_session_tokens', { projectPath, sessionId });
export const agentGetSessionContextUsage = (projectPath: string, sessionId: string) => invoke<ContextUsage>('agent_get_session_context_usage', { projectPath, sessionId });

// System
export const agentUpdateTrayTitle = (title: string) => invoke<void>('agent_update_tray_title', { title });
export const agentGetClaudePlan = () => invoke<string>('agent_get_claude_plan');
export const agentCheckClaudeInstalled = () => invoke<boolean>('agent_check_claude_installed');
