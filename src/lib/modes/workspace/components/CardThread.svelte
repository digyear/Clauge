<script lang="ts">
  // Renders a card's comment thread as left/right chat bubbles. The
  // card's plain markdown body (now back to being just a body, not a
  // mixed thread) sits above the bubbles as a description block.
  //
  // Comments come from `workspace_card_comments` (migration 13). Each
  // row carries actor + body + createdAt; we derive bubble side, label,
  // avatar and agent icon at render time via describeActor().

  import { marked } from 'marked';
  import { describeActor } from '../attribution';
  import { agentIcon } from '../agentIcon';
  import type { WorkspaceCardComment } from '../types';

  interface Props {
    body: string;
    comments: WorkspaceCardComment[];
  }

  let { body, comments }: Props = $props();

  const renderedBody = $derived(body ? (marked.parse(body, { async: false }) as string) : '');

  function bubbleSide(actor: string): 'user' | 'agent' {
    return describeActor(actor).kind === 'agent' ? 'agent' : 'user';
  }

  /** HH:MM extracted from createdAt, falling back to the raw string
   *  for any unexpected shape. The full ISO is available in the title
   *  attribute for fuller context on hover. */
  function shortStamp(iso: string): string {
    if (iso.length >= 16 && iso[10] === 'T') return iso.slice(11, 16);
    return iso;
  }

  function renderBody(text: string): string {
    if (!text.trim()) return '';
    return marked.parse(text, { async: false }) as string;
  }
</script>

<div class="th">
  {#if renderedBody}
    <div class="th-body">
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      {@html renderedBody}
    </div>
  {/if}

  {#if comments.length === 0 && !renderedBody}
    <div class="th-empty">
      No description or comments yet — the thread will start here once
      you add a comment or trigger an agent.
    </div>
  {/if}

  {#each comments as c (c.id)}
    {@const desc = describeActor(c.actor)}
    {@const side = bubbleSide(c.actor)}
    {@const ico = side === 'agent' ? agentIcon(desc.agentId) : null}
    <div class="th-row" class:th-row-agent={side === 'agent'} class:th-row-user={side === 'user'}>
      <div
        class="th-avatar"
        class:th-avatar-agent={side === 'agent'}
        style={ico ? `color: ${ico.color}; background: color-mix(in srgb, ${ico.color} 14%, transparent); border-color: color-mix(in srgb, ${ico.color} 40%, transparent);` : ''}
        title={desc.label}
      >
        {#if side === 'agent' && ico}
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          {@html ico.svg}
        {:else if desc.avatarUrl}
          <img src={desc.avatarUrl} alt="" />
        {:else}
          <span class="th-avatar-init">{desc.label.charAt(0).toUpperCase()}</span>
        {/if}
      </div>
      <div class="th-bubble" class:th-bubble-agent={side === 'agent'}>
        <div class="th-meta">
          <span class="th-author">{desc.label}</span>
          <span class="th-stamp" title={c.createdAt}>{shortStamp(c.createdAt)}</span>
        </div>
        <div class="th-content">
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          {@html renderBody(c.body)}
        </div>
      </div>
    </div>
  {/each}
</div>

<style>
  .th {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 4px 0 12px;
  }
  .th-empty {
    color: var(--t4);
    font-family: var(--ui);
    font-size: 12px;
    padding: 18px 6px;
    text-align: center;
    line-height: 1.6;
    border: 1px dashed var(--b1);
    border-radius: 6px;
  }
  .th-body {
    font-family: var(--ui);
    font-size: 12.5px;
    color: var(--t1);
    line-height: 1.65;
    padding: 10px 12px;
    background: rgba(255, 255, 255, 0.025);
    border: 1px solid var(--b1);
    border-radius: 6px;
  }
  .th-body :global(p) { margin: 0 0 8px; }
  .th-body :global(p:last-child) { margin-bottom: 0; }
  .th-body :global(code) {
    font-family: var(--mono);
    font-size: 11.5px;
    background: rgba(255, 255, 255, 0.06);
    padding: 1px 4px;
    border-radius: 3px;
  }
  .th-body :global(pre) {
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid var(--b1);
    border-radius: 5px;
    padding: 8px 10px;
    overflow-x: auto;
    font-size: 11px;
  }
  .th-body :global(a) { color: var(--acc); text-decoration: none; }
  .th-body :global(a:hover) { text-decoration: underline; }

  .th-row {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .th-row-user { flex-direction: row-reverse; }

  .th-avatar {
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid var(--b1);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--t3);
    font-family: var(--ui);
    overflow: hidden;
    margin-top: 2px;
  }
  .th-avatar img { width: 100%; height: 100%; object-fit: cover; }
  .th-avatar-init {
    font-size: 10.5px;
    font-weight: 700;
    color: var(--t2);
  }

  .th-bubble {
    max-width: 78%;
    background: rgba(255, 255, 255, 0.045);
    border: 1px solid var(--b1);
    border-radius: 8px;
    padding: 7px 10px 8px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .th-bubble-agent {
    background: color-mix(in srgb, var(--acc) 7%, transparent);
    border-color: color-mix(in srgb, var(--acc) 28%, transparent);
  }
  .th-row-user .th-bubble {
    background: rgba(255, 255, 255, 0.06);
  }
  .th-meta {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-family: var(--ui);
    font-size: 10px;
  }
  .th-author { color: var(--t2); font-weight: 600; }
  .th-bubble-agent .th-author { color: var(--acc); }
  .th-stamp { color: var(--t4); font-family: var(--mono); font-size: 9.5px; }
  .th-content {
    color: var(--t1);
    font-family: var(--ui);
    font-size: 12px;
    line-height: 1.55;
    word-wrap: break-word;
  }
  .th-content :global(p) { margin: 0 0 6px; }
  .th-content :global(p:last-child) { margin-bottom: 0; }
  .th-content :global(code) {
    font-family: var(--mono);
    font-size: 11px;
    background: rgba(0, 0, 0, 0.25);
    padding: 1px 4px;
    border-radius: 3px;
  }
  .th-content :global(pre) {
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid var(--b1);
    border-radius: 4px;
    padding: 7px 9px;
    overflow-x: auto;
    margin: 4px 0;
    font-size: 10.5px;
  }
  .th-content :global(a) { color: var(--acc); text-decoration: none; }
  .th-content :global(a:hover) { text-decoration: underline; }
  .th-content :global(blockquote) {
    border-left: 2px solid var(--b2);
    padding-left: 8px;
    margin: 4px 0;
    color: var(--t3);
  }
  .th-content :global(ul),
  .th-content :global(ol) { padding-left: 18px; margin: 4px 0; }
</style>
