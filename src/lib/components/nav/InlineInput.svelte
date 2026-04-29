<script lang="ts">
  import { onMount } from 'svelte';
  import { INLINE_INPUT_BLUR_MS } from '$lib/shared/constants/timings';

  interface Props {
    placeholder?: string;
    value?: string;
    onsubmit?: (value: string) => void;
    oncancel?: () => void;
  }

  let {
    placeholder = '',
    value: initialValue = '',
    onsubmit,
    oncancel,
  }: Props = $props();

  let inputEl: HTMLInputElement | undefined = $state();
  let inputValue = $state(initialValue);
  let done = false;

  onMount(() => {
    // Small delay to ensure DOM is settled before focusing
    requestAnimationFrame(() => {
      inputEl?.focus();
      if (initialValue) inputEl?.select();
    });
  });

  function submit() {
    if (done) return;
    const trimmed = inputValue.trim();
    if (trimmed) {
      done = true;
      onsubmit?.(trimmed);
    }
  }

  function cancel() {
    if (done) return;
    done = true;
    oncancel?.();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  }

  function handleBlur() {
    // Only cancel on blur if nothing was submitted
    setTimeout(() => {
      if (!done) cancel();
    }, INLINE_INPUT_BLUR_MS);
  }
</script>

<input
  bind:this={inputEl}
  type="text"
  class="nav-inline-input"
  {placeholder}
  bind:value={inputValue}
  onkeydown={handleKeydown}
  onblur={handleBlur}
/>

<style>
  .nav-inline-input {
    width: 100%;
    background: var(--e);
    border: 1px solid var(--b1);
    border-radius: 5px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--t1);
    font-family: var(--mono);
    outline: none;
    transition: border-color 0.15s;
    box-sizing: border-box;
  }
  .nav-inline-input:focus {
    border-color: var(--acc);
  }
</style>
