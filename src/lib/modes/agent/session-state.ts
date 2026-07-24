export interface ResumeLinkedSession {
  id: string;
  claudeSessionId: string | null;
}

/**
 * Return a new session list with the provider resume id attached to exactly
 * one managed row. This keeps sidebar/session-list objects in sync when the
 * id is discovered from a CLI exit banner after the normal capture poll.
 */
export function applyCapturedResumeId<T extends ResumeLinkedSession>(
  sessions: T[],
  rowId: string,
  resumeId: string,
): T[] {
  return sessions.map((session) =>
    session.id === rowId
      ? { ...session, claudeSessionId: resumeId }
      : session,
  );
}
