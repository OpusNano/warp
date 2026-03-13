import type { AppBootstrap } from './types'

export const mockBootstrap: AppBootstrap = {
  connectionProfiles: [
    { name: 'prod-edge', target: 'deploy@edge-01.example.com:22', auth: 'ed25519' },
    { name: 'media-origin', target: 'ops@origin.internal:22', auth: 'password' },
  ],
  session: {
    connectionState: 'Connected',
    protocolMode: 'SFTP primary',
    host: 'deploy@edge-01.example.com',
    authMethod: 'SSH key',
    trustState: 'Known host verified',
  },
  panes: {
    local: {
      id: 'local',
      title: 'Local',
      location: '/home/cyberdyne/projects/warp',
      itemCount: 8,
      canGoUp: true,
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
      location: '/srv/www/releases/current',
      itemCount: 9,
      canGoUp: true,
      entries: [
        { path: '/srv/www/releases/current/assets', name: 'assets', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000000000, permissions: 'drwxr-xr-x' },
        { path: '/srv/www/releases/current/config', name: 'config', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000100000, permissions: 'drwxr-x---' },
        { path: '/srv/www/releases/current/public', name: 'public', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000200000, permissions: 'drwxr-xr-x' },
        { path: '/srv/www/releases/current/storage', name: 'storage', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000300000, permissions: 'drwxrwx---' },
        { path: '/srv/www/releases/current/vendor', name: 'vendor', kind: 'dir', sizeBytes: null, modifiedUnixMs: 1760000400000, permissions: 'drwxr-xr-x' },
        { path: '/srv/www/releases/current/.env.production', name: '.env.production', kind: 'file', sizeBytes: 1434, modifiedUnixMs: 1760000500000, permissions: '-rw-------' },
        { path: '/srv/www/releases/current/index.php', name: 'index.php', kind: 'file', sizeBytes: 1843, modifiedUnixMs: 1760000600000, permissions: '-rw-r--r--' },
        { path: '/srv/www/releases/current/release.json', name: 'release.json', kind: 'file', sizeBytes: 732, modifiedUnixMs: 1760000700000, permissions: '-rw-r--r--' },
        { path: '/srv/www/releases/current/var', name: 'var', kind: 'symlink', sizeBytes: null, modifiedUnixMs: 1760000800000, permissions: 'lrwxrwxrwx' },
      ],
    },
  },
  transfers: [],
  shortcuts: ['Tab pane', 'Ctrl+1 local', 'Ctrl+2 remote', 'Ctrl+F filter', 'F5 refresh'],
}
