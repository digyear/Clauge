export function defaultBranchName(title: string): string {
  return title
    .normalize('NFKC')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/-+/g, '-')
    .replace(/^-+|-+$/g, '');
}

/** Selecting a repository must not turn its directory name into a task title. */
export function sessionTitleAfterProjectSelection(
  currentTitle: string,
  _projectPath: string,
): string {
  return currentTitle;
}
