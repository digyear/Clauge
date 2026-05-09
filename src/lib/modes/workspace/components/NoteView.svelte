<script lang="ts">
  import { onDestroy } from 'svelte';
  import MilkdownEditor from './MilkdownEditor.svelte';
  import TagInput from './TagInput.svelte';
  import { activeWorkspace } from '../stores';
  import { workspaceNoteGet, workspaceNoteUpdate } from '../commands';
  import { describeActor, formatAttribution, currentUserActor } from '../attribution';
  import type { WorkspaceNote } from '../types';
  import { showToast } from '$lib/shared/primitives/toast';
  import { agentSessions, activeAgentSession } from '$lib/modes/agent/stores';
  import { mode } from '$lib/stores/app';
  import { activateTabAcrossMode } from '$lib/utils/tabActivation';
  import { tabs as sharedTabs, addTab, activateTab, updateTab } from '$lib/shared/stores/tabs';
  import { getPurposeColor } from '$lib/modes/agent/ai/prompt';
  import { get } from 'svelte/store';

  interface Props {
    noteId: string;
  }

  let { noteId }: Props = $props();

  let note = $state<WorkspaceNote | null>(null);
  let title = $state('');
  let tags = $state<string[]>([]);
  let currentContent = $state('');
  let saving = $state(false);
  let dirty = $state(false);
  let saveTimeout: ReturnType<typeof setTimeout> | null = null;

  const linkedSession = $derived.by(() => {
    if (!note?.linkedSessionId) return null;
    return get(agentSessions).find(s => s.id === note!.linkedSessionId) ?? null;
  });

  /** Load the note. Re-runs when noteId changes (user switching tabs).
   *  If the note isn't already linked AND there's an active agent session
   *  for the same project, auto-link silently — saves the user a click on
   *  the "Link active session" button. The project-match check avoids
   *  cross-wiring an unrelated session that just happens to be active. */
  async function bootstrap(id: string) {
    note = null;
    try {
      const fetched = await workspaceNoteGet(id);
      note = fetched;
      title = fetched.title;
      try { tags = JSON.parse(fetched.tags); } catch { tags = []; }
      currentContent = fetched.content;

      if (!fetched.linkedSessionId) {
        const active = get(activeAgentSession);
        const ws = get(activeWorkspace);
        const projectMatches =
          !!active &&
          (!ws?.projectPath || active.projectPath === ws.projectPath);
        if (active && projectMatches) {
          // Persist the link silently; no toast — auto-links should feel
          // ambient, not announce themselves on every open.
          note = { ...fetched, linkedSessionId: active.id };
          try {
            await workspaceNoteUpdate({
              id: fetched.id,
              title: fetched.title,
              content: fetched.content,
              tags: (() => { try { return JSON.parse(fetched.tags); } catch { return []; } })(),
              linkedSessionId: active.id,
              actor: currentUserActor(),
            });
          } catch (e) {
            console.warn('Auto-link failed:', e);
            // Roll back the optimistic local change so the UI doesn't lie.
            note = fetched;
          }
        }
      }
    } catch (e) {
      showToast(`Failed to load note: ${e}`, 'error');
    }
  }

  function onContentChange(markdown: string) {
    currentContent = markdown;
    dirty = true;
    scheduleSave();
  }

  function scheduleSave() {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(saveNow, 600);
  }

  async function saveNow() {
    if (!note || saving) return;
    saving = true;
    try {
      await workspaceNoteUpdate({
        id: note.id,
        title: title.trim() || 'Untitled',
        content: currentContent,
        tags,
        linkedSessionId: note.linkedSessionId,
        actor: currentUserActor(),
      });
      dirty = false;
      // Local refresh — keep editor mounted, just refresh metadata.
      const refreshed = await workspaceNoteGet(note.id);
      note = { ...refreshed, content: currentContent };
      const myTab = get(sharedTabs).find(t => t.mode === 'workspace' && t.key === `note:${refreshed.id}`);
      if (myTab && myTab.label !== refreshed.title) {
        updateTab(myTab.id, { label: refreshed.title || 'Untitled' });
      }
    } catch (e) {
      showToast(`Save failed: ${e}`, 'error');
    } finally {
      saving = false;
    }
  }

  function onTitleBlur() {
    if (!dirty && title === note?.title) return;
    dirty = true;
    saveNow();
  }
  function onTagsChange(_next: string[]) {
    dirty = true;
    saveNow();
  }

  async function attachToActiveSession() {
    if (!note) return;
    const s = get(activeAgentSession);
    if (!s) {
      showToast('No active agent session — open one in Agent mode first', 'error');
      return;
    }
    note.linkedSessionId = s.id;
    await saveNow();
    showToast(`Linked to "${s.title}"`, 'success');
  }

  async function detachSession() {
    if (!note) return;
    note.linkedSessionId = null;
    await saveNow();
  }

  function openLinkedSession() {
    if (!linkedSession) return;
    // Activate / open the agent tab for this session.
    const allTabs = get(sharedTabs);
    const existing = allTabs.find(t => t.mode === 'agent' && t.key === linkedSession.id);
    if (existing) {
      activateTabAcrossMode(existing.id);
    } else {
      const tab = addTab(linkedSession.title, 'agent', linkedSession.id, getPurposeColor(linkedSession.purpose));
      activateTab(tab.id);
      activeAgentSession.set(linkedSession);
      mode.set('agent');
    }
  }

  $effect(() => { bootstrap(noteId); });

  onDestroy(() => {
    if (saveTimeout) clearTimeout(saveTimeout);
    if (dirty) saveNow();
  });
</script>

{#if !note}
  <div class="nv-loading">Loading…</div>
{:else}
  {@const editor_info = describeActor(note.updatedBy)}
  <div class="nv">
    <div class="nv-meta">
      <span class="nv-crumb">{$activeWorkspace?.name ?? 'workspace'}</span>
      <span class="nv-sep">/</span>
      <span class="nv-crumb-active">{note.title || 'untitled'}</span>
      <span style="flex:1"></span>
      {#if saving}
        <span class="nv-saving">saving…</span>
      {:else if dirty}
        <span class="nv-dirty">unsaved</span>
      {:else}
        <span class="nv-saved">saved</span>
      {/if}
    </div>

    <input
      class="nv-title"
      bind:value={title}
      onblur={onTitleBlur}
      placeholder="Untitled"
      spellcheck="false"
    />

    <div class="nv-props">
      <div class="nv-prop-key">PROJECT</div>
      <div class="nv-prop-val">
        {#if $activeWorkspace?.projectName}
          <span class="nv-pill">
            <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
            {$activeWorkspace.projectName}
          </span>
        {:else}
          <span class="nv-prop-empty">none</span>
        {/if}
      </div>

      <div class="nv-prop-key">TAGS</div>
      <div class="nv-prop-val">
        <TagInput bind:value={tags} onchange={onTagsChange} />
      </div>

      <div class="nv-prop-key">LINKED SESSION</div>
      <div class="nv-prop-val">
        {#if linkedSession}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span class="nv-pill nv-pill-clickable" onclick={openLinkedSession}>
            <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l1.6 4.8L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.2L12 3z"/></svg>
            {linkedSession.title}
            <span class="nv-pill-dim">· {linkedSession.purpose}</span>
          </span>
          <button class="nv-mini-btn" onclick={detachSession} title="Unlink">×</button>
        {:else if $activeAgentSession && (!$activeWorkspace?.projectPath || $activeAgentSession.projectPath !== $activeWorkspace.projectPath)}
          <!-- Active session is for a different project — manual opt-in only. -->
          <button class="nv-mini-btn" onclick={attachToActiveSession} title="The active agent session is for a different project — link anyway?">
            Link {$activeAgentSession.title} (different project)
          </button>
        {:else}
          <span class="nv-prop-empty">no session linked</span>
        {/if}
      </div>

      <div class="nv-prop-key">UPDATED</div>
      <div class="nv-prop-val">
        <span class="nv-attr">
          {#if editor_info.kind === 'agent'}
            <span class="nv-attr-badge nv-attr-agent" title="Edited by {editor_info.label}">
              <svg viewBox="0 0 24 24" width="9" height="9" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l1.6 4.8L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.2L12 3z"/></svg>
              {editor_info.label}
            </span>
          {:else if editor_info.avatarUrl}
            <img class="nv-attr-avatar" src={editor_info.avatarUrl} alt="" width="14" height="14"/>
            <span>{editor_info.label}</span>
          {:else}
            <span class="nv-attr-anon">{editor_info.label}</span>
          {/if}
          <span class="nv-attr-time">· {formatAttribution(note.updatedBy, note.updatedAt).split('· ')[1] ?? ''}</span>
        </span>
      </div>
    </div>

    <div class="nv-editor">
      {#key note.id}
        <MilkdownEditor value={note.content} onChange={onContentChange} />
      {/key}
    </div>
  </div>
{/if}

<style>
  .nv-loading {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--t3);
    font-family: var(--ui);
    font-size: 12.5px;
  }
  .nv {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
    padding: 16px 28px 0;
  }
  .nv-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 14px;
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--t4);
  }
  .nv-crumb { color: var(--t3); }
  .nv-crumb-active { color: var(--t2); }
  .nv-sep { color: var(--t4); }
  .nv-saving { color: var(--warn, #f5a623); font-style: italic; }
  .nv-dirty { color: var(--t4); font-style: italic; }
  .nv-saved { color: var(--ok, #1dc880); }

  .nv-title {
    border: none;
    background: transparent;
    color: var(--t1);
    font-family: var(--ui);
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.01em;
    outline: none;
    padding: 0;
    margin: 0 0 14px;
    width: 100%;
  }
  .nv-title::placeholder { color: var(--t4); }

  .nv-props {
    display: grid;
    grid-template-columns: 110px 1fr;
    row-gap: 8px;
    column-gap: 14px;
    align-items: center;
    margin-bottom: 18px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--b1);
    max-width: 780px;
  }
  .nv-prop-key {
    font-family: var(--ui);
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: var(--t4);
  }
  .nv-prop-val {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-family: var(--ui);
    font-size: 12px;
    color: var(--t2);
    min-width: 0;
  }
  .nv-prop-empty { color: var(--t4); font-style: italic; }
  .nv-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 9px;
    border-radius: 12px;
    border: 1px solid var(--b1);
    background: rgba(255, 255, 255, 0.03);
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--t1);
  }
  .nv-pill-clickable { cursor: default; }
  .nv-pill-clickable:hover { border-color: var(--acc); color: var(--acc); }
  .nv-pill-dim { color: var(--t4); }


  .nv-mini-btn {
    border: 1px solid var(--b1);
    background: transparent;
    color: var(--t3);
    font-family: var(--ui);
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 5px;
    cursor: default;
    transition: border-color 0.12s, color 0.12s;
  }
  .nv-mini-btn:hover { border-color: var(--acc); color: var(--t1); }

  .nv-attr {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-family: var(--ui);
    font-size: 11.5px;
    color: var(--t2);
  }
  .nv-attr-avatar { border-radius: 50%; }
  .nv-attr-anon { color: var(--t3); }
  .nv-attr-time { color: var(--t4); }
  .nv-attr-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 7px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--acc) 15%, transparent);
    color: var(--acc);
    font-size: 10px;
    font-weight: 500;
  }

  .nv-editor {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    margin: 0 -28px;
    padding: 0 28px;
  }
</style>
