import { invoke } from '@tauri-apps/api/core'
import { mockBootstrap } from './mock-data'
import type { AppBootstrap, ConnectRequest, PaneSnapshot, RemoteConnectionSnapshot, TrustDecision } from './types'

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

export async function listLocalDirectory(path?: string): Promise<PaneSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.panes.local
  }

  return invoke<PaneSnapshot>('list_local_directory', path === undefined ? {} : { path })
}

export async function openLocalDirectory(path: string, entryName: string): Promise<PaneSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.panes.local
  }

  return invoke<PaneSnapshot>('open_local_directory', { path, entryName })
}

export async function goUpLocalDirectory(path: string): Promise<PaneSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.panes.local
  }

  return invoke<PaneSnapshot>('go_up_local_directory', { path })
}

export async function renameLocalEntry(path: string, entryName: string, newName: string): Promise<PaneSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.panes.local
  }

  return invoke<PaneSnapshot>('rename_local_entry', { path, entryName, newName })
}

export async function deleteLocalEntry(path: string, entryName: string): Promise<PaneSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.panes.local
  }

  return invoke<PaneSnapshot>('delete_local_entry', { path, entryName })
}

export async function connectRemote(request: ConnectRequest): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('connect_remote', { request })
}

export async function resolveRemoteTrust(decision: TrustDecision): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('resolve_remote_trust', { decision })
}

export async function disconnectRemote(): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('disconnect_remote')
}

export async function refreshRemoteDirectory(): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('refresh_remote_directory')
}

export async function openRemoteDirectory(path: string): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('open_remote_directory', { path })
}

export async function goUpRemoteDirectory(): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('go_up_remote_directory')
}
