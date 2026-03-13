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
}

export type ConnectionProfile = {
  name: string
  target: string
  auth: string
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
