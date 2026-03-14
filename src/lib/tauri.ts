import { invoke } from '@tauri-apps/api/core'
import { mockBootstrap } from './mock-data'
import type {
  AppBootstrap,
  ConnectRequest,
  CreateRemoteDirectoryRequest,
  DeleteLocalEntriesRequest,
  DeleteRemoteEntryRequest,
  DeleteRemoteEntriesRequest,
  PaneSnapshot,
  QueueDownloadRequest,
  QueueUploadRequest,
  RemoteConnectionSnapshot,
  RemoteDeleteResponse,
  RenameRemoteEntryRequest,
  TransferConflictResolution,
  TransferQueueSnapshot,
  TrustDecision,
} from './types'

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

export async function deleteLocalEntries(request: DeleteLocalEntriesRequest): Promise<PaneSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.panes.local
  }

  return invoke<PaneSnapshot>('delete_local_entries', { request })
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

export async function createRemoteDirectory(request: CreateRemoteDirectoryRequest): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('create_remote_directory', { request })
}

export async function renameRemoteEntry(request: RenameRemoteEntryRequest): Promise<RemoteConnectionSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      session: mockBootstrap.session,
      remotePane: mockBootstrap.panes.remote,
      trustPrompt: null,
    }
  }

  return invoke<RemoteConnectionSnapshot>('rename_remote_entry', { request })
}

export async function deleteRemoteEntry(request: DeleteRemoteEntryRequest): Promise<RemoteDeleteResponse> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      snapshot: {
        session: mockBootstrap.session,
        remotePane: mockBootstrap.panes.remote,
        trustPrompt: null,
      },
      prompt: null,
    }
  }

  return invoke<RemoteDeleteResponse>('delete_remote_entry', { request })
}

export async function deleteRemoteEntries(request: DeleteRemoteEntriesRequest): Promise<RemoteDeleteResponse> {
  if (!window.__TAURI_INTERNALS__) {
    return {
      snapshot: { session: mockBootstrap.session, remotePane: mockBootstrap.panes.remote, trustPrompt: null },
      prompt: null,
    }
  }

  return invoke<RemoteDeleteResponse>('delete_remote_entries', { request })
}

export async function queueDownload(request: QueueDownloadRequest): Promise<TransferQueueSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.transfers
  }

  return invoke<TransferQueueSnapshot>('queue_download', { request })
}

export async function queueUpload(request: QueueUploadRequest): Promise<TransferQueueSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.transfers
  }

  return invoke<TransferQueueSnapshot>('queue_upload', { request })
}

export async function listTransferJobs(): Promise<TransferQueueSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.transfers
  }

  return invoke<TransferQueueSnapshot>('list_transfer_jobs')
}

export async function cancelTransfer(jobId: string): Promise<TransferQueueSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.transfers
  }

  return invoke<TransferQueueSnapshot>('cancel_transfer', { jobId })
}

export async function retryTransfer(jobId: string): Promise<TransferQueueSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.transfers
  }

  return invoke<TransferQueueSnapshot>('retry_transfer', { jobId })
}

export async function resolveTransferConflict(
  jobId: string,
  resolution: TransferConflictResolution,
): Promise<TransferQueueSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.transfers
  }

  return invoke<TransferQueueSnapshot>('resolve_transfer_conflict', { jobId, resolution })
}

export async function clearCompletedTransfers(): Promise<TransferQueueSnapshot> {
  if (!window.__TAURI_INTERNALS__) {
    return mockBootstrap.transfers
  }

  return invoke<TransferQueueSnapshot>('clear_completed_transfers')
}
