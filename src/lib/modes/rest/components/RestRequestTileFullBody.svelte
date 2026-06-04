<script lang="ts">
  import { get } from 'svelte/store';
  import { tabs, activeTabId, draftRequests, setDraft, getDraft, markDirty } from '$lib/shared/stores/tabs';
  import {
    activeEnvId,
    getEffectiveEnvId,
    requestEnvOverrides,
    loadHistory,
    commitRequest,
    environments,
    setActiveEnv,
  } from '$lib/modes/rest/stores';
  import { executeRequest, quickExecute, getRequest } from '$lib/modes/rest/commands';
  import { showToast } from '$lib/shared/primitives/toast';
  import { friendlyError } from '$lib/utils/errors';
  import { METHOD_COLORS } from '$lib/utils/theme';
  import { safeDecodeURIComponent } from '../utils/url-decode';
  import type { HttpResponse, RequestWithDetails, KVInput } from '$lib/modes/rest/types';

  import KVTable from './KVTable.svelte';
  import AuthEditor from './AuthEditor.svelte';
  import BodyEditor from './BodyEditor.svelte';
  import ResponseViewer from './ResponseViewer.svelte';

  interface Props {
    tabId: string;
  }

  let { tabId }: Props = $props();

  const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'] as const;
  const BODY_METHODS = ['POST', 'PUT', 'PATCH', 'DELETE'];

  type EditorTab = 'headers' | 'auth' | 'params' | 'body' | 'pre-script';

  let saved = $state<RequestWithDetails | null>(null);
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let response = $state<HttpResponse | null>(null);
  let activeEditorTab = $state<EditorTab>('headers');
  let methodMenuOpen = $state(false);
  let envMenuOpen = $state(false);
  let tileRoot = $state<HTMLDivElement | undefined>();
  const envName = $derived(
    $environments.find((e) => e.id === $activeEnvId)?.name ?? 'No env',
  );

  function handleDocPointerDown(e: PointerEvent) {
    const t = e.target as Node | null;
    if (!t || !tileRoot) return;
    if (methodMenuOpen || envMenuOpen) {
      const inside = tileRoot.contains(t);
      if (!inside) {
        methodMenuOpen = false;
        envMenuOpen = false;
      }
    }
  }

  $effect(() => {
    document.addEventListener('pointerdown', handleDocPointerDown);
    return () => document.removeEventListener('pointerdown', handleDocPointerDown);
  });

  // Drafts are keyed by the numeric topbar tab id. The adapter passes the
  // request UUID, so resolve the matching topbar tab to get its numeric id.
  const topbarTab = $derived($tabs.find((t) => t.mode === 'rest' && t.key === tabId));
  const topbarTabId = $derived(topbarTab?.id ?? -1);
  const draft = $derived(topbarTabId >= 0 ? $draftRequests.get(topbarTabId) : undefined);
  const isDirty = $derived(!!draft);

  // draft-first display values so canvas edits feel immediate
  const method = $derived((draft?.method ?? saved?.method ?? 'GET').toUpperCase());
  const url = $derived(draft?.url ?? saved?.url ?? '');
  const body = $derived(draft?.body ?? saved?.body ?? '');
  const bodyType = $derived(draft?.bodyType ?? saved?.bodyType ?? 'json');
  const headers = $derived<KVInput[]>(
    draft?.headers ??
      saved?.headers?.map((h) => ({ key: h.key, value: h.value, enabled: h.enabled })) ??
      [],
  );
  const savedParams = $derived<KVInput[]>(
    draft?.params ??
      saved?.params?.map((p) => ({ key: p.key, value: p.value, enabled: p.enabled })) ??
      [],
  );
  const params = $derived.by(() => {
    const qIdx = url.indexOf('?');
    if (qIdx === -1) return savedParams;
    const qs = url.substring(qIdx + 1);
    if (!qs) return savedParams;
    const urlParams: KVInput[] = qs.split('&').filter(Boolean).map((pair) => {
      const eqIdx = pair.indexOf('=');
      if (eqIdx >= 0) {
        return {
          key: safeDecodeURIComponent(pair.substring(0, eqIdx)),
          value: safeDecodeURIComponent(pair.substring(eqIdx + 1)),
          enabled: 1,
        };
      }
      return { key: safeDecodeURIComponent(pair), value: '', enabled: 1 };
    });
    const urlKeys = new Set(urlParams.map((p) => p.key));
    const extra = savedParams.filter((p) => !urlKeys.has(p.key));
    return [...urlParams, ...extra];
  });
  const authType = $derived(draft?.authType ?? saved?.authType ?? 'none');
  const authData = $derived(draft?.authData ?? saved?.authData ?? '{}');
  const preScriptVal = $derived(draft?.preScript ?? saved?.preScript ?? '');
  const methodStyle = $derived(METHOD_COLORS[method] ?? METHOD_COLORS.GET);
  const title = $derived(saved?.name ?? topbarTab?.label ?? 'Untitled');

  const effectiveEnvId = $derived(
    getEffectiveEnvId(tabId, $requestEnvOverrides, $activeEnvId),
  );

  // Load the saved request so we can fall back when the draft is empty.
  $effect(() => {
    const id = tabId;
    saved = null;
    loadError = null;
    getRequest(id)
      .then((r) => { if (tabId === id) saved = r; })
      .catch((e) => { if (tabId === id) loadError = friendlyError(e); });
  });

  // Auto-switch the default editor tab when the method type changes the body availability.
  let prevMethod = '';
  $effect(() => {
    const m = method;
    if (m !== prevMethod) {
      prevMethod = m;
      if (BODY_METHODS.includes(m) && activeEditorTab === 'params') {
        activeEditorTab = 'body';
      } else if (!BODY_METHODS.includes(m) && activeEditorTab === 'body') {
        activeEditorTab = 'headers';
      }
    }
  });

  function writeDraft(patch: Partial<import('$lib/shared/stores/tabs').DraftRequest>) {
    if (topbarTabId < 0) return;
    setDraft(topbarTabId, patch);
    markDirty(topbarTabId);
  }

  function handleMethodSelect(m: string) {
    methodMenuOpen = false;
    writeDraft({ method: m });
  }

  function handleUrlInput(e: Event) {
    const v = (e.target as HTMLInputElement).value;
    writeDraft({ url: v });
  }

  function handleHeadersChange(items: KVInput[]) {
    writeDraft({ headers: items });
  }

  function handleParamsChange(items: KVInput[]) {
    const currentUrl = url;
    const baseUrl = currentUrl.split('?')[0];
    const enabledParams = items.filter((p) => p.enabled && p.key.trim());
    let nextUrl = currentUrl;
    if (enabledParams.length > 0) {
      const qs = enabledParams
        .map((p) => `${encodeURIComponent(p.key)}=${encodeURIComponent(p.value)}`)
        .join('&');
      nextUrl = `${baseUrl}?${qs}`;
    } else if (currentUrl.includes('?')) {
      nextUrl = baseUrl;
    }
    writeDraft({ params: items, url: nextUrl });
  }

  function handleAuthChange(nextType: string, nextData: string) {
    writeDraft({ authType: nextType, authData: nextData });
  }

  function handleBodyChange(nextBody: string, nextType: string) {
    writeDraft({ body: nextBody, bodyType: nextType });
  }

  function handlePreScriptInput(e: Event) {
    const v = (e.target as HTMLTextAreaElement).value;
    writeDraft({ preScript: v });
  }

  async function send() {
    if (loading) return;
    if (!url.trim()) {
      showToast('Enter a URL first', 'error');
      return;
    }
    loading = true;
    response = null;

    const reqId = topbarTab?.key ?? null;
    const envId = effectiveEnvId ?? '';
    let resp: HttpResponse | null = null;

    try {
      if (reqId && saved && saved.id === reqId) {
        if (isDirty && topbarTabId >= 0) {
          const d = getDraft(topbarTabId);
          if (d) await commitRequest(reqId, d);
          // Refresh saved snapshot so subsequent re-sends without edits skip the commit.
          try { saved = await getRequest(reqId); } catch {}
        }
        resp = await executeRequest(reqId, envId);
      } else {
        const enabledHeaders: [string, string][] = headers
          .filter((h) => h.enabled && h.key.trim())
          .map((h) => [h.key, h.value]);
        let finalUrl = url.trim();
        const enabledParams = (draft?.params ?? [])
          .filter((p) => p.enabled && p.key.trim());
        if (enabledParams.length > 0 && !finalUrl.includes('?')) {
          const qs = enabledParams
            .map((p) => `${encodeURIComponent(p.key)}=${encodeURIComponent(p.value)}`)
            .join('&');
          finalUrl = `${finalUrl}?${qs}`;
        }
        resp = await quickExecute(
          method,
          finalUrl,
          body,
          enabledHeaders,
          envId,
          authType,
          authData,
          bodyType,
        );
      }
      if (resp) {
        showToast(
          `${resp.status} ${resp.status_text}`,
          resp.status > 0 && resp.status < 400 ? 'success' : 'error',
        );
      }
      loadHistory();
    } catch (e) {
      const msg = friendlyError(e);
      showToast(msg, 'error');
      resp = {
        status: 0,
        status_text: 'Error',
        headers: [],
        body: msg,
        duration_ms: 0,
        size_bytes: 0,
      };
    } finally {
      response = resp;
      loading = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      send();
    }
  }

  const editorTabs: { id: EditorTab; label: string }[] = [
    { id: 'headers', label: 'Headers' },
    { id: 'auth', label: 'Auth' },
    { id: 'params', label: 'Params' },
    { id: 'body', label: 'Body' },
    { id: 'pre-script', label: 'Pre-script' },
  ];
</script>

<svelte:window onkeydown={onKeydown} />

<div class="tile">
  {#if loadError && !saved && !draft}
    <div class="tile-empty">{loadError}</div>
  {:else}
    <div bind:this={tileRoot} class="tile-inner">
    <div class="tile-title">
      <span class="tile-title-text">{title}</span>
      {#if isDirty}<span class="tile-dirty" title="Unsaved changes">.</span>{/if}
    </div>

    <div class="tile-bar">
      <div class="method-wrap">
        <button
          type="button"
          class="method-pill"
          style="color: {methodStyle.color}; background: {methodStyle.bg};"
          onclick={() => (methodMenuOpen = !methodMenuOpen)}
        >
          {method}
          <svg class="method-caret" viewBox="0 0 24 24" width="9" height="9"><path d="M6 9l6 6 6-6" stroke="currentColor" fill="none" stroke-width="2" stroke-linecap="round"/></svg>
        </button>
        {#if methodMenuOpen}
          <div class="method-menu">
            {#each METHODS as m (m)}
              {@const mc = METHOD_COLORS[m] ?? METHOD_COLORS.GET}
              <button
                type="button"
                class="method-menu-item"
                style="color: {mc.color};"
                onclick={() => handleMethodSelect(m)}
              >
                {m}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <input
        type="text"
        class="url-input"
        value={url}
        placeholder="https://example.com/api"
        oninput={handleUrlInput}
        spellcheck="false"
      />

      <div class="env-wrap">
        <button
          type="button"
          class="env-pill"
          onclick={() => (envMenuOpen = !envMenuOpen)}
          title="Active environment"
        >
          {envName}
          <svg class="method-caret" viewBox="0 0 24 24" width="9" height="9"><path d="M6 9l6 6 6-6" stroke="currentColor" fill="none" stroke-width="2" stroke-linecap="round"/></svg>
        </button>
        {#if envMenuOpen}
          <div class="env-menu">
            <button
              type="button"
              class="env-menu-item"
              class:active={!$activeEnvId}
              onclick={() => { void setActiveEnv(''); envMenuOpen = false; }}
            >No env</button>
            {#each $environments as e (e.id)}
              <button
                type="button"
                class="env-menu-item"
                class:active={$activeEnvId === e.id}
                onclick={() => { void setActiveEnv(e.id); envMenuOpen = false; }}
              >{e.name}</button>
            {/each}
          </div>
        {/if}
      </div>

      <button
        type="button"
        class="send-btn"
        disabled={loading || !url.trim()}
        onclick={send}
      >
        {loading ? 'Sending...' : 'Send'}
      </button>
    </div>

    <div class="tile-editor-tabs">
      {#each editorTabs as t (t.id)}
        <button
          type="button"
          class="ed-tab"
          class:active={activeEditorTab === t.id}
          onclick={() => (activeEditorTab = t.id)}
        >
          {t.label}
          {#if t.id === 'headers' && headers.length > 0}<span class="ed-count">{headers.length}</span>{/if}
          {#if t.id === 'params' && params.length > 0}<span class="ed-count">{params.length}</span>{/if}
        </button>
      {/each}
    </div>

    <div class="tile-editor">
      {#if activeEditorTab === 'headers'}
        <KVTable items={headers} label="Add header" showKeySuggestions onchange={handleHeadersChange} />
      {:else if activeEditorTab === 'auth'}
        <AuthEditor {authType} {authData} onchange={handleAuthChange} />
      {:else if activeEditorTab === 'params'}
        <KVTable items={params} label="Add param" onchange={handleParamsChange} />
      {:else if activeEditorTab === 'body'}
        {#if BODY_METHODS.includes(method)}
          <BodyEditor {body} {bodyType} onchange={handleBodyChange} />
        {:else}
          <div class="tile-empty">Body is unavailable for {method} requests.</div>
        {/if}
      {:else}
        <textarea
          class="prescript"
          value={preScriptVal}
          placeholder="// JS pre-request script. Available: pm.environment.set(key, value), pm.request"
          oninput={handlePreScriptInput}
          spellcheck="false"
        ></textarea>
      {/if}
    </div>

    <div class="tile-response">
      <ResponseViewer {response} {loading} />
    </div>
    </div>
  {/if}
</div>

<style>
  .tile {
    width: 100%;
    height: 100%;
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
    font-family: var(--ui);
    color: var(--t1);
    background: var(--n);
  }
  .tile-inner {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .env-wrap {
    position: relative;
  }
  .env-pill {
    font-family: var(--mono);
    font-size: 10px;
    padding: 4px 8px;
    border-radius: 3px;
    border: 1px solid var(--b1);
    background: var(--b0);
    color: var(--t2);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .env-pill:hover {
    color: var(--t1);
    border-color: var(--b2);
  }
  .env-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--b0);
    border: 1px solid var(--b1);
    border-radius: 4px;
    padding: 3px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    z-index: 30;
    min-width: 130px;
    box-shadow: 0 4px 12px rgba(0,0,0,0.3);
  }
  .env-menu-item {
    font-family: var(--ui);
    font-size: 11px;
    padding: 5px 10px;
    border: none;
    background: transparent;
    color: var(--t2);
    text-align: left;
    border-radius: 3px;
    cursor: pointer;
  }
  .env-menu-item:hover { background: var(--b1); color: var(--t1); }
  .env-menu-item.active { color: var(--acc); font-weight: 600; }
  .tile-title {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    border-bottom: 1px solid var(--b1);
    font-size: 11px;
    color: var(--t3);
    flex-shrink: 0;
  }
  .tile-title-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tile-dirty {
    color: var(--acc);
    font-size: 18px;
    line-height: 0;
    transform: translateY(-3px);
  }
  .tile-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--b1);
    flex-shrink: 0;
  }
  .method-wrap {
    position: relative;
  }
  .method-pill {
    font-family: var(--mono);
    font-size: 10px;
    font-weight: 600;
    padding: 4px 8px;
    border-radius: 3px;
    border: none;
    cursor: pointer;
    letter-spacing: 0.3px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .method-caret {
    opacity: 0.7;
  }
  .method-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    background: var(--b0);
    border: 1px solid var(--b1);
    border-radius: 4px;
    z-index: 10;
    box-shadow: 0 6px 20px rgba(0,0,0,0.35);
    min-width: 90px;
    display: flex;
    flex-direction: column;
    padding: 4px 0;
  }
  .method-menu-item {
    background: transparent;
    border: none;
    text-align: left;
    font-family: var(--mono);
    font-size: 11px;
    font-weight: 600;
    padding: 4px 12px;
    cursor: pointer;
  }
  .method-menu-item:hover {
    background: var(--b1);
  }
  .url-input {
    flex: 1;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--t1);
    background: var(--b0);
    border: 1px solid var(--b1);
    border-radius: 3px;
    padding: 4px 8px;
    outline: none;
    min-width: 0;
  }
  .url-input:focus {
    border-color: var(--acc);
  }
  .send-btn {
    font-family: var(--ui);
    font-size: 11px;
    font-weight: 600;
    padding: 5px 14px;
    border: 1px solid var(--b1);
    border-radius: 3px;
    background: var(--acc);
    color: var(--b0);
    cursor: pointer;
  }
  .send-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .tile-editor-tabs {
    display: flex;
    gap: 2px;
    padding: 4px 6px 0;
    border-bottom: 1px solid var(--b1);
    flex-shrink: 0;
  }
  .ed-tab {
    font-family: var(--ui);
    font-size: 11px;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--t3);
    padding: 4px 8px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .ed-tab.active {
    color: var(--t1);
    border-bottom-color: var(--acc);
  }
  .ed-count {
    background: var(--b1);
    color: var(--t3);
    border-radius: 8px;
    padding: 0 5px;
    font-size: 9px;
    font-family: var(--mono);
  }
  .tile-editor {
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-bottom: 1px solid var(--b1);
  }
  .tile-response {
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .tile-empty {
    color: var(--t4);
    font-size: 12px;
    padding: 16px;
  }
  .prescript {
    flex: 1;
    width: 100%;
    box-sizing: border-box;
    border: none;
    background: var(--b0);
    color: var(--t1);
    font-family: var(--mono);
    font-size: 12px;
    padding: 10px 12px;
    outline: none;
    resize: none;
  }
</style>
