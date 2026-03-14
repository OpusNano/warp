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
  protocol: 'SFTP' | 'SCP compatibility'
  direction: 'Upload' | 'Download'
  name: string
  sourcePath: string
  destinationPath: string
  rate: string | null
  bytesTotal: number | null
  bytesTransferred: number
  progressPercent: number | null
  state: 'Queued' | 'Checking' | 'AwaitingConflictDecision' | 'Running' | 'Cancelling' | 'Cancelled' | 'Succeeded' | 'Failed'
  errorMessage: string | null
  conflict: TransferConflict | null
  canCancel: boolean
}

export type TransferConflict = {
  destinationExists: boolean
  destinationKind: 'file' | 'dir' | 'symlink' | 'unknown'
  canOverwrite: boolean
}

export type TransferQueueSnapshot = {
  jobs: TransferJob[]
  activeJobId: string | null
  queuedCount: number
  finishedCount: number
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
  remotePath: string
  remoteName: string
  localDirectory: string
}

export type QueueUploadRequest = {
  localPath: string
  localName: string
  remoteDirectory: string
}

export type CreateRemoteDirectoryRequest = {
  parentPath: string
  name: string
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

export type RemoteDeletePrompt = {
  path: string
  name: string
  entryKind: FileEntry['kind']
  message: string
  requiresRecursive: boolean
}

export type RemoteDeleteResponse = {
  snapshot: RemoteConnectionSnapshot
  prompt: RemoteDeletePrompt | null
}

export type TransferConflictResolution = {
  action: 'overwrite' | 'cancel'
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
