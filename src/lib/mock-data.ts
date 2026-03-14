import type { AppBootstrap } from './types'

export const mockBootstrap: AppBootstrap = {
  connectionProfiles: [
    { name: 'prod-edge', target: 'deploy@edge-01.example.com:22', auth: 'ed25519' },
    { name: 'media-origin', target: 'ops@origin.internal:22', auth: 'password' },
  ],
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
      location: '/home/cyberdyne/projects/warp',
      itemCount: 8,
      canGoUp: true,
      emptyMessage: 'Local directory is empty.',
      entries: [
        { path: '/home/cyberdyne/projects/warp/.github', name: '.github', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000000000, permissions: 'drwxr-xr-x' },
        { path: '/home/cyberdyne/projects/warp/src', name: 'src', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000100000, permissions: 'drwxr-xr-x' },
        { path: '/home/cyberdyne/projects/warp/src-tauri', name: 'src-tauri', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000200000, permissions: 'drwxr-xr-x' },
        { path: '/home/cyberdyne/projects/warp/package-lock.json', name: 'package-lock.json', kind: 'file', sizeBytes: 43008, modifiedUnixMs: 1760000300000, permissions: '-rw-r--r--' },
        { path: '/home/cyberdyne/projects/warp/package.json', name: 'package.json', kind: 'file', sizeBytes: 841, modifiedUnixMs: 1760000400000, permissions: '-rw-r--r--' },
        { path: '/home/cyberdyne/projects/warp/README.md', name: 'README.md', kind: 'file', sizeBytes: 3174, modifiedUnixMs: 1760000500000, permissions: '-rw-r--r--' },
        { path: '/home/cyberdyne/projects/warp/tsconfig.app.json', name: 'tsconfig.app.json', kind: 'file', sizeBytes: 689, modifiedUnixMs: 1760000600000, permissions: '-rw-r--r--' },
        { path: '/home/cyberdyne/projects/warp/vite.config.ts', name: 'vite.config.ts', kind: 'file', sizeBytes: 196, modifiedUnixMs: 1760000700000, permissions: '-rw-r--r--' },
      ],
    },
    remote: {
      id: 'remote',
      title: 'Remote',
      location: 'Not connected',
      itemCount: 0,
      canGoUp: false,
      emptyMessage: 'Connect to a host to browse remote files.',
      entries: [],
    },
  },
  transfers: {
    jobs: [],
    activeJobId: null,
    queuedCount: 0,
    finishedCount: 0,
  },
  shortcuts: ['Tab pane', 'Ctrl+1 local', 'Ctrl+2 remote', 'Ctrl+F filter', 'F5 refresh'],
}
