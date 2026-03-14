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
  path: string
  rate: string | null
  progressPercent: number | null
  state: 'Queued' | 'Running' | 'Complete' | 'Failed'
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

export type AppBootstrap = {
  connectionProfiles: ConnectionProfile[]
  session: SessionSnapshot
  panes: {
    local: PaneSnapshot
    remote: PaneSnapshot
  }
  transfers: TransferJob[]
  shortcuts: string[]
}
