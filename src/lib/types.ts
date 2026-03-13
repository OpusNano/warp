export type PaneId = 'local' | 'remote'

export type FileEntry = {
  name: string
  kind: 'dir' | 'file' | 'symlink'
  size: string
  modified: string
  permissions: string
}

export type PaneSnapshot = {
  id: PaneId
  title: string
  location: string
  filter: string
  itemCount: number
  selectedCount: number
  entries: FileEntry[]
}

export type TransferJob = {
  id: string
  protocol: 'SFTP' | 'SCP compatibility'
  direction: 'Upload' | 'Download'
  name: string
  path: string
  rate: string
  progress: string
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
