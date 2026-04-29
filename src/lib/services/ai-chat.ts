import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { aiChat, recordAiUsage } from '$lib/commands/ai';
import type { ChatMessage, ChatContext } from '$lib/types/ai';
import { aiEvent } from '$lib/shared/constants/events';

export interface ChatCallbacks {
  onText: (text: string) => void;
  onToolStart: (toolName: string) => void;
  onToolEnd: (toolName: string) => void;
  onAction: (action: string, data: any) => void;
  onDone: (inputTokens: number, outputTokens: number) => void;
  onError: (error: string) => void;
}

export async function sendChatMessage(
  apiKey: string,
  messages: ChatMessage[],
  context: ChatContext,
  sessionId: string,
  systemPrompt: string,
  tools: any[],
  callbacks: ChatCallbacks,
  provider: string = 'claude',
  model: string = 'claude-haiku-4-5-20251001',
  chatMode: string = 'rest',
): Promise<() => void> {
  const unlisteners: UnlistenFn[] = [];

  unlisteners.push(await listen<{ text: string }>(aiEvent.text(sessionId), (e) => {
    callbacks.onText(e.payload.text);
  }));

  unlisteners.push(await listen<{ toolName: string }>(aiEvent.toolStart(sessionId), (e) => {
    callbacks.onToolStart(e.payload.toolName);
  }));

  unlisteners.push(await listen<{ toolName: string }>(aiEvent.toolEnd(sessionId), (e) => {
    callbacks.onToolEnd(e.payload.toolName);
  }));

  unlisteners.push(await listen<{ action: string; data: any }>(aiEvent.action(sessionId), (e) => {
    callbacks.onAction(e.payload.action, e.payload.data);
  }));

  unlisteners.push(await listen<{ inputTokens: number; outputTokens: number }>(aiEvent.done(sessionId), (e) => {
    callbacks.onDone(e.payload.inputTokens, e.payload.outputTokens);
    recordAiUsage(chatMode, model, e.payload.inputTokens, e.payload.outputTokens).catch(() => {});
  }));

  unlisteners.push(await listen<{ error: string }>(aiEvent.error(sessionId), (e) => {
    callbacks.onError(e.payload.error);
  }));

  aiChat(apiKey, messages, context, sessionId, systemPrompt, tools, provider).catch((err) => {
    callbacks.onError(typeof err === 'string' ? err : String(err));
  });

  return () => {
    for (const u of unlisteners) u();
  };
}

let sessionCounter = 0;
export function generateSessionId(): string {
  return `ai-${Date.now()}-${++sessionCounter}`;
}
