// @ts-expect-error Bun provides this module when running `bun test`.
import { describe, expect, test } from 'bun:test';
import { defaultBranchName, sessionTitleAfterProjectSelection } from './branch-name';

describe('defaultBranchName', () => {
  test('turns a session title into a readable branch name', () => {
    expect(defaultBranchName('Add User Login')).toBe('add-user-login');
  });

  test('removes punctuation and collapses separators', () => {
    expect(defaultBranchName(' Fix: IPv6 / listener... ')).toBe('fix-ipv6-listener');
  });

  test('keeps unicode letters readable', () => {
    expect(defaultBranchName('修复 登录流程')).toBe('修复-登录流程');
  });
});

describe('sessionTitleAfterProjectSelection', () => {
  test('does not use the project directory name as the task title', () => {
    expect(sessionTitleAfterProjectSelection('', '/workspaces/lute_station')).toBe('');
  });

  test('preserves a title the user already entered', () => {
    expect(sessionTitleAfterProjectSelection('Fix membership renewal', '/workspaces/lute_station'))
      .toBe('Fix membership renewal');
  });
});
