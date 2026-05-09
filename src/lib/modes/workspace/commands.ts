import { invoke } from '@tauri-apps/api/core';
import type {
  InboxItem,
  ProjectScanResult,
  Workspace,
  WorkspaceBoard,
  WorkspaceBoardCard,
  WorkspaceBoardColumn,
  WorkspaceCardComment,
  WorkspaceNote,
} from './types';

// ── Workspaces ────────────────────────────────────────────────────────

export const workspaceList = () => invoke<Workspace[]>('workspace_list');
export const workspaceGet = (id: string) => invoke<Workspace>('workspace_get', { id });
export const workspaceCreate = (params: {
  name: string;
  projectPath?: string | null;
  color?: string | null;
  actor: string;
}) => invoke<Workspace>('workspace_create', params);
export const workspaceUpdate = (params: {
  id: string;
  name: string;
  projectPath?: string | null;
  color?: string | null;
  actor: string;
}) => invoke<void>('workspace_update', params);
export const workspaceDelete = (id: string) => invoke<void>('workspace_delete', { id });

// ── Notes ─────────────────────────────────────────────────────────────

export const workspaceNoteList = (workspaceId: string) =>
  invoke<WorkspaceNote[]>('workspace_note_list', { workspaceId });
export const workspaceNoteGet = (id: string) => invoke<WorkspaceNote>('workspace_note_get', { id });
export const workspaceNoteCreate = (params: {
  workspaceId: string;
  title: string;
  content?: string;
  tags?: string[];
  linkedSessionId?: string | null;
  actor: string;
}) => invoke<WorkspaceNote>('workspace_note_create', params);
export const workspaceNoteUpdate = (params: {
  id: string;
  title: string;
  content: string;
  tags: string[];
  linkedSessionId?: string | null;
  actor: string;
}) => invoke<void>('workspace_note_update', params);
export const workspaceNoteDelete = (id: string) => invoke<void>('workspace_note_delete', { id });

// ── Boards + columns ──────────────────────────────────────────────────

export const workspaceBoardList = (workspaceId: string) =>
  invoke<WorkspaceBoard[]>('workspace_board_list', { workspaceId });
export const workspaceBoardGet = (id: string) => invoke<WorkspaceBoard>('workspace_board_get', { id });
export const workspaceBoardCreate = (workspaceId: string, name: string) =>
  invoke<WorkspaceBoard>('workspace_board_create', { workspaceId, name });
export const workspaceBoardRename = (id: string, name: string) =>
  invoke<void>('workspace_board_rename', { id, name });
export const workspaceBoardSetProject = (
  id: string,
  projectPath: string | null,
  projectUrl: string | null,
) => invoke<void>('workspace_board_set_project', { id, projectPath, projectUrl });
export const workspaceBoardDelete = (id: string) => invoke<void>('workspace_board_delete', { id });

export const workspaceColumnList = (boardId: string) =>
  invoke<WorkspaceBoardColumn[]>('workspace_column_list', { boardId });

// ── Cards ─────────────────────────────────────────────────────────────

export const workspaceCardList = (boardId: string) =>
  invoke<WorkspaceBoardCard[]>('workspace_card_list', { boardId });
export const workspaceCardCreate = (params: {
  columnId: string;
  title: string;
  description?: string;
  priority?: string | null;
  tags?: string[];
  position?: number;
  externalId?: string | null;
  externalUrl?: string | null;
  linkedSessionId?: string | null;
  actor: string;
}) => invoke<WorkspaceBoardCard>('workspace_card_create', params);
export const workspaceCardUpdate = (params: {
  id: string;
  title: string;
  description: string;
  priority?: string | null;
  tags: string[];
  reviewChecklist?: string | null;
  actor: string;
}) => invoke<void>('workspace_card_update', params);
export const workspaceCardMove = (params: {
  id: string;
  columnId: string;
  position: number;
  actor: string;
}) => invoke<void>('workspace_card_move', params);
export const workspaceCardClearReview = (id: string, actor: string) =>
  invoke<void>('workspace_card_clear_review', { id, actor });
export const workspaceCardDelete = (id: string) => invoke<void>('workspace_card_delete', { id });
export const workspaceCardAddComment = (id: string, body: string, actor: string) =>
  invoke<WorkspaceCardComment>('workspace_card_add_comment', { id, body, actor });
export const workspaceCardCommentList = (cardId: string) =>
  invoke<WorkspaceCardComment[]>('workspace_card_comment_list', { cardId });
export const workspaceCardCommentDelete = (id: string) =>
  invoke<void>('workspace_card_comment_delete', { id });

export interface CardPushResult {
  id: string;
  externalId: string;
  externalUrl: string;
  source: 'github' | 'gitlab' | string;
}
export const workspaceCardPushToRepo = (id: string, actor: string) =>
  invoke<CardPushResult>('workspace_card_push_to_repo', { id, actor });

export const workspaceCardSetLinkedSession = (
  id: string,
  sessionId: string | null,
  actor: string,
) =>
  invoke<void>('workspace_card_set_linked_session', { id, sessionId, actor });

export interface CardMentionResult {
  ok: true;
  sessionId: string;
  provider: string;
  response: string;
  userCommentId?: string;
  replyCommentId?: string;
}
export const workspaceCardMentionSession = (id: string, body: string, actor: string) =>
  invoke<CardMentionResult>('workspace_card_mention_session', { id, body, actor });

// ── Inbox ────────────────────────────────────────────────────────────

export const workspaceInboxList = (limit?: number) =>
  invoke<InboxItem[]>('workspace_inbox_list', { limit });

// ── MCP server lifecycle ─────────────────────────────────────────────

export interface McpStatus { running: boolean; port: number | null; }

export const workspaceMcpStatus = () =>
  invoke<McpStatus>('workspace_mcp_status');
export const workspaceMcpStart = (port: number, token: string) =>
  invoke<McpStatus>('workspace_mcp_start', { port, token });
export const workspaceMcpStop = () =>
  invoke<McpStatus>('workspace_mcp_stop');
/** `agent` defaults to `'claude-code'` server-side. Pass `'codex'`,
 *  `'gemini'`, or `'opencode'` once those arms land in Rust. */
export const workspaceMcpRegister = (port: number, token: string, agent?: string) =>
  invoke<void>('workspace_mcp_register', { agent, port, token });
export const workspaceMcpUnregister = (agent?: string) =>
  invoke<void>('workspace_mcp_unregister', { agent });
export const workspaceMcpNewToken = () =>
  invoke<string>('workspace_mcp_new_token');

// ── Project issue scan ───────────────────────────────────────────────

export const workspaceScanProjectIssues = (projectPath: string) =>
  invoke<ProjectScanResult>('workspace_scan_project_issues', { projectPath });

export const workspaceScanProjectIssuesByUrl = (projectUrl: string) =>
  invoke<ProjectScanResult>('workspace_scan_project_issues_by_url', { projectUrl });
