import { invoke } from '@tauri-apps/api/core'
import { mockBootstrap } from './mock-data'
import type { AppBootstrap } from './types'

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown
  }
}

export async function bootstrapAppState(): Promise<AppBootstrap> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap
  }

  try {
    return await invoke<AppBootstrap>('bootstrap_app_state')
  } catch {
    return mockBootstrap
  }
}
