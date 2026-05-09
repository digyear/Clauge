<script lang="ts">
  // Side drawer for editing a single card. Title, priority, tags,
  // markdown description, review checklist (read-only when populated by
  // an agent self-review). Saves via workspaceCardUpdate; the parent
  // BoardView refreshes after save.

  import { onMount } from 'svelte';
  import {
    workspaceCardUpdate,
    workspaceCardAddComment,
    workspaceCardCommentList,
    workspaceCardPushToRepo,
    workspaceCardSetLinkedSession,
    workspaceCardMentionSession,
  } from '../commands';
  import { agentListSessions } from '$lib/modes/agent/commands';
  import type { AgentSession } from '$lib/modes/agent/types';
  import { currentUserActor, describeActor, formatAttribution } from '../attribution';
  import { cardSourceBadge } from '../cardSource';
  import { markMentionStart, markMentionEnd, markCardSeen } from '../stores';
  import type { Workspace, WorkspaceBoardCard, WorkspaceCardComment } from '../types';
  import { showToast } from '$lib/shared/primitives/toast';
  import ConfirmDialog from '$lib/shared/primitives/ConfirmDialog.svelte';
  import TagInput from './TagInput.svelte';
  import CardThread from './CardThread.svelte';

  interface Props {
    card: WorkspaceBoardCard;
    /** Workspace this card lives in. Optional — if omitted, we look it
     *  up via the cards-by-board store. We need the repo URL to decide
     *  whether the "Push to repo" button is even shown. */
    workspace?: Workspace | null;
    onclose?: () => void;
    onsave?: () => void;
  }

  let { card, workspace = null, onclose, onsave }: Props = $props();

  let title = $state(card.title);
  let description = $state(card.description);
  let priority = $state<string | null>(card.priority);
  let tags = $state<string[]>((() => {
    try { return JSON.parse(card.tags) as string[]; } catch { return []; }
  })());
  let saving = $state(false);
  let commentDraft = $state('');
  let commenting = $state(false);
  let pushing = $state(false);
  let showPushConfirm = $state(false);
  let mentioning = $state(false);
  let allSessions = $state<AgentSession[]>([]);
  /** Comments live in their own table now (migration 13). We load on
   *  mount, then mutate locally for optimistic UI; the server is the
   *  source of truth, but appending one bubble doesn't need a refetch. */
  let comments = $state<WorkspaceCardComment[]>([]);
  /** "Thread" = chat bubble view of body + comments.
   *  "Edit"   = raw markdown textarea for the body (no thread). */
  let descTab = $state<'thread' | 'edit'>('thread');
  /** Local override of card.linkedSessionId so the drawer reflects the
   *  user's pick immediately, before the parent re-renders with the
   *  refreshed card row. */
  let linkedSessionId = $state<string | null>(card.linkedSessionId);
  let linkPickerOpen = $state(false);

  const editor = $derived(describeActor(card.updatedBy));
  /** Linked session resolved from the loaded sessions list. Null when
   *  unlinked, or `'missing'` when the link points to a deleted row —
   *  surfaced as a warning so the user can re-link. */
  const linked = $derived.by<AgentSession | 'missing' | null>(() => {
    if (!linkedSessionId) return null;
    const found = allSessions.find((s) => s.id === linkedSessionId);
    if (!found) return allSessions.length === 0 ? null : 'missing';
    return found;
  });
  /** Provider slug used in the @ mention button label. Today every
   *  session is Claude; when codex/gemini sessions land they should
   *  add a `provider` field on AgentSession and this picks it up. */
  const linkedProvider = $derived.by<string>(() => {
    if (linked && linked !== 'missing') {
      // Single-provider product today; future: read (linked as any).provider
      return 'claude';
    }
    return 'claude';
  });
  const canMention = $derived(
    linked !== null && linked !== 'missing' && !mentioning && !!commentDraft.trim(),
  );
  /** Detect "@<provider>" with word boundaries — e.g. "@claude" matches
   *  but "claude@example.com" doesn't. Case-insensitive. The presence
   *  of this token in the draft is what makes a Post auto-route to
   *  postAndMention. Stays a regex (not a startsWith) so the user can
   *  drop the mention anywhere in their text. */
  const draftHasMention = $derived.by(() => {
    if (!linked || linked === 'missing') return false;
    const re = new RegExp(`(^|[^\\w])@${linkedProvider}\\b`, 'i');
    return re.test(commentDraft);
  });
  /** Source state for the live card (re-derived as parent passes new props). */
  const source = $derived(cardSourceBadge(card));
  const repoUrl = $derived(workspace?.repoUrl ?? null);
  const canPush = $derived(source.kind === 'local' && !!repoUrl);
  const repoLabel = $derived.by(() => {
    const u = (repoUrl ?? '').toLowerCase();
    if (u.includes('github.com')) return 'GitHub';
    if (u.includes('gitlab')) return 'GitLab';
    return 'repo';
  });

  async function save() {
    if (saving) return;
    saving = true;
    try {
      await workspaceCardUpdate({
        id: card.id,
        title: title.trim() || 'Untitled',
        description,
        priority: priority || null,
        tags,
        reviewChecklist: card.reviewChecklist,
        actor: currentUserActor(),
      });
      onsave?.();
    } catch (e) {
      showToast(`Save failed: ${e}`, 'error');
    } finally {
      saving = false;
    }
  }

  /** Post a comment as a markdown blockquote at the bottom of the
   *  description, then refresh the local description so the new
   *  block shows up without closing the drawer. We deliberately re-
   *  read the row instead of stitching the block locally — the Rust
   *  side knows the canonical timestamp and stamps the blockquote.
   *
   *  Auto-route: if the draft contains "@<linkedProvider>" and we have
   *  a viable session, treat Post as Post-and-mention. The button
   *  pair stays useful (you can still click "Post & @claude" without
   *  typing the mention), but power users can just type and hit ⌘↵. */
  async function postComment() {
    const body = commentDraft.trim();
    if (!body || commenting) return;
    if (draftHasMention && canMention) {
      return postAndMention();
    }
    commenting = true;
    try {
      const created = await workspaceCardAddComment(card.id, body, currentUserActor());
      // Append the canonical server row to our local thread so the
      // bubble shows up without refetching the whole list.
      comments = [...comments, created];
      commentDraft = '';
      onsave?.();
    } catch (e) {
      showToast(`Comment failed: ${e}`, 'error');
    } finally {
      commenting = false;
    }
  }

  /** "5m", "2h", "3d" — keeps the linked-session card compact. */
  function formatRelative(iso: string): string {
    const t = Date.parse(iso);
    if (Number.isNaN(t)) return iso;
    const secs = Math.max(1, Math.floor((Date.now() - t) / 1000));
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
    return `${Math.floor(secs / 86400)}d ago`;
  }

  /** Initial bootstrap — load the comment thread + agent sessions in
   *  parallel, mark the card as seen, set the default tab based on
   *  whether there are comments to show. */
  onMount(async () => {
    markCardSeen(card.id, card.updatedAt);
    try {
      const [loadedComments, loadedSessions] = await Promise.all([
        workspaceCardCommentList(card.id),
        agentListSessions(),
      ]);
      comments = loadedComments;
      allSessions = loadedSessions;
      // Land on Thread if the card has any conversation, otherwise on
      // Edit so a brand-new card opens straight to its body field.
      descTab = comments.length > 0 ? 'thread' : 'edit';
    } catch (e) {
      console.warn('Drawer bootstrap failed:', e);
    }
  });

  async function applyLinkedSession(sessionId: string | null) {
    try {
      await workspaceCardSetLinkedSession(card.id, sessionId, currentUserActor());
      linkedSessionId = sessionId;
      linkPickerOpen = false;
      onsave?.();
    } catch (e) {
      showToast(`Link failed: ${e}`, 'error');
    }
  }

  /** Post the comment AND trigger the linked session. The Rust side
   *  handles both: it appends the user comment, runs the agent, then
   *  appends the agent's reply as another comment. We optimistically
   *  show the user's comment locally so the drawer feels responsive
   *  while the agent runs (which can take 30-120s). */
  async function postAndMention() {
    const body = commentDraft.trim();
    if (!body || !canMention) return;
    mentioning = true;
    markMentionStart(card.id, linkedProvider);
    // Optimistic user-comment row so the bubble appears immediately.
    // We synthesise an id locally; the server will issue its own id
    // and we'll reconcile if needed via the response.
    const optimisticUser: WorkspaceCardComment = {
      id: `pending-user-${Date.now()}`,
      cardId: card.id,
      actor: currentUserActor(),
      body,
      parentId: null,
      createdAt: new Date().toISOString(),
    };
    comments = [...comments, optimisticUser];
    commentDraft = '';
    descTab = 'thread';
    try {
      const result = await workspaceCardMentionSession(
        card.id,
        body,
        currentUserActor(),
      );
      // Replace the synthetic user comment id with the canonical one
      // from the server so future deletes/edits reference the right row.
      // Then append the agent reply.
      const reconciled: WorkspaceCardComment = {
        ...optimisticUser,
        id: result.userCommentId ?? optimisticUser.id,
      };
      const reply: WorkspaceCardComment = {
        id: result.replyCommentId ?? `pending-reply-${Date.now()}`,
        cardId: card.id,
        actor: result.provider,
        body: result.response,
        parentId: null,
        createdAt: new Date().toISOString(),
      };
      comments = comments.map((c) => (c.id === optimisticUser.id ? reconciled : c)).concat(reply);
      onsave?.();
    } catch (e) {
      // Roll back the optimistic comment so the UI reflects truth — the
      // server may or may not have written it depending on which step
      // errored. Refetching from the server is the safe move.
      try {
        comments = await workspaceCardCommentList(card.id);
      } catch { /* keep the optimistic in place if refetch fails */ }
      showToast(`Mention failed: ${e}`, 'error');
    } finally {
      mentioning = false;
      markMentionEnd(card.id);
    }
  }

  function confirmPush() {
    if (!canPush || pushing) return;
    showPushConfirm = true;
  }

  /** User confirmed — shell out to gh/glab and update the card. We
   *  don't need to mutate local state; parent's onsave refresh pulls
   *  the new external_id/url and the badge re-renders. */
  async function doPush() {
    if (pushing) return;
    pushing = true;
    try {
      const result = await workspaceCardPushToRepo(card.id, currentUserActor());
      showToast(`Pushed as ${result.externalId}`, 'success');
      onsave?.();
    } catch (e) {
      showToast(`Push failed: ${e}`, 'error');
    } finally {
      pushing = false;
    }
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose?.();
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      save();
    }
  }
</script>

<svelte:window onkeydown={handleKey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ce-overlay" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ce-drawer" onclick={(e) => e.stopPropagation()}>
    <div class="ce-head">
      <span class="ce-eyebrow">Edit card</span>
      <button class="ce-close" onclick={onclose} title="Close">×</button>
    </div>

    <div class="ce-body">
      <label class="ce-field">
        <span class="ce-label">Title</span>
        <input class="ce-input ce-input-title" bind:value={title} placeholder="Untitled" spellcheck="false" />
      </label>

      <div class="ce-row">
        <label class="ce-field ce-field-half">
          <span class="ce-label">Priority</span>
          <select class="ce-input" bind:value={priority}>
            <option value={null}>None</option>
            <option value="P0">P0 — critical</option>
            <option value="P1">P1 — high</option>
            <option value="P2">P2 — normal</option>
            <option value="P3">P3 — low</option>
          </select>
        </label>

        <div class="ce-field ce-field-half">
          <span class="ce-label">Tags</span>
          <TagInput bind:value={tags} />
        </div>
      </div>

      <div class="ce-field">
        <div class="ce-tab-row">
          <button
            type="button"
            class="ce-tab"
            class:ce-tab-active={descTab === 'thread'}
            onclick={() => (descTab = 'thread')}
            title="Read view — body + comment bubbles"
          >
            Thread
            {#if comments.length > 0}
              <span class="ce-tab-count">{comments.length}</span>
            {/if}
          </button>
          <button
            type="button"
            class="ce-tab"
            class:ce-tab-active={descTab === 'edit'}
            onclick={() => (descTab = 'edit')}
            title="Raw markdown — full edit control"
          >
            Edit
          </button>
        </div>
        {#if descTab === 'thread'}
          <CardThread body={description} {comments} />
        {:else}
          <textarea
            class="ce-textarea"
            bind:value={description}
            placeholder="Notes, links, file references… markdown supported."
          ></textarea>
        {/if}
      </div>

      <div class="ce-field">
        <span class="ce-label">Linked session</span>
        {#if linked === 'missing'}
          <div class="ce-link-warn">
            ⚠ Linked session no longer exists.
            <button class="ce-link-action" onclick={() => (linkPickerOpen = true)}>Re-link…</button>
            ·
            <button class="ce-link-action" onclick={() => applyLinkedSession(null)}>Unlink</button>
          </div>
        {:else if linked}
          <div class="ce-link-card">
            <div class="ce-link-card-top">
              <span class="ce-link-icon" aria-hidden="true">
                <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l1.6 4.8L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.2L12 3z"/></svg>
              </span>
              <span class="ce-link-title" title={linked.id}>{linked.title || 'Untitled session'}</span>
              <span class="ce-link-tag">@{linkedProvider}</span>
            </div>
            <div class="ce-link-meta">
              <span class="ce-link-project">{linked.projectName}</span>
              <span class="ce-link-dot">·</span>
              <span>last used {formatRelative(linked.lastUsedAt)}</span>
            </div>
            <div class="ce-link-actions">
              <button class="ce-link-action" onclick={() => (linkPickerOpen = true)}>Change…</button>
              <button class="ce-link-action ce-link-action-warn" onclick={() => applyLinkedSession(null)}>Unlink</button>
            </div>
          </div>
        {:else}
          <div class="ce-link-empty">
            <span>No session linked.</span>
            <button class="ce-link-action" onclick={() => (linkPickerOpen = true)} disabled={allSessions.length === 0}>
              {allSessions.length === 0 ? 'No sessions exist yet — create one in Agent mode' : 'Link a session…'}
            </button>
          </div>
        {/if}
        {#if linkPickerOpen}
          <div class="ce-link-picker">
            <select
              class="ce-input"
              onchange={(e) => {
                const v = (e.currentTarget as HTMLSelectElement).value;
                applyLinkedSession(v || null);
              }}
            >
              <option value="">— Pick a session —</option>
              {#each allSessions as s}
                <option value={s.id} selected={s.id === linkedSessionId}>
                  {s.title || 'Untitled'} · {s.projectName}
                </option>
              {/each}
            </select>
            <button class="ce-link-action" onclick={() => (linkPickerOpen = false)}>Cancel</button>
          </div>
        {/if}
      </div>

      <div class="ce-field">
        <span class="ce-label">Add comment</span>
        <textarea
          class="ce-comment-input"
          bind:value={commentDraft}
          placeholder={linked && linked !== 'missing'
            ? `Write a comment, or send to @${linkedProvider} to trigger the linked session…`
            : 'Write a comment — appended as a quoted block to the description.'}
          onkeydown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
              e.preventDefault();
              if (e.shiftKey && canMention) postAndMention();
              else postComment();
            }
          }}
        ></textarea>
        <div class="ce-comment-foot">
          <span class="ce-comment-hint">
            {#if linked && linked !== 'missing'}
              {#if draftHasMention}
                <span class="ce-hint-active">
                  ⚡ Will trigger <strong>@{linkedProvider}</strong> on Post
                </span>
              {:else}
                Type <strong>@{linkedProvider}</strong> to send · ⌘↵ post · ⇧⌘↵ post + send
              {/if}
            {:else}
              ⌘↵ to post
            {/if}
          </span>
          <div class="ce-comment-buttons">
            <button
              type="button"
              class="ce-btn-comment"
              class:ce-btn-comment-mention={draftHasMention && canMention}
              onclick={postComment}
              disabled={commenting || mentioning || !commentDraft.trim()}
              title={draftHasMention && canMention
                ? `@${linkedProvider} detected — Post will trigger the mention`
                : 'Add to thread without triggering an agent'}
            >
              {#if commenting}
                Posting…
              {:else if draftHasMention && canMention}
                Post & @{linkedProvider}
              {:else}
                Post
              {/if}
            </button>
            {#if linked && linked !== 'missing'}
              <button
                type="button"
                class="ce-btn-mention"
                onclick={postAndMention}
                disabled={!canMention}
                title={canMention
                  ? `Post and trigger @${linkedProvider} on the linked session`
                  : 'Type a comment first'}
              >
                {mentioning ? `Sending to @${linkedProvider}…` : `Post & @${linkedProvider}`}
              </button>
            {/if}
          </div>
        </div>
      </div>

      {#if card.reviewChecklist}
        <div class="ce-field">
          <span class="ce-label">Review checklist <span class="ce-label-dim">(set by {editor.label})</span></span>
          <pre class="ce-checklist">{card.reviewChecklist}</pre>
        </div>
      {/if}

      <div class="ce-meta">
        <span class="ce-meta-key">Updated</span>
        <span>{formatAttribution(card.updatedBy, card.updatedAt)}</span>
        {#if card.reviewPending === 1}
          <span class="ce-pending">· Pending review</span>
        {/if}
      </div>
    </div>

    <div class="ce-foot">
      {#if source.kind === 'local'}
        <button
          type="button"
          class="ce-btn-push"
          onclick={confirmPush}
          disabled={!canPush || pushing}
          title={canPush
            ? `Create a real ${repoLabel} issue from this card`
            : 'Set the workspace repo URL first (Workspace settings)'}
        >
          {pushing ? 'Pushing…' : `Push to ${repoLabel}`}
        </button>
      {:else if source.url}
        <a
          class="ce-link-out"
          href={source.url}
          target="_blank"
          rel="noreferrer noopener"
          title="Open the linked issue in your browser"
        >
          {source.label} ↗
        </a>
      {/if}
      <span class="ce-foot-spacer"></span>
      <button class="ce-btn-cancel" onclick={onclose}>Cancel</button>
      <button class="ce-btn-save" onclick={save} disabled={saving}>
        {saving ? 'Saving…' : 'Save'}
      </button>
    </div>
  </div>
</div>

<ConfirmDialog
  bind:show={showPushConfirm}
  title="Push to {repoLabel}?"
  message={`This will create a new issue on ${repoLabel} with the card's title and description. The issue will be public if the repo is public — make sure you're ready for that.`}
  confirmText={`Push to ${repoLabel}`}
  confirmColor="var(--acc)"
  onconfirm={doPush}
/>

<style>
  .ce-overlay {
    /* Absolute (not fixed) so the overlay is contained by .app-workspace
     * and never covers Topbar / StatusBar. Topbar lives above this in
     * the flex stack; StatusBar below. */
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 200;
    display: flex;
    justify-content: flex-end;
    animation: fadeIn 0.15s ease;
  }
  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }

  .ce-drawer {
    width: 460px;
    height: 100%;
    background: var(--n, var(--modal-bg, #0d1117));
    border-left: 1px solid var(--b1);
    box-shadow: -10px 0 30px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    animation: slideIn 0.18s ease;
  }
  @keyframes slideIn {
    from { transform: translateX(20px); opacity: 0.6; }
    to { transform: none; opacity: 1; }
  }

  .ce-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 18px;
    border-bottom: 1px solid var(--b1);
    flex-shrink: 0;
  }
  .ce-eyebrow {
    font-family: var(--ui);
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: var(--t4);
    text-transform: uppercase;
  }
  .ce-close {
    width: 26px;
    height: 26px;
    border: none;
    background: transparent;
    color: var(--t3);
    font-size: 18px;
    line-height: 1;
    cursor: default;
    border-radius: 5px;
  }
  .ce-close:hover { background: rgba(255, 255, 255, 0.06); color: var(--t1); }

  .ce-body {
    flex: 1;
    overflow-y: auto;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .ce-field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .ce-field-half { flex: 1; }
  .ce-row {
    display: flex;
    gap: 10px;
  }
  .ce-label {
    font-family: var(--ui);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--t4);
    text-transform: uppercase;
  }
  .ce-label-dim {
    font-weight: 500;
    color: var(--t4);
    text-transform: none;
    letter-spacing: 0;
    font-size: 10.5px;
  }
  .ce-input {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--b1);
    border-radius: 6px;
    padding: 7px 10px;
    color: var(--t1);
    font-family: var(--mono);
    font-size: 12px;
    outline: none;
    transition: border-color 0.12s;
  }
  .ce-input:focus { border-color: var(--acc); }
  .ce-input-title {
    font-family: var(--ui);
    font-size: 14px;
    font-weight: 600;
  }
  .ce-tab-row {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--b1);
    margin-bottom: 8px;
  }
  .ce-tab {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--t3);
    font-family: var(--ui);
    font-size: 11.5px;
    font-weight: 600;
    padding: 7px 10px;
    cursor: default;
    margin-bottom: -1px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    transition: color 0.12s, border-color 0.12s;
  }
  .ce-tab:hover { color: var(--t1); }
  .ce-tab-active { color: var(--t1); border-bottom-color: var(--acc); }
  .ce-tab-count {
    font-family: var(--mono);
    font-size: 10px;
    background: rgba(255, 255, 255, 0.08);
    color: var(--t2);
    padding: 1px 6px;
    border-radius: 8px;
    line-height: 1.4;
  }
  .ce-tab-active .ce-tab-count {
    background: color-mix(in srgb, var(--acc) 18%, transparent);
    color: var(--acc);
  }
  .ce-textarea {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--b1);
    border-radius: 6px;
    padding: 10px 12px;
    color: var(--t1);
    font-family: var(--ui);
    font-size: 12.5px;
    line-height: 1.6;
    outline: none;
    min-height: 220px;
    resize: vertical;
    transition: border-color 0.12s;
  }
  .ce-textarea:focus { border-color: var(--acc); }
  .ce-comment-input {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--b1);
    border-radius: 6px;
    padding: 8px 10px;
    color: var(--t1);
    font-family: var(--ui);
    font-size: 12px;
    line-height: 1.5;
    outline: none;
    min-height: 64px;
    resize: vertical;
    transition: border-color 0.12s;
  }
  .ce-comment-input:focus { border-color: var(--acc); }
  .ce-comment-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 6px;
  }
  .ce-comment-hint {
    font-family: var(--ui);
    font-size: 10.5px;
    color: var(--t4);
  }
  .ce-comment-hint strong { color: var(--t2); font-weight: 600; }
  .ce-hint-active {
    color: var(--acc);
    font-weight: 500;
  }
  .ce-hint-active strong { color: var(--acc); }
  /* When @-mention is detected, the regular Post button morphs into the
     accent-style mention button so the user sees the routing change. */
  .ce-btn-comment-mention {
    background: var(--acc);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .ce-btn-comment-mention:hover:not(:disabled) {
    opacity: 0.9;
    color: #fff;
  }
  .ce-btn-comment {
    border: 1px solid var(--b2);
    background: transparent;
    color: var(--t2);
    height: 26px;
    padding: 0 12px;
    border-radius: 5px;
    font-family: var(--ui);
    font-size: 11.5px;
    cursor: default;
    transition: opacity 0.12s, border-color 0.12s, color 0.12s;
  }
  .ce-btn-comment:hover:not(:disabled) { color: var(--t1); border-color: var(--acc); }
  .ce-btn-comment:disabled { opacity: 0.4; }
  .ce-comment-buttons {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .ce-btn-mention {
    border: none;
    background: var(--acc);
    color: #fff;
    height: 26px;
    padding: 0 12px;
    border-radius: 5px;
    font-family: var(--ui);
    font-size: 11.5px;
    font-weight: 600;
    cursor: default;
    transition: opacity 0.12s;
  }
  .ce-btn-mention:hover:not(:disabled) { opacity: 0.9; }
  .ce-btn-mention:disabled { opacity: 0.4; }

  /* Linked-session card — visually anchors the @mention affordance.
     The accent border on the left mirrors the review-checklist block
     so the user reads them as a related family of "agent" surfaces. */
  .ce-link-card {
    border: 1px solid color-mix(in srgb, var(--acc) 28%, transparent);
    border-left: 3px solid var(--acc);
    background: color-mix(in srgb, var(--acc) 6%, transparent);
    border-radius: 6px;
    padding: 9px 11px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .ce-link-card-top {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ce-link-icon {
    color: var(--acc);
    display: inline-flex;
    align-items: center;
  }
  .ce-link-title {
    font-family: var(--ui);
    font-size: 12.5px;
    font-weight: 600;
    color: var(--t1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ce-link-tag {
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--acc);
    background: color-mix(in srgb, var(--acc) 14%, transparent);
    padding: 1px 6px;
    border-radius: 4px;
    margin-left: auto;
  }
  .ce-link-meta {
    font-family: var(--ui);
    font-size: 10.5px;
    color: var(--t3);
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .ce-link-project {
    font-family: var(--mono);
    color: var(--t2);
  }
  .ce-link-dot { color: var(--t4); }
  .ce-link-actions {
    display: flex;
    gap: 6px;
    margin-top: 3px;
  }
  .ce-link-action {
    background: transparent;
    border: none;
    padding: 0;
    color: var(--acc);
    font-family: var(--ui);
    font-size: 10.5px;
    cursor: default;
  }
  .ce-link-action:hover:not(:disabled) { text-decoration: underline; }
  .ce-link-action:disabled { color: var(--t4); cursor: not-allowed; }
  .ce-link-action-warn { color: var(--t3); }
  .ce-link-action-warn:hover { color: var(--err, #f87171); }
  .ce-link-warn {
    border: 1px solid color-mix(in srgb, var(--err, #f87171) 35%, transparent);
    background: color-mix(in srgb, var(--err, #f87171) 8%, transparent);
    color: var(--t1);
    font-family: var(--ui);
    font-size: 11.5px;
    border-radius: 6px;
    padding: 8px 10px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ce-link-empty {
    border: 1px dashed var(--b1);
    border-radius: 6px;
    padding: 9px 11px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    color: var(--t3);
    font-family: var(--ui);
    font-size: 11.5px;
  }
  .ce-link-picker {
    display: flex;
    gap: 6px;
    align-items: center;
    margin-top: 6px;
  }
  .ce-link-picker .ce-input { flex: 1; }
  .ce-checklist {
    background: rgba(167, 139, 250, 0.08);
    border: 1px solid color-mix(in srgb, #a78bfa 30%, transparent);
    border-radius: 6px;
    padding: 10px 12px;
    color: var(--t1);
    font-family: var(--mono);
    font-size: 11.5px;
    line-height: 1.6;
    margin: 0;
    white-space: pre-wrap;
  }

  .ce-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-family: var(--ui);
    font-size: 11px;
    color: var(--t3);
    padding-top: 6px;
    border-top: 1px solid var(--b1);
  }
  .ce-meta-key {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--t4);
    text-transform: uppercase;
  }
  .ce-pending { color: #a78bfa; font-weight: 500; }

  .ce-foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 18px;
    border-top: 1px solid var(--b1);
    flex-shrink: 0;
  }
  .ce-btn-cancel,
  .ce-btn-save {
    height: 30px;
    padding: 0 16px;
    border-radius: 6px;
    font-family: var(--ui);
    font-size: 12px;
    cursor: default;
    transition: opacity 0.12s, border-color 0.12s, color 0.12s;
  }
  .ce-btn-cancel {
    border: 1px solid var(--b1);
    background: transparent;
    color: var(--t2);
  }
  .ce-btn-cancel:hover { border-color: var(--b2); color: var(--t1); }
  .ce-btn-save {
    border: none;
    background: var(--acc);
    color: #fff;
    font-weight: 600;
  }
  .ce-btn-save:hover:not(:disabled) { opacity: 0.9; }
  .ce-btn-save:disabled { opacity: 0.4; }
  .ce-foot-spacer { flex: 1; }
  .ce-btn-push {
    height: 30px;
    padding: 0 14px;
    border-radius: 6px;
    font-family: var(--ui);
    font-size: 11.5px;
    font-weight: 600;
    border: 1px solid var(--b2);
    background: transparent;
    color: var(--t2);
    cursor: default;
    transition: opacity 0.12s, border-color 0.12s, color 0.12s, background 0.12s;
  }
  .ce-btn-push:hover:not(:disabled) {
    color: var(--t1);
    border-color: var(--acc);
    background: rgba(255, 255, 255, 0.03);
  }
  .ce-btn-push:disabled { opacity: 0.4; }
  .ce-link-out {
    align-self: center;
    font-family: var(--ui);
    font-size: 11.5px;
    color: var(--t3);
    text-decoration: none;
    border: 1px solid var(--b1);
    border-radius: 6px;
    padding: 6px 10px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    box-sizing: border-box;
  }
  .ce-link-out:hover { color: var(--t1); border-color: var(--b2); }
</style>
