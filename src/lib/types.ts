export type PaneId = 'local' | 'remote'

export type FileEntry = {
  path: string
  name: string
  kind: 'dir' | 'file' | 'symlink'
  sizeBytes: number | null
  modifiedUnixMs: number | null
  permissions: string
}

export type PaneSnapshot = {
  id: PaneId
  title: string
  location: string
  itemCount: number
  canGoUp: boolean
  entries: FileEntry[]
  emptyMessage: string | null
}

export type TransferJob = {
  id: string
  kind: 'batch' | 'child'
  batchId: string | null
  parentId: string | null
  protocol: 'SFTP' | 'SCP compatibility'
  direction: 'Upload' | 'Download'
  name: string
  sourcePath: string
  destinationPath: string
  rate: string | null
  bytesTotal: number | null
  bytesTransferred: number
  progressPercent: number | null
  state:
    | 'Queued'
    | 'Checking'
    | 'AwaitingConflictDecision'
    | 'Running'
    | 'Cancelling'
    | 'Cancelled'
    | 'Succeeded'
    | 'Failed'
    | 'Skipped'
    | 'CompletedWithErrors'
    | 'PausedDisconnected'
  errorMessage: string | null
  conflict: TransferConflict | null
  canCancel: boolean
  canRetry: boolean
  summary: TransferJobSummary | null
  currentItemLabel: string | null
}

export type TransferConflict = {
  destinationExists: boolean
  destinationKind: 'file' | 'dir' | 'symlink' | 'unknown'
  sourceKind: 'file' | 'dir' | 'symlink' | 'unknown'
  sourceName: string
  sourcePath: string
  destinationName: string
  destinationPath: string
  conflictKind: 'fileExists' | 'dirExists' | 'typeMismatch' | 'unknown'
  canOverwrite: boolean
  applyToRemaining: boolean
}

export type TransferJobSummary = {
  totalFiles: number
  totalDirectories: number
  completedFiles: number
  failedFiles: number
  skippedFiles: number
}

export type TransferQueueSnapshot = {
  sequence: number
  jobs: TransferJob[]
  activeJobId: string | null
  queuedCount: number
  finishedCount: number
  batchCount: number
}

export type SessionSnapshot = {
  connectionState: string
  protocolMode: string
  host: string
  authMethod: string
  trustState: string
  lastError: string | null
  canDisconnect: boolean
}

export type ConnectionProfile = {
  name: string
  target: string
  auth: string
}

export type TrustPrompt = {
  host: string
  port: number
  keyAlgorithm: string
  fingerprintSha256: string
  status: 'firstSeen' | 'mismatch'
  message: string
  expectedFingerprintSha256: string | null
}

export type RemoteConnectionSnapshot = {
  session: SessionSnapshot
  remotePane: PaneSnapshot
  trustPrompt: TrustPrompt | null
}

export type ConnectAuth =
  | { kind: 'password'; password: string }
  | { kind: 'key'; privateKeyPath: string; passphrase: string | null }

export type ConnectRequest = {
  host: string
  port: number
  username: string
  auth: ConnectAuth
}

export type TrustDecision = {
  trust: boolean
}

export type QueueDownloadRequest = {
  entries: TransferSelectionItem[]
  localDirectory: string
}

export type QueueUploadRequest = {
  entries: TransferSelectionItem[]
  remoteDirectory: string
}

export type TransferSelectionItem = {
  path: string
  name: string
  kind: FileEntry['kind']
}

export type CreateRemoteDirectoryRequest = {
  parentPath: string
  name: string
}

export type DeleteLocalEntriesRequest = {
  path: string
  entryNames: string[]
}

export type RenameRemoteEntryRequest = {
  parentPath: string
  entryName: string
  newName: string
}

export type DeleteRemoteEntryRequest = {
  parentPath: string
  entryName: string
  entryKind: FileEntry['kind']
  recursive: boolean
}

export type DeleteRemoteEntryTarget = {
  entryName: string
  entryKind: FileEntry['kind']
}

export type DeleteRemoteEntriesRequest = {
  parentPath: string
  entries: DeleteRemoteEntryTarget[]
  recursive: boolean
}

export type RemoteDeletePrompt = {
  message: string
  requiresRecursive: boolean
  entries: DeleteRemoteEntryTarget[]
}

export type RemoteDeleteResponse = {
  snapshot: RemoteConnectionSnapshot
  prompt: RemoteDeletePrompt | null
}

export type TransferConflictResolution = {
  action: 'overwrite' | 'skip' | 'overwriteAll' | 'skipAll' | 'cancelBatch'
}

export type AppBootstrap = {
  connectionProfiles: ConnectionProfile[]
  session: SessionSnapshot
  panes: {
    local: PaneSnapshot
    remote: PaneSnapshot
  }
  transfers: TransferQueueSnapshot
  shortcuts: string[]
}
