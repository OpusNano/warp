import { For, Show, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import { listen } from '@tauri-apps/api/event'
import {
  bootstrapAppState,
  cancelTransfer,
  clearCompletedTransfers,
  connectRemote,
  createRemoteDirectory,
  deleteRemoteEntry,
  deleteLocalEntry,
  disconnectRemote,
  goUpLocalDirectory,
  goUpRemoteDirectory,
  queueDownload,
  queueUpload,
  listLocalDirectory,
  openLocalDirectory,
  openRemoteDirectory,
  refreshRemoteDirectory,
  renameLocalEntry,
  renameRemoteEntry,
  resolveRemoteTrust,
  resolveTransferConflict,
} from './lib/tauri'
import type {
  AppBootstrap,
  ConnectRequest,
  FileEntry,
  PaneId,
  PaneSnapshot,
  RemoteDeletePrompt,
  TransferQueueSnapshot,
  RemoteConnectionSnapshot,
  TransferJob,
  TrustPrompt,
} from './lib/types'

const defaultState: AppBootstrap = {
  connectionProfiles: [],
  session: {
    connectionState: 'Disconnected',
    protocolMode: 'SFTP primary',
    host: 'No active session',
    authMethod: 'None',
    trustState: 'No host selected',
    lastError: null,
    canDisconnect: false,
  },
  panes: {
    local: {
      id: 'local',
      title: 'Local',
      location: '/home/cyberdyne/projects',
      itemCount: 0,
      canGoUp: true,
      entries: [],
      emptyMessage: 'Local directory is empty.',
    },
    remote: {
      id: 'remote',
      title: 'Remote',
      location: 'Not connected',
      itemCount: 0,
      canGoUp: false,
      entries: [],
      emptyMessage: 'Connect to a host to browse remote files.',
    },
  },
  transfers: {
    jobs: [],
    activeJobId: null,
    queuedCount: 0,
    finishedCount: 0,
  },
  shortcuts: [],
}

function formatSize(sizeBytes: number | null) {
  if (sizeBytes === null) return '--'
  if (sizeBytes < 1024) return `${sizeBytes} B`

  const units = ['KB', 'MB', 'GB', 'TB']
  let value = sizeBytes / 1024
  let unit = units[0]

  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024
    unit = units[index]
  }

  return `${value < 10 ? value.toFixed(1) : value.toFixed(0)} ${unit}`
}

function formatModified(modifiedUnixMs: number | null) {
  if (modifiedUnixMs === null) return '--'

  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(modifiedUnixMs))
}

function entryTone(kind: FileEntry['kind']) {
  switch (kind) {
    case 'dir':
      return 'text-white'
    case 'symlink':
      return 'text-zinc-300'
    default:
      return 'text-zinc-400'
  }
}

function transferTone(state: TransferJob['state']) {
  switch (state) {
    case 'Checking':
    case 'Running':
    case 'Cancelling':
      return 'text-white'
    case 'Succeeded':
      return 'text-emerald-300'
    case 'Failed':
      return 'text-red-300'
    case 'AwaitingConflictDecision':
      return 'text-amber-200'
    default:
      return 'text-zinc-400'
  }
}

function transferStateLabel(state: TransferJob['state']) {
  switch (state) {
    case 'AwaitingConflictDecision':
      return 'Conflict'
    default:
      return state
  }
}

function parentDirectory(path: string) {
  if (!path || path === '/' || path === 'Not connected') return path
  const normalized = path.endsWith('/') && path !== '/' ? path.slice(0, -1) : path
  const index = normalized.lastIndexOf('/')
  if (index <= 0) return normalized.startsWith('/') ? '/' : '.'
  return normalized.slice(0, index)
}

function isEditableTarget(target: EventTarget | null) {
  return target instanceof HTMLElement && (target.closest('input, textarea, [contenteditable="true"]') !== null)
}

function renameSelectionRange(entry: FileEntry) {
  if (entry.kind !== 'file') {
    return [0, entry.name.length] as const
  }

  if (entry.name.startsWith('.') && !entry.name.slice(1).includes('.')) {
    return [0, entry.name.length] as const
  }

  const extensionIndex = entry.name.lastIndexOf('.')
  if (extensionIndex <= 0) {
    return [0, entry.name.length] as const
  }

  return [0, extensionIndex] as const
}

function remotePlaceholder(emptyMessage: string): PaneSnapshot {
  return {
    id: 'remote',
    title: 'Remote',
    location: 'Not connected',
    itemCount: 0,
    canGoUp: false,
    entries: [],
    emptyMessage,
  }
}

function App() {
  const [session, setSession] = createSignal(defaultState.session)
  const [localPane, setLocalPane] = createSignal(defaultState.panes.local)
  const [remotePane, setRemotePane] = createSignal(defaultState.panes.remote)
  const [transfers, setTransfers] = createSignal<TransferQueueSnapshot>(defaultState.transfers)
  const [shortcuts, setShortcuts] = createSignal(defaultState.shortcuts)
  const [activePane, setActivePane] = createSignal<PaneId>('local')
  const [dividerRatio, setDividerRatio] = createSignal(0.5)
  const [dragging, setDragging] = createSignal(false)
  const [localFilter, setLocalFilter] = createSignal('')
  const [remoteFilter, setRemoteFilter] = createSignal('')
  const [localSelection, setLocalSelection] = createSignal<string | null>(null)
  const [remoteSelection, setRemoteSelection] = createSignal<string | null>(null)
  const [localLoading, setLocalLoading] = createSignal(false)
  const [remoteLoading, setRemoteLoading] = createSignal(false)
  const [localError, setLocalError] = createSignal<string | null>(null)
  const [connectError, setConnectError] = createSignal<string | null>(null)
  const [remoteRuntimeError, setRemoteRuntimeError] = createSignal<string | null>(null)
  const [trustPrompt, setTrustPrompt] = createSignal<TrustPrompt | null>(null)
  const [renamingPane, setRenamingPane] = createSignal<PaneId | null>(null)
  const [renamingEntryName, setRenamingEntryName] = createSignal<string | null>(null)
  const [renameDraft, setRenameDraft] = createSignal('')
  const [creatingDirectoryPane, setCreatingDirectoryPane] = createSignal<PaneId | null>(null)
  const [createDirectoryDraft, setCreateDirectoryDraft] = createSignal('')
  const [pendingDeleteTarget, setPendingDeleteTarget] = createSignal<{
    paneId: PaneId
    entry: FileEntry
    recursive: boolean
    message?: string
  } | null>(null)
  const [connectHost, setConnectHost] = createSignal('')
  const [connectPort, setConnectPort] = createSignal('22')
  const [connectUsername, setConnectUsername] = createSignal('')
  const [connectAuthMode, setConnectAuthMode] = createSignal<'password' | 'key'>('password')
  const [connectPassword, setConnectPassword] = createSignal('')
  const [connectPrivateKeyPath, setConnectPrivateKeyPath] = createSignal('')
  const [connectPassphrase, setConnectPassphrase] = createSignal('')

  let localPaneElement: HTMLElement | undefined
  let remotePaneElement: HTMLElement | undefined
  let localFilterInput: HTMLInputElement | undefined
  let remoteFilterInput: HTMLInputElement | undefined
  let createDirectoryInput: HTMLInputElement | undefined
  let deleteConfirmButton: HTMLButtonElement | undefined

  const localSelectedEntry = createMemo(() => {
    const selection = localSelection()
    if (!selection) return null
    return localPane().entries.find((entry) => entry.name === selection) ?? null
  })

  const remoteSelectedEntry = createMemo(() => {
    const selection = remoteSelection()
    if (!selection) return null
    return remotePane().entries.find((entry) => entry.name === selection) ?? null
  })

  const applyTransferSnapshot = (snapshot: TransferQueueSnapshot) => {
    const previousJobs = new Map(transfers().jobs.map((job) => [job.id, job.state]))
    setTransfers(snapshot)

    for (const job of snapshot.jobs) {
      if (job.state !== 'Succeeded' || previousJobs.get(job.id) === 'Succeeded') {
        continue
      }

      if (job.direction === 'Download' && parentDirectory(job.destinationPath) === localPane().location) {
        void refreshPane('local')
      }

      if (job.direction === 'Upload' && parentDirectory(job.destinationPath) === remotePane().location) {
        void refreshPane('remote')
      }
    }
  }

  const handleRemoteSessionUpdate = (snapshot: RemoteConnectionSnapshot) => {
    setRemoteLoading(false)
    applyRemoteSnapshot(snapshot, remoteSelection())
    if (snapshot.session.connectionState === 'Disconnected') {
      clearPaneTransientUi('remote')
      setTrustPrompt(null)
      setRemoteSelection(null)
    }
  }

  const clearRemoteTransientState = (emptyMessage: string) => {
    setRemotePane(remotePlaceholder(emptyMessage))
    setRemoteSelection(null)
  }

  const setTransientRemoteSession = (connectionState: string, trustState: string, hostOverride?: string) => {
    const request = buildConnectRequest()
    const authMethod = request.auth.kind === 'password' ? 'Password' : 'SSH key'
    const host = hostOverride ?? (request.host && request.username ? `${request.username}@${request.host}:${request.port}` : session().host)

    setSession({
      ...session(),
      connectionState,
      protocolMode: 'SFTP primary',
      host,
      authMethod,
      trustState,
      lastError: null,
      canDisconnect: false,
    })
  }

  onMount(async () => {
    if (window.__TAURI_INTERNALS__) {
      const unlistenTransfers = await listen<TransferQueueSnapshot>('transfer-queue-updated', (event) => {
        applyTransferSnapshot(event.payload)
      })
      const unlistenSession = await listen<RemoteConnectionSnapshot>('remote-session-updated', (event) => {
        handleRemoteSessionUpdate(event.payload)
      })
      onCleanup(() => {
        void unlistenTransfers()
        void unlistenSession()
      })
    }

    const state = await bootstrapAppState()

    setSession(state.session)
    setLocalPane(state.panes.local)
    setRemotePane(state.panes.remote)
    applyTransferSnapshot(state.transfers)
    setShortcuts(state.shortcuts)
    syncSelection('local', state.panes.local, null)
    syncSelection('remote', state.panes.remote, null)
  })

  const applyRemoteSnapshot = (snapshot: RemoteConnectionSnapshot, preferredName: string | null = null) => {
    setSession(snapshot.session)
    setRemotePane(snapshot.remotePane)
    setTrustPrompt(snapshot.trustPrompt)
    if (snapshot.session.connectionState === 'Disconnected') {
      clearPaneTransientUi('remote')
    }
    const isRuntimeError = snapshot.remotePane.location !== 'Not connected' || snapshot.remotePane.entries.length > 0
    setConnectError(isRuntimeError ? null : snapshot.session.lastError)
    setRemoteRuntimeError(isRuntimeError ? snapshot.session.lastError : null)
    syncSelection('remote', snapshot.remotePane, preferredName)
  }

  const buildConnectRequest = (): ConnectRequest => ({
    host: connectHost().trim(),
    port: Number.parseInt(connectPort().trim(), 10) || 22,
    username: connectUsername().trim(),
    auth:
      connectAuthMode() === 'password'
        ? { kind: 'password', password: connectPassword() }
        : {
            kind: 'key',
            privateKeyPath: connectPrivateKeyPath().trim(),
            passphrase: connectPassphrase().trim() || null,
          },
  })

  const showRemoteValidationError = (message: string) => {
    setConnectError(message)
    setRemoteRuntimeError(null)
    setSession({ ...session(), lastError: message })
  }

  const submitConnect = async () => {
    const request = buildConnectRequest()

    if (!request.host || !request.username) {
      showRemoteValidationError('Host and username are required.')
      return
    }

    if (!Number.isInteger(request.port) || request.port < 1 || request.port > 65535) {
      showRemoteValidationError('Port must be between 1 and 65535.')
      return
    }

    if (request.auth.kind === 'key' && request.auth.privateKeyPath.length === 0) {
      showRemoteValidationError('Private key path is required for SSH key authentication.')
      return
    }

    setRemoteLoading(true)
    setConnectError(null)
    setRemoteRuntimeError(null)
    setTrustPrompt(null)
    clearRemoteTransientState('Connecting to remote host...')
    setTransientRemoteSession('Connecting', 'Verifying host key')

    try {
      const snapshot = await connectRemote(request)
      applyRemoteSnapshot(snapshot)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to connect to remote host.'
      setConnectError(message)
      setSession({ ...session(), lastError: message })
    } finally {
      setRemoteLoading(false)
    }
  }

  const submitDisconnect = async () => {
    setRemoteLoading(true)
    setConnectError(null)
    setRemoteRuntimeError(null)
    setTrustPrompt(null)
    clearRemoteTransientState('Disconnecting remote session...')
    setSession({
      ...session(),
      connectionState: 'Disconnecting',
      trustState: 'Clearing session',
      lastError: null,
      canDisconnect: false,
    })

    try {
      const snapshot = await disconnectRemote()
      applyRemoteSnapshot(snapshot, null)
      setRemoteSelection(null)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to disconnect remote session.'
      setConnectError(message)
      setSession({ ...session(), lastError: message })
    } finally {
      setRemoteLoading(false)
    }
  }

  const handleTrustDecision = async (trust: boolean) => {
    setRemoteLoading(true)
    setConnectError(null)
    setRemoteRuntimeError(null)

    if (trust) {
      clearRemoteTransientState('Authenticating with remote host...')
      setTransientRemoteSession('Authenticating', 'Trust accepted')
    }

    try {
      const snapshot = await resolveRemoteTrust({ trust })
      applyRemoteSnapshot(snapshot)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to resolve trust decision.'
      setConnectError(message)
      setSession({ ...session(), lastError: message })
    } finally {
      setRemoteLoading(false)
    }
  }

  const resize = (clientX: number) => {
    const root = document.getElementById('workspace-shell')
    if (!root) return

    const rect = root.getBoundingClientRect()
    const next = (clientX - rect.left) / rect.width
    setDividerRatio(Math.min(0.72, Math.max(0.28, next)))
  }

  const onPointerMove = (event: PointerEvent) => {
    if (!dragging()) return
    resize(event.clientX)
  }

  const stopDragging = () => setDragging(false)

  const filteredEntries = (pane: PaneSnapshot, filterValue: string) => {
    const filter = filterValue.trim().toLowerCase()

    if (filter.length === 0) return pane.entries

    return pane.entries.filter((entry) => {
      const haystack = `${entry.name} ${entry.permissions} ${entry.path}`.toLowerCase()
      return haystack.includes(filter)
    })
  }

  const localEntries = createMemo(() => filteredEntries(localPane(), localFilter()))
  const remoteEntries = createMemo(() => filteredEntries(remotePane(), remoteFilter()))
  const orderedTransferJobs = createMemo(() => [...transfers().jobs].reverse())

  const paneClass = (paneId: PaneId) =>
    activePane() === paneId
      ? 'border-white/70 bg-white/[0.03] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.12)]'
      : 'border-white/10 bg-white/[0.015]'

  const selectionForPane = (paneId: PaneId) => (paneId === 'local' ? localSelection() : remoteSelection())

  const setSelectionForPane = (paneId: PaneId, value: string | null) => {
    if (paneId === 'local') {
      setLocalSelection(value)
      return
    }

    setRemoteSelection(value)
  }

  const selectedEntryForPane = (paneId: PaneId) => (paneId === 'local' ? localSelectedEntry() : remoteSelectedEntry())

  const cancelInlineRename = (paneId?: PaneId) => {
    if (paneId !== undefined && renamingPane() !== paneId) return
    setRenamingPane(null)
    setRenamingEntryName(null)
    setRenameDraft('')
  }

  const cancelCreateDirectory = (paneId?: PaneId) => {
    if (paneId !== undefined && creatingDirectoryPane() !== paneId) return
    setCreatingDirectoryPane(null)
    setCreateDirectoryDraft('')
  }

  const closeDeleteConfirmation = (paneId?: PaneId) => {
    if (paneId !== undefined && pendingDeleteTarget()?.paneId !== paneId) return
    setPendingDeleteTarget(null)
  }

  const syncSelection = (paneId: PaneId, pane: PaneSnapshot, preferredName: string | null) => {
    const currentSelection = selectionForPane(paneId)
    const nextSelection =
      [preferredName, currentSelection]
        .filter((value): value is string => Boolean(value))
        .find((value) => pane.entries.some((entry) => entry.name === value)) ?? pane.entries[0]?.name ?? null

    setSelectionForPane(paneId, nextSelection)
  }

  const setPaneSnapshot = (paneId: PaneId, pane: PaneSnapshot, preferredName: string | null) => {
    if (paneId === 'local') {
      setLocalPane(pane)
      syncSelection('local', pane, preferredName)
      return
    }

    setRemotePane(pane)
    syncSelection('remote', pane, preferredName)
  }

  const focusPane = (paneId: PaneId) => {
    setActivePane(paneId)
    if (paneId === 'local') {
      localPaneElement?.focus()
      return
    }

    remotePaneElement?.focus()
  }

  const activatePane = (paneId: PaneId) => {
    setActivePane(paneId)
  }

  const focusPaneFilter = (paneId: PaneId) => {
    focusPane(paneId)
    if (paneId === 'local') {
      localFilterInput?.focus()
      localFilterInput?.select()
      return
    }

    remoteFilterInput?.focus()
    remoteFilterInput?.select()
  }

  const clearPaneTransientUi = (paneId: PaneId) => {
    cancelInlineRename(paneId)
    cancelCreateDirectory(paneId)
    closeDeleteConfirmation(paneId)
  }

  const refreshPane = async (paneId: PaneId) => {
    if (paneId === 'local') {
      setLocalLoading(true)
      setLocalError(null)
      clearPaneTransientUi('local')

      try {
        const nextPane = await listLocalDirectory(localPane().location)
        setPaneSnapshot('local', nextPane, localSelection())
      } catch (error) {
        setLocalError(error instanceof Error ? error.message : 'Failed to refresh local directory.')
      } finally {
        setLocalLoading(false)
      }

      return
    }

    setRemoteLoading(true)
    setRemoteRuntimeError(null)
    setConnectError(null)
    clearPaneTransientUi('remote')
    setSession({ ...session(), connectionState: 'Refreshing', lastError: null })

    try {
      const snapshot = await refreshRemoteDirectory()
      applyRemoteSnapshot(snapshot, remoteSelection())
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to refresh remote directory.'
      setRemoteRuntimeError(message)
      setSession({ ...session(), lastError: null })
    } finally {
      setRemoteLoading(false)
    }
  }

  const openEntry = async (paneId: PaneId, entry: FileEntry) => {
    setSelectionForPane(paneId, entry.name)
    activatePane(paneId)

    if (entry.kind !== 'dir') return

    if (paneId === 'local') {
      setLocalLoading(true)
      setLocalError(null)
      clearPaneTransientUi('local')

      try {
        const nextPane = await openLocalDirectory(localPane().location, entry.name)
        setPaneSnapshot('local', nextPane, null)
      } catch (error) {
        setLocalError(error instanceof Error ? error.message : 'Failed to open directory.')
      } finally {
        setLocalLoading(false)
      }

      return
    }

    setRemoteLoading(true)
    setRemoteRuntimeError(null)
    setConnectError(null)
    clearPaneTransientUi('remote')
    setSession({ ...session(), connectionState: 'Opening directory', lastError: null })

    try {
      const snapshot = await openRemoteDirectory(entry.path)
      applyRemoteSnapshot(snapshot, null)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to open remote directory.'
      setRemoteRuntimeError(message)
      setSession({ ...session(), lastError: null })
    } finally {
      setRemoteLoading(false)
    }
  }

  const goUpInPane = async (paneId: PaneId) => {
    if (paneId === 'local') {
      if (!localPane().canGoUp) return

      const currentName = localPane().location.split('/').filter(Boolean).at(-1) ?? null

      setLocalLoading(true)
      setLocalError(null)
      clearPaneTransientUi('local')

      try {
        const nextPane = await goUpLocalDirectory(localPane().location)
        setPaneSnapshot('local', nextPane, currentName)
      } catch (error) {
        setLocalError(error instanceof Error ? error.message : 'Failed to navigate to parent directory.')
      } finally {
        setLocalLoading(false)
      }

      return
    }

    if (!remotePane().canGoUp) return

    setRemoteLoading(true)
    setRemoteRuntimeError(null)
    setConnectError(null)
    clearPaneTransientUi('remote')
    setSession({ ...session(), connectionState: 'Opening directory', lastError: null })

    try {
      const snapshot = await goUpRemoteDirectory()
      applyRemoteSnapshot(snapshot, null)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to navigate to parent remote directory.'
      setRemoteRuntimeError(message)
      setSession({ ...session(), lastError: null })
    } finally {
      setRemoteLoading(false)
    }
  }

  const moveSelection = (paneId: PaneId, delta: number) => {
    const entries = paneId === 'local' ? localEntries() : remoteEntries()
    if (entries.length === 0) return

    const currentSelection = selectionForPane(paneId)
    const currentIndex = entries.findIndex((entry) => entry.name === currentSelection)
    const nextIndex = currentIndex === -1 ? 0 : Math.min(entries.length - 1, Math.max(0, currentIndex + delta))
    setSelectionForPane(paneId, entries[nextIndex]?.name ?? null)
    activatePane(paneId)
  }

  const openSelectedEntry = async (paneId: PaneId) => {
    const entries = paneId === 'local' ? localEntries() : remoteEntries()
    const selection = selectionForPane(paneId)
    const entry = entries.find((item) => item.name === selection) ?? entries[0]

    if (!entry) return
    await openEntry(paneId, entry)
  }

  const startInlineRename = (paneId: PaneId) => {
    const entry = selectedEntryForPane(paneId)
    if (!entry) return

    closeDeleteConfirmation(paneId)
    cancelCreateDirectory(paneId)
    setRenamingPane(paneId)
    setRenamingEntryName(entry.name)
    setRenameDraft(entry.name)
  }

  const commitInlineRename = async (paneId: PaneId) => {
    const entry = selectedEntryForPane(paneId)
    const currentRenamingEntry = renamingEntryName()
    const nextName = renameDraft().trim()

    if (!entry || !currentRenamingEntry || renamingPane() !== paneId || entry.name !== currentRenamingEntry) {
      cancelInlineRename(paneId)
      return
    }

    if (nextName.length === 0 || nextName === entry.name) {
      cancelInlineRename(paneId)
      return
    }

    if (paneId === 'local') {
      setLocalLoading(true)
      setLocalError(null)

      try {
        const nextPane = await renameLocalEntry(localPane().location, entry.name, nextName)
        setPaneSnapshot('local', nextPane, nextName)
        cancelInlineRename('local')
      } catch (error) {
        setLocalError(error instanceof Error ? error.message : 'Failed to rename entry.')
      } finally {
        setLocalLoading(false)
      }

      return
    }

    setRemoteLoading(true)
    setRemoteRuntimeError(null)
    setConnectError(null)
    setSession({ ...session(), connectionState: 'Renaming remote entry', lastError: null })

    try {
      const snapshot = await renameRemoteEntry({
        parentPath: remotePane().location,
        entryName: entry.name,
        newName: nextName,
      })
      applyRemoteSnapshot(snapshot, nextName)
      cancelInlineRename('remote')
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to rename remote entry.'
      setRemoteRuntimeError(message)
      setSession({ ...session(), lastError: null })
    } finally {
      setRemoteLoading(false)
    }
  }

  const startCreateDirectory = (paneId: PaneId) => {
    closeDeleteConfirmation(paneId)
    cancelInlineRename(paneId)
    setCreatingDirectoryPane(paneId)
    setCreateDirectoryDraft('')
    queueMicrotask(() => createDirectoryInput?.focus())
  }

  const commitCreateDirectory = async (paneId: PaneId) => {
    const nextName = createDirectoryDraft().trim()

    if (creatingDirectoryPane() !== paneId) {
      cancelCreateDirectory(paneId)
      return
    }

    if (nextName.length === 0) {
      cancelCreateDirectory(paneId)
      return
    }

    if (paneId !== 'remote') {
      cancelCreateDirectory(paneId)
      return
    }

    setRemoteLoading(true)
    setRemoteRuntimeError(null)
    setConnectError(null)
    setSession({ ...session(), connectionState: 'Creating remote directory', lastError: null })

    try {
      const snapshot = await createRemoteDirectory({
        parentPath: remotePane().location,
        name: nextName,
      })
      applyRemoteSnapshot(snapshot, nextName)
      cancelCreateDirectory('remote')
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to create remote directory.'
      setRemoteRuntimeError(message)
      setSession({ ...session(), lastError: null })
    } finally {
      setRemoteLoading(false)
    }
  }

  const openDeleteConfirmation = (paneId: PaneId) => {
    const entry = selectedEntryForPane(paneId)
    if (!entry) return

    cancelInlineRename(paneId)
    cancelCreateDirectory(paneId)
    setPendingDeleteTarget({ paneId, entry, recursive: false })
    queueMicrotask(() => deleteConfirmButton?.focus())
  }

  const applyRemoteDeletePrompt = (prompt: RemoteDeletePrompt, entry: FileEntry) => {
    setPendingDeleteTarget({
      paneId: 'remote',
      entry,
      recursive: prompt.requiresRecursive,
      message: prompt.message,
    })
    queueMicrotask(() => deleteConfirmButton?.focus())
  }

  const confirmDelete = async () => {
    const target = pendingDeleteTarget()
    if (!target) return

    if (target.paneId === 'local') {
      setLocalLoading(true)
      setLocalError(null)

      try {
        const nextPane = await deleteLocalEntry(localPane().location, target.entry.name)
        setPaneSnapshot('local', nextPane, null)
        closeDeleteConfirmation('local')
      } catch (error) {
        setLocalError(error instanceof Error ? error.message : 'Failed to delete entry.')
      } finally {
        setLocalLoading(false)
      }

      return
    }

    setRemoteLoading(true)
    setRemoteRuntimeError(null)
    setConnectError(null)

    try {
      const response = await deleteRemoteEntry({
        parentPath: remotePane().location,
        entryName: target.entry.name,
        entryKind: target.entry.kind,
        recursive: target.recursive,
      })

      if (response.prompt) {
        applyRemoteSnapshot(response.snapshot, target.entry.name)
        applyRemoteDeletePrompt(response.prompt, target.entry)
      } else {
        applyRemoteSnapshot(response.snapshot, null)
        closeDeleteConfirmation('remote')
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to delete remote entry.'
      setRemoteRuntimeError(message)
    } finally {
      setRemoteLoading(false)
    }
  }

  const uploadCandidate = createMemo(() => {
    const entry = localSelectedEntry()
    if (!entry || entry.kind !== 'file') return null
    if (session().connectionState !== 'Connected' || remotePane().location === 'Not connected') return null
    return entry
  })

  const downloadCandidate = createMemo(() => {
    const entry = remoteSelectedEntry()
    if (!entry || entry.kind !== 'file') return null
    if (session().connectionState !== 'Connected') return null
    return entry
  })

  const queueSelectedUpload = async () => {
    const entry = uploadCandidate()
    if (!entry) return

    try {
      const snapshot = await queueUpload({
        localPath: entry.path,
        localName: entry.name,
        remoteDirectory: remotePane().location,
      })
      applyTransferSnapshot(snapshot)
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : 'Failed to queue upload.')
    }
  }

  const queueSelectedDownload = async () => {
    const entry = downloadCandidate()
    if (!entry) return

    try {
      const snapshot = await queueDownload({
        remotePath: entry.path,
        remoteName: entry.name,
        localDirectory: localPane().location,
      })
      applyTransferSnapshot(snapshot)
    } catch (error) {
      setRemoteRuntimeError(error instanceof Error ? error.message : 'Failed to queue download.')
    }
  }

  const cancelTransferJob = async (jobId: string) => {
    try {
      const snapshot = await cancelTransfer(jobId)
      applyTransferSnapshot(snapshot)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to cancel transfer.'
      setRemoteRuntimeError(message)
    }
  }

  const resolveTransferJobConflict = async (jobId: string, action: 'overwrite' | 'cancel') => {
    try {
      const snapshot = await resolveTransferConflict(jobId, { action })
      applyTransferSnapshot(snapshot)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to resolve transfer conflict.'
      setRemoteRuntimeError(message)
    }
  }

  const clearTransferHistory = async () => {
    try {
      const snapshot = await clearCompletedTransfers()
      applyTransferSnapshot(snapshot)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to clear transfer history.'
      setRemoteRuntimeError(message)
    }
  }

  const selectedCount = (paneId: PaneId) => {
    const pane = paneId === 'local' ? localPane() : remotePane()
    const selection = selectionForPane(paneId)
    return selection !== null && pane.entries.some((entry) => entry.name === selection) ? 1 : 0
  }

  const handleShortcut = (event: KeyboardEvent) => {
    if (dragging()) return

    if (pendingDeleteTarget()) {
      if (event.key === 'Escape') {
        event.preventDefault()
        closeDeleteConfirmation()
        return
      }

      if (event.key === 'Enter') {
        event.preventDefault()
        void confirmDelete()
      }

      return
    }

    const editableTarget = isEditableTarget(event.target)

    if ((event.key === 'Tab' && !editableTarget) || (event.ctrlKey && event.key === '1') || (event.ctrlKey && event.key === '2')) {
      event.preventDefault()
      if (event.ctrlKey && event.key === '1') {
        focusPane('local')
        return
      }

      if (event.ctrlKey && event.key === '2') {
        focusPane('remote')
        return
      }

      focusPane(activePane() === 'local' ? 'remote' : 'local')
      return
    }

    if (event.ctrlKey && event.key.toLowerCase() === 'f') {
      event.preventDefault()
      focusPaneFilter(activePane())
      return
    }

    if (editableTarget) return

    if (event.key === 'F5' || (event.ctrlKey && event.key.toLowerCase() === 'r')) {
      event.preventDefault()
      void refreshPane(activePane())
      return
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveSelection(activePane(), -1)
      return
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault()
      moveSelection(activePane(), 1)
      return
    }

    if (event.key === 'Enter') {
      event.preventDefault()
      void openSelectedEntry(activePane())
      return
    }

    if (event.key === 'F2') {
      event.preventDefault()
      startInlineRename(activePane())
      return
    }

    if (event.key === 'Delete') {
      event.preventDefault()
      openDeleteConfirmation(activePane())
      return
    }

    if (event.key === 'Backspace' || (event.altKey && event.key === 'ArrowUp')) {
      event.preventDefault()
      void goUpInPane(activePane())
    }
  }

  onMount(() => {
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', stopDragging)
    window.addEventListener('keydown', handleShortcut)
  })

  onCleanup(() => {
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', stopDragging)
    window.removeEventListener('keydown', handleShortcut)
  })

  return (
    <div class="relative h-screen overflow-hidden bg-[var(--warp-bg)] text-[var(--warp-text)]">
      <div class="flex h-full min-h-0 flex-col overflow-hidden border-x border-white/10 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.06),transparent_32%),linear-gradient(180deg,rgba(255,255,255,0.02),transparent_28%),var(--warp-bg)]">
        <header class="border-b border-white/10 px-5 py-4 sm:px-7">
          <div class="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
            <div>
              <div class="flex items-center gap-3">
                <div class="rounded-full border border-white/20 px-2 py-1 font-mono text-[11px] uppercase tracking-[0.28em] text-zinc-400">
                  warp
                </div>
                <div class="font-mono text-[11px] uppercase tracking-[0.26em] text-zinc-500">
                  split-pane sftp client
                </div>
              </div>
            </div>

            <div class="grid gap-2 font-mono text-xs text-zinc-300 sm:grid-cols-2 xl:grid-cols-4">
              <div class="rounded-md border border-white/10 bg-black/40 px-3 py-2">
                <div class="text-[10px] uppercase tracking-[0.24em] text-zinc-500">Session</div>
                <div class="mt-1 text-white">{session().connectionState}</div>
              </div>
              <div class="rounded-md border border-white/10 bg-black/40 px-3 py-2">
                <div class="text-[10px] uppercase tracking-[0.24em] text-zinc-500">Protocol</div>
                <div class="mt-1 text-white">{session().protocolMode}</div>
              </div>
              <div class="rounded-md border border-white/10 bg-black/40 px-3 py-2">
                <div class="text-[10px] uppercase tracking-[0.24em] text-zinc-500">Target</div>
                <div class="mt-1 text-white">{session().host}</div>
              </div>
              <div class="rounded-md border border-white/10 bg-black/40 px-3 py-2">
                <div class="text-[10px] uppercase tracking-[0.24em] text-zinc-500">Trust</div>
                <div class="mt-1 text-white">{session().trustState}</div>
              </div>
            </div>
          </div>

          <Show when={connectError()}>
            {(message) => <div class="mt-4 rounded-md border border-red-400/20 bg-red-400/10 px-3 py-2 font-mono text-xs text-red-200">{message()}</div>}
          </Show>

          <div class="mt-4 flex flex-col gap-4 border-t border-white/10 pt-4">
            <div class="rounded-lg border border-white/10 bg-black/35 px-4 py-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
              <div class="grid gap-3 lg:grid-cols-[minmax(220px,1.5fr)_80px_minmax(140px,0.9fr)_110px_auto] lg:items-end">
                <label class="block">
                  <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Host</span>
                  <input value={connectHost()} onInput={(event) => setConnectHost(event.currentTarget.value)} placeholder="example.com" class="w-full rounded-md border border-white/12 bg-black/60 px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/35" />
                </label>
                <label class="block">
                  <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Port</span>
                  <input value={connectPort()} onInput={(event) => setConnectPort(event.currentTarget.value)} inputmode="numeric" placeholder="22" class="w-full rounded-md border border-white/12 bg-black/60 px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/35" />
                </label>
                <label class="block">
                  <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">User</span>
                  <input value={connectUsername()} onInput={(event) => setConnectUsername(event.currentTarget.value)} placeholder="username" class="w-full rounded-md border border-white/12 bg-black/60 px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/35" />
                </label>
                <label class="block">
                  <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Auth</span>
                  <select value={connectAuthMode()} onChange={(event) => setConnectAuthMode(event.currentTarget.value === 'key' ? 'key' : 'password')} class="w-full appearance-none rounded-md border border-white/12 bg-black/60 px-3 py-2 font-mono text-sm text-white outline-none transition focus:border-white/35">
                    <option value="password">Password</option>
                    <option value="key">SSH key</option>
                  </select>
                </label>

                <div class="flex flex-wrap gap-2 lg:justify-end">
                  <button class="warp-button" onClick={() => void refreshPane(activePane())} disabled={localLoading() || remoteLoading()}>
                    Refresh Active Pane
                  </button>
                  <button class="warp-button warp-button-primary" onClick={() => void submitConnect()} disabled={remoteLoading()}>
                    {session().canDisconnect ? 'Reconnect' : 'Connect'}
                  </button>
                  <button class="warp-button" onClick={() => void submitDisconnect()} disabled={!session().canDisconnect || remoteLoading()}>
                    Disconnect
                  </button>
                </div>
              </div>

              <Show
                when={connectAuthMode() === 'password'}
                fallback={
                  <div class="mt-3 grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(220px,320px)] lg:items-end">
                    <label class="block">
                      <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Private Key</span>
                      <input value={connectPrivateKeyPath()} onInput={(event) => setConnectPrivateKeyPath(event.currentTarget.value)} placeholder="/home/user/.ssh/id_ed25519" class="w-full rounded-md border border-white/12 bg-black/60 px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/35" />
                    </label>
                    <label class="block">
                      <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Passphrase</span>
                      <input type="password" value={connectPassphrase()} onInput={(event) => setConnectPassphrase(event.currentTarget.value)} placeholder="optional" class="w-full rounded-md border border-white/12 bg-black/60 px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/35" />
                    </label>
                  </div>
                }
              >
                <div class="mt-3 grid gap-3 lg:grid-cols-[minmax(260px,420px)] lg:items-end">
                  <label class="block">
                    <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Password</span>
                    <input type="password" value={connectPassword()} onInput={(event) => setConnectPassword(event.currentTarget.value)} placeholder="optional" class="w-full rounded-md border border-white/12 bg-black/60 px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/35" />
                  </label>
                </div>
              </Show>
            </div>

            <Show when={trustPrompt()}>
              {(prompt) => (
                <div class="rounded-lg border border-amber-300/20 bg-amber-200/10 px-4 py-3 font-mono text-sm text-amber-50">
                  <div class="text-[10px] uppercase tracking-[0.22em] text-amber-200/70">Trust Required</div>
                  <div class="mt-2">{prompt().message}</div>
                  <div class="mt-3 grid grid-cols-[88px_minmax(0,1fr)] gap-x-3 gap-y-2 text-xs text-amber-100/80">
                    <div class="text-amber-200/70">Host</div>
                    <div>{prompt().host}:{prompt().port}</div>
                    <div class="text-amber-200/70">Fingerprint</div>
                    <div class="break-all">{prompt().fingerprintSha256}</div>
                    <div class="text-amber-200/70">Algorithm</div>
                    <div>{prompt().keyAlgorithm}</div>
                    <Show when={prompt().expectedFingerprintSha256}>
                      {(fingerprint) => (
                        <>
                          <div class="text-amber-200/70">Expected</div>
                          <div class="break-all">{fingerprint()}</div>
                        </>
                      )}
                    </Show>
                  </div>
                  <div class="mt-4 flex flex-wrap gap-2">
                    <button class="warp-button" onClick={() => void handleTrustDecision(false)} disabled={remoteLoading()}>
                      Cancel
                    </button>
                    <button class="warp-button warp-button-primary" onClick={() => void handleTrustDecision(true)} disabled={remoteLoading() || prompt().status !== 'firstSeen'}>
                      Trust And Connect
                    </button>
                  </div>
                </div>
              )}
            </Show>

            <div class="flex flex-wrap gap-2 font-mono text-[11px] uppercase tracking-[0.22em] text-zinc-500">
              <For each={shortcuts()}>
                {(shortcut) => <span class="rounded-full border border-white/10 px-2 py-1">{shortcut}</span>}
              </For>
            </div>
          </div>
        </header>

        <main class="flex min-h-0 flex-1 flex-col overflow-hidden">
          <section id="workspace-shell" class="min-h-0 flex-1 overflow-hidden px-4 py-4 sm:px-6">
            <div class="flex h-full min-h-0 overflow-hidden rounded-xl border border-white/10 bg-black/30 p-3 shadow-[0_30px_120px_rgba(0,0,0,0.45)] backdrop-blur-sm">
              <div class="min-w-0" style={{ width: `${dividerRatio() * 100}%` }}>
                <Pane
                  pane={localPane()}
                  entries={localEntries()}
                  active={activePane() === 'local'}
                  paneClass={paneClass('local')}
                  filterValue={localFilter()}
                  selectedName={localSelection()}
                  loading={localLoading()}
                  errorMessage={localError()}
                  editingName={renamingPane() === 'local' ? renamingEntryName() : null}
                  renameDraft={renameDraft()}
                  creatingDirectory={creatingDirectoryPane() === 'local'}
                  createDirectoryDraft={createDirectoryDraft()}
                  selectedCount={selectedCount('local')}
                  setPaneRef={(element) => {
                    localPaneElement = element
                  }}
                  setFilterRef={(element) => {
                    localFilterInput = element
                  }}
                  setRenameInputRef={(element, entry) => {
                    queueMicrotask(() => {
                      if (renamingEntryName() !== entry.name) return
                      const [start, end] = renameSelectionRange(entry)
                      element.focus()
                      element.setSelectionRange(start, end)
                    })
                  }}
                  onFilter={setLocalFilter}
                  onFocus={() => activatePane('local')}
                  onSelect={(entry) => {
                    setLocalSelection(entry.name)
                    activatePane('local')
                  }}
                  onEntryOpen={(entry) => void openEntry('local', entry)}
                  onRenameStart={() => startInlineRename('local')}
                  onRenameDraft={setRenameDraft}
                  onRenameCommit={() => void commitInlineRename('local')}
                  onRenameCancel={() => cancelInlineRename('local')}
                  onUp={() => void goUpInPane('local')}
                  onRefresh={() => void refreshPane('local')}
                  transferActionLabel="Upload"
                  onTransfer={() => void queueSelectedUpload()}
                  transferDisabled={!uploadCandidate() || localLoading() || remoteLoading()}
                  onDelete={() => openDeleteConfirmation('local')}
                />
              </div>

              <div class="flex w-4 shrink-0 items-center justify-center">
                <button
                  type="button"
                  aria-label="Resize panes"
                  class={`flex h-full w-3 cursor-col-resize items-center justify-center rounded-full transition ${dragging() ? 'bg-white/12' : 'bg-transparent hover:bg-white/6'}`}
                  onPointerDown={(event) => {
                    setDragging(true)
                    resize(event.clientX)
                  }}
                >
                  <span class="h-14 w-px bg-white/30" />
                </button>
              </div>

              <div class="min-w-0 flex-1">
                <Pane
                  pane={remotePane()}
                  entries={remoteEntries()}
                  active={activePane() === 'remote'}
                  paneClass={paneClass('remote')}
                  filterValue={remoteFilter()}
                  selectedName={remoteSelection()}
                  loading={remoteLoading()}
                  errorMessage={remoteRuntimeError()}
                  editingName={renamingPane() === 'remote' ? renamingEntryName() : null}
                  renameDraft={renamingPane() === 'remote' ? renameDraft() : ''}
                  creatingDirectory={creatingDirectoryPane() === 'remote'}
                  createDirectoryDraft={createDirectoryDraft()}
                  selectedCount={selectedCount('remote')}
                  setPaneRef={(element) => {
                    remotePaneElement = element
                  }}
                  setFilterRef={(element) => {
                    remoteFilterInput = element
                  }}
                  setCreateDirectoryInputRef={(element: HTMLInputElement) => {
                    createDirectoryInput = element
                  }}
                  setRenameInputRef={(element, entry) => {
                    queueMicrotask(() => {
                      if (renamingPane() !== 'remote' || renamingEntryName() !== entry.name) return
                      const [start, end] = renameSelectionRange(entry)
                      element.focus()
                      element.setSelectionRange(start, end)
                    })
                  }}
                  onFilter={setRemoteFilter}
                  onFocus={() => activatePane('remote')}
                  onSelect={(entry) => {
                    setRemoteSelection(entry.name)
                    activatePane('remote')
                  }}
                  onEntryOpen={(entry) => void openEntry('remote', entry)}
                  onRenameStart={() => startInlineRename('remote')}
                  onRenameDraft={setRenameDraft}
                  onRenameCommit={() => void commitInlineRename('remote')}
                  onRenameCancel={() => cancelInlineRename('remote')}
                  onCreateDirectoryStart={session().connectionState === 'Connected' ? () => startCreateDirectory('remote') : undefined}
                  onCreateDirectoryDraft={setCreateDirectoryDraft}
                  onCreateDirectoryCommit={() => void commitCreateDirectory('remote')}
                  onCreateDirectoryCancel={() => cancelCreateDirectory('remote')}
                  onUp={() => void goUpInPane('remote')}
                  onRefresh={() => void refreshPane('remote')}
                  transferActionLabel="Download"
                  onTransfer={() => void queueSelectedDownload()}
                  transferDisabled={!downloadCandidate() || remoteLoading() || localLoading()}
                  onDelete={() => openDeleteConfirmation('remote')}
                />
              </div>
            </div>
          </section>

          <section class="shrink-0 border-t border-white/10 bg-black/50 px-4 pb-3 pt-2 sm:px-6">
            <div class="mb-2 flex items-center justify-between gap-3">
              <div class="font-mono text-[11px] uppercase tracking-[0.24em] text-zinc-500">Transfer Queue</div>
              <div class="flex items-center gap-3 font-mono text-xs text-zinc-500">
                <div>{transfers().jobs.length} jobs</div>
                <button
                  class="warp-button px-2 py-1 text-[11px]"
                  disabled={!transfers().jobs.some((job) => ['Succeeded', 'Failed', 'Cancelled'].includes(job.state))}
                  onClick={() => void clearTransferHistory()}
                >
                  Clear
                </button>
              </div>
            </div>

            <Show
              when={transfers().jobs.length > 0}
              fallback={
                <div class="flex h-20 items-center justify-center rounded-lg border border-dashed border-white/10 bg-black/40 px-6 text-center font-mono text-sm text-zinc-500">
                  No transfer activity yet.
                </div>
              }
            >
              <div class="max-h-52 overflow-y-auto rounded-lg border border-white/10 bg-black/70">
                <For each={orderedTransferJobs()}>
                  {(job) => (
                    <div class="border-b border-white/5 px-3 py-2 last:border-b-0">
                      <div class="flex items-center gap-3 text-left">
                        <div class={`font-mono text-sm ${job.direction === 'Upload' ? 'text-zinc-400' : 'text-zinc-300'}`}>
                          {job.direction === 'Upload' ? '↑' : '↓'}
                        </div>
                        <div class="min-w-0 flex-1">
                          <div class="flex items-center gap-3 font-mono text-sm">
                            <div class="truncate text-white">{job.name}</div>
                            <Show when={!job.conflict && job.state !== 'Failed'}>
                              <div class="shrink-0 text-zinc-500">
                                {job.progressPercent === null ? '--' : `${job.progressPercent}%`}
                                <span class="mx-2 text-zinc-700">/</span>
                                {job.rate ?? transferStateLabel(job.state)}
                              </div>
                            </Show>
                          </div>
                          <Show when={job.conflict}>
                            {(conflict) => (
                              <div class="mt-1 flex flex-wrap items-center gap-2 font-mono text-xs text-amber-100">
                                <span>
                                  Conflict: destination exists as {conflict().destinationKind === 'dir' ? 'a directory' : 'a file'}.
                                </span>
                                <Show when={conflict().canOverwrite}>
                                  <button class="warp-button px-2 py-1 text-[11px]" onClick={() => void resolveTransferJobConflict(job.id, 'overwrite')}>
                                    Overwrite
                                  </button>
                                </Show>
                                <button class="warp-button px-2 py-1 text-[11px]" onClick={() => void resolveTransferJobConflict(job.id, 'cancel')}>
                                  Cancel
                                </button>
                              </div>
                            )}
                          </Show>
                          <Show when={!job.conflict && job.state === 'Failed' && job.errorMessage}>
                            {(message) => <div class="mt-1 truncate font-mono text-xs text-red-200">{message()}</div>}
                          </Show>
                          <Show when={!job.conflict && job.state !== 'Failed'}>
                            <div class="mt-1 truncate font-mono text-[11px] text-zinc-600" title={`${job.sourcePath} -> ${job.destinationPath}`}>
                              {job.destinationPath}
                            </div>
                          </Show>
                        </div>
                        <div class={`shrink-0 font-mono text-xs ${transferTone(job.state)}`}>{transferStateLabel(job.state)}</div>
                        <button class="warp-button shrink-0 px-2 py-1 text-[11px]" disabled={!job.canCancel} onClick={() => void cancelTransferJob(job.id)}>
                          Cancel
                        </button>
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </section>
        </main>
      </div>

      <Show when={pendingDeleteTarget()}>
        {(target) => (
          <div class="absolute inset-0 z-30 flex items-center justify-center bg-black/60 px-4" onPointerDown={() => closeDeleteConfirmation()}>
            <div
              class="w-full max-w-md rounded-xl border border-white/10 bg-[var(--warp-surface-elevated)] p-5 shadow-[0_30px_120px_rgba(0,0,0,0.55)]"
              onPointerDown={(event) => event.stopPropagation()}
            >
              <div class="font-mono text-[11px] uppercase tracking-[0.24em] text-zinc-500">Delete Entry</div>
              <div class="mt-3 font-mono text-sm text-zinc-300">
                {target().message ?? 'This will permanently delete:'}
              </div>
              <div class="mt-3 rounded-md border border-red-400/20 bg-red-400/10 px-3 py-3 font-mono text-sm text-white">
                {target().entry.name}
                <div class="mt-1 text-[11px] uppercase tracking-[0.18em] text-red-100/70">
                  {target().entry.kind === 'dir' ? 'Directory' : 'File'}
                </div>
              </div>
              <div class="mt-5 flex justify-end gap-2">
                <button class="warp-button" onClick={() => closeDeleteConfirmation()}>
                  Cancel
                </button>
                <button
                  ref={(element) => {
                    deleteConfirmButton = element
                  }}
                  class="warp-button border-red-300/30 bg-red-300/10 text-red-100 hover:border-red-300/50 hover:bg-red-300/18"
                  onClick={() => void confirmDelete()}
                >
                  {target().recursive ? 'Delete All' : 'Delete'}
                </button>
              </div>
            </div>
          </div>
        )}
      </Show>
    </div>
  )
}

type PaneProps = {
  pane: PaneSnapshot
  entries: FileEntry[]
  active: boolean
  paneClass: string
  filterValue: string
  selectedName: string | null
  loading: boolean
  errorMessage: string | null
  editingName: string | null
  renameDraft: string
  creatingDirectory: boolean
  createDirectoryDraft: string
  selectedCount: number
  setPaneRef: (element: HTMLElement) => void
  setFilterRef: (element: HTMLInputElement) => void
  setRenameInputRef?: (element: HTMLInputElement, entry: FileEntry) => void
  setCreateDirectoryInputRef?: (element: HTMLInputElement) => void
  onFilter: (value: string) => void
  onFocus: () => void
  onSelect: (entry: FileEntry) => void
  onEntryOpen: (entry: FileEntry) => void
  onRenameStart?: () => void
  onRenameDraft?: (value: string) => void
  onRenameCommit?: () => void
  onRenameCancel?: () => void
  onCreateDirectoryStart?: () => void
  onCreateDirectoryDraft?: (value: string) => void
  onCreateDirectoryCommit?: () => void
  onCreateDirectoryCancel?: () => void
  onUp?: () => void
  onRefresh?: () => void
  transferActionLabel?: string
  onTransfer?: () => void
  transferDisabled?: boolean
  onDelete?: () => void
}

function Pane(props: PaneProps) {
  const filteredCount = () => props.entries.length
  const paneStatus = () => {
    if (props.loading) return 'Loading'
    if (props.errorMessage) return 'Error'
    return props.active ? 'Focused' : 'Idle'
  }

  const paneStatusClass = () => {
    if (props.loading) return 'border-white/20 text-white'
    if (props.errorMessage) return 'border-red-300/30 text-red-200'
    return 'border-white/10 text-zinc-400'
  }

  const emptyStateClass = () => {
    if (props.errorMessage && !props.loading) return 'text-red-200'
    if (props.loading) return 'text-white'
    return 'text-zinc-500'
  }

  const emptyStateMessage = () => {
    if (props.loading) return 'Loading directory...'
    if (props.errorMessage && props.pane.entries.length === 0) return props.errorMessage
    if (props.filterValue.trim().length > 0) return 'No entries match the current filter.'
    return props.pane.emptyMessage ?? 'Directory is empty.'
  }

  return (
    <section
      ref={props.setPaneRef}
      class={`flex h-full min-h-0 flex-col rounded-lg border transition ${props.paneClass}`}
      onMouseDown={props.onFocus}
      onFocusIn={props.onFocus}
      tabindex={0}
    >
      <div class="border-b border-white/10 px-4 py-3">
        <div class="flex items-center justify-between gap-3">
          <div>
            <div class="font-mono text-[11px] uppercase tracking-[0.24em] text-zinc-500">{props.pane.title}</div>
            <div class="mt-1 truncate font-mono text-sm text-white">{props.pane.location}</div>
          </div>
          <div class={`rounded-full border px-2 py-1 font-mono text-[10px] uppercase tracking-[0.18em] ${paneStatusClass()}`}>
            {paneStatus()}
          </div>
        </div>

        <div class="mt-3 flex flex-wrap gap-2">
          <button class="warp-button" disabled={!props.onUp || !props.pane.canGoUp || props.loading} onClick={props.onUp}>
            Up
          </button>
          <button class="warp-button" disabled={!props.onRefresh || props.loading} onClick={props.onRefresh}>
            Refresh
          </button>
          <Show when={props.onCreateDirectoryStart}>
            <button class="warp-button" disabled={props.loading} onClick={props.onCreateDirectoryStart}>
              New Folder
            </button>
          </Show>
          <button class="warp-button" disabled={!props.onTransfer || props.transferDisabled} onClick={props.onTransfer}>
            {props.transferActionLabel ?? 'Transfer'}
          </button>
          <button
            class="warp-button"
            disabled={!props.onRenameStart || props.selectedCount === 0 || props.loading}
            onClick={props.onRenameStart}
          >
            Rename
          </button>
          <button class="warp-button" disabled={!props.onDelete || props.selectedCount === 0 || props.loading} onClick={props.onDelete}>
            Delete
          </button>
        </div>

        <Show when={props.creatingDirectory}>
          <div class="mt-3 rounded-md border border-white/10 bg-white/[0.03] px-3 py-2">
            <div class="font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">New Folder</div>
            <input
              ref={props.setCreateDirectoryInputRef}
              value={props.createDirectoryDraft}
              onInput={(event) => props.onCreateDirectoryDraft?.(event.currentTarget.value)}
              onBlur={() => props.onCreateDirectoryCancel?.()}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  void props.onCreateDirectoryCommit?.()
                  return
                }

                if (event.key === 'Escape') {
                  event.preventDefault()
                  props.onCreateDirectoryCancel?.()
                }
              }}
              placeholder="folder name"
              class="mt-2 block w-full rounded-md border border-white/10 bg-black/30 px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/35"
            />
          </div>
        </Show>

        <label class="mt-3 block">
          <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Filter current pane</span>
          <input
            ref={props.setFilterRef}
            value={props.filterValue}
            onInput={(event) => props.onFilter(event.currentTarget.value)}
            placeholder="name, path, permissions"
            class="w-full rounded-md border border-white/10 bg-white/[0.03] px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/40"
          />
        </label>

        <Show when={props.errorMessage !== null}>
          <div class="mt-3 rounded-md border border-red-400/20 bg-red-400/10 px-3 py-2 font-mono text-xs text-red-200">
            {props.errorMessage}
          </div>
        </Show>
      </div>

      <div class="grid grid-cols-[minmax(0,1.8fr)_110px_130px_90px] gap-3 border-b border-white/10 px-4 py-2 font-mono text-[10px] uppercase tracking-[0.22em] text-zinc-500">
        <div>Name</div>
        <div>Size</div>
        <div>Modified</div>
        <div>Perms</div>
      </div>

      <div class="relative min-h-0 flex-1 overflow-auto">
        <Show
          when={props.entries.length > 0}
          fallback={
            <div class={`flex h-full items-center justify-center px-6 text-center font-mono text-sm ${emptyStateClass()}`}>
              {emptyStateMessage()}
            </div>
          }
        >
          <div class="divide-y divide-white/5">
            <For each={props.entries}>
              {(entry) => {
                const isSelected = () => props.selectedName === entry.name
                const isEditing = () => props.editingName === entry.name

                return (
                  <div
                    class={`grid w-full grid-cols-[minmax(0,1.8fr)_110px_130px_90px] gap-3 px-4 py-3 text-left transition ${isSelected() ? 'bg-white/[0.08]' : 'hover:bg-white/[0.03]'}`}
                    onPointerDown={() => props.onSelect(entry)}
                    onDblClick={() => props.onEntryOpen(entry)}
                  >
                    <div class="min-w-0 w-full">
                      <div class="relative min-w-0 w-full">
                        <div class={`truncate font-mono text-sm leading-5 font-normal tracking-normal ${entryTone(entry.kind)} ${isEditing() ? 'invisible' : ''}`}>
                          {entry.name}
                        </div>
                        <Show when={isEditing()}>
                          <div class="pointer-events-none absolute inset-0 rounded-sm ring-1 ring-white/30" />
                          <input
                            ref={(element) => props.setRenameInputRef?.(element, entry)}
                            value={props.renameDraft}
                            onClick={(event) => event.stopPropagation()}
                            onPointerDown={(event) => event.stopPropagation()}
                            onInput={(event) => props.onRenameDraft?.(event.currentTarget.value)}
                            onBlur={() => props.onRenameCancel?.()}
                            onKeyDown={(event) => {
                              if (event.key === 'Enter') {
                                event.preventDefault()
                                void props.onRenameCommit?.()
                                return
                              }

                              if (event.key === 'Escape') {
                                event.preventDefault()
                                props.onRenameCancel?.()
                              }
                            }}
                            class="absolute inset-0 block w-full min-w-0 bg-transparent px-0 py-0 font-mono text-sm font-normal leading-5 tracking-normal text-white outline-none"
                          />
                        </Show>
                      </div>
                      <div class="mt-1 truncate font-mono text-[11px] uppercase tracking-[0.16em] text-zinc-600">
                        {entry.kind}
                      </div>
                    </div>
                    <div class="font-mono text-sm text-zinc-400">{formatSize(entry.sizeBytes)}</div>
                    <div class="font-mono text-sm text-zinc-400">{formatModified(entry.modifiedUnixMs)}</div>
                    <div class="font-mono text-sm text-zinc-300">{entry.permissions}</div>
                  </div>
                )
              }}
            </For>
          </div>
        </Show>

        <Show when={props.loading && props.entries.length > 0}>
          <div class="pointer-events-none absolute inset-0 flex items-center justify-center bg-black/50 px-6 text-center font-mono text-sm text-white backdrop-blur-[1px]">
            Loading directory...
          </div>
        </Show>
      </div>

      <div class="grid grid-cols-3 gap-3 border-t border-white/10 px-4 py-3 font-mono text-xs text-zinc-500">
        <div>{props.pane.itemCount} total</div>
        <div>{filteredCount()} visible</div>
        <div>{props.selectedCount} selected</div>
      </div>
    </section>
  )
}

export default App
