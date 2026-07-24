// @ts-expect-error Bun provides this module when running `bun test`.
import { describe, expect, test } from 'bun:test';
import { applyCapturedResumeId } from './session-state';

describe('applyCapturedResumeId', () => {
  test('updates the sidebar row used by the next reopen', () => {
    const staleSidebarRows = [
      { id: 'hermes-row', claudeSessionId: null, title: 'yyy' },
      { id: 'sibling', claudeSessionId: 'existing-id', title: 'other' },
    ];

    const updated = applyCapturedResumeId(
      staleSidebarRows,
      'hermes-row',
      '20260724_175729_2ff8d4',
    );

    expect(updated[0].claudeSessionId).toBe('20260724_175729_2ff8d4');
    expect(updated[1]).toBe(staleSidebarRows[1]);
    expect(staleSidebarRows[0].claudeSessionId).toBeNull();
  });
});
