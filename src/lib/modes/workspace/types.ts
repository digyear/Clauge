// Workspace mode types — mirror of the Rust models in
// `src-tauri/src/modes/workspace/models.rs`. Camel-cased because the
// Rust serde annotations rename on the boundary.

export interface Workspace {
  id: string;
  name: string;
  projectPath: string | null;
  projectName: string | null;
  color: string | null;
  createdAt: string;
  createdBy: string;
  updatedAt: string;
  updatedBy: string;
  /** Workspace-level GitHub/GitLab URL — added in migration 12.
   *  Used as the agent's default remote when no per-board override
   *  is set. */
  repoUrl: string | null;
}

export interface WorkspaceNote {
  id: string;
  workspaceId: string;
  title: string;
  content: string;
  /** JSON-encoded `string[]` on the wire — parsed by the store helper. */
  tags: string;
  linkedSessionId: string | null;
  createdAt: string;
  createdBy: string;
  updatedAt: string;
  updatedBy: string;
  /** `1` = blocked from agent edits; UI edits allowed. */
  frozen: number;
}

export interface WorkspaceBoard {
  id: string;
  workspaceId: string;
  name: string;
  /** `'manual' | 'github_issues'` etc. Always `'manual'` in v1. */
  source: string;
  sourceConfig: string | null;
  position: number;
  createdAt: string;
  updatedAt: string;
}

export interface WorkspaceBoardColumn {
  id: string;
  boardId: string;
  name: string;
  color: string | null;
  position: number;
  createdAt: string;
}

export interface WorkspaceBoardCard {
  id: string;
  columnId: string;
  title: string;
  description: string;
  priority: string | null;
  /** JSON-encoded `string[]` on the wire — parsed by the store helper. */
  tags: string;
  position: number;
  externalId: string | null;
  externalUrl: string | null;
  linkedSessionId: string | null;
  /** `1` when an agent moved this card into a Review-class column. */
  reviewPending: number;
  reviewChecklist: string | null;
  createdAt: string;
  createdBy: string;
  updatedAt: string;
  updatedBy: string;
  /** `1` = blocked from agent edits. */
  frozen: number;
}

/** Card comment row — added in migration 13. Replaces the markdown-
 *  blockquote-in-description pattern used through v12. */
export interface WorkspaceCardComment {
  id: string;
  cardId: string;
  actor: string;
  body: string;
  parentId: string | null;
  createdAt: string;
}

/** Attribution actor format. Use the helper in `attribution.ts` to derive
 *  the right string from the GitHub-sync state. */
export type WorkspaceActor =
  | 'user'
  | `user:${string}` // GitHub-synced: 'user:<login>'
  | 'claude'
  | 'codex'
  | 'gemini'
  | (string & {});

// Project-issue scan — mirror of `ProjectScanResult` enum on the Rust
// side. Each variant maps to a distinct UI banner state.

export interface ProjectIssue {
  externalId: string;
  title: string;
  body: string;
  url: string;
  source: 'github' | 'gitlab' | string;
  labels: string[];
}

// Inbox row — a note or card recently mutated by an agent (any
// `updated_by` not starting with 'user'). Shape matches Rust's
// `InboxItem` repo struct.
export interface InboxItem {
  kind: 'note' | 'card' | (string & {});
  id: string;
  workspaceId: string;
  workspaceName: string;
  label: string;
  boardId: string | null;
  boardName: string | null;
  updatedBy: string;
  updatedAt: string;
}

export type ProjectScanResult =
  | { kind: 'success'; issues: ProjectIssue[]; remote: string; source: string }
  | { kind: 'notGitRepo' }
  | { kind: 'noRemote' }
  | { kind: 'unsupportedRemote'; url: string }
  | { kind: 'toolNotInstalled'; tool: string; installUrl: string }
  | { kind: 'notAuthenticated'; tool: string; loginCommand: string }
  | { kind: 'apiError'; message: string };
