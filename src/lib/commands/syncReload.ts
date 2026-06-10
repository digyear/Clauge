/**
 * Reload every synced domain store from SQLite after a bulk import
 * (restore, merge, snapshot restore). Dynamic imports keep this module
 * free of eager dependencies on every mode's store graph.
 */
export async function reloadSyncedStores(): Promise<void> {
  const [r, s, n, ssh, agent, explorer, workspace] = await Promise.all([
    import('$lib/modes/rest/stores'),
    import('$lib/modes/sql/stores'),
    import('$lib/modes/nosql/stores'),
    import('$lib/modes/ssh/stores'),
    import('$lib/modes/agent/stores'),
    import('$lib/modes/explorer/stores'),
    import('$lib/modes/workspace/stores'),
  ]);
  await Promise.all([
    r.loadCollections(),
    r.loadEnvironments(),
    s.loadConnections(),
    s.loadSqlScripts(),
    n.loadNoSqlConnections(),
    ssh.loadSshProfiles(),
    agent.loadAgentSessions(),
    agent.loadAgentContexts(),
    explorer.loadExplorerConnections(),
    workspace.loadWorkspaces(),
    workspace.loadCoworkers(),
  ]);
}
