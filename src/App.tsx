import { For, Show, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import { bootstrapAppState } from './lib/tauri'
import type { AppBootstrap, FileEntry, PaneId, TransferJob } from './lib/types'

const defaultState: AppBootstrap = {
  connectionProfiles: [],
  session: {
    connectionState: 'Disconnected',
    protocolMode: 'SFTP primary',
    host: 'No active session',
    authMethod: 'SSH key',
    trustState: 'No host selected',
  },
  panes: {
    local: {
      id: 'local',
      title: 'Local',
      location: '/home/cyberdyne/projects',
      filter: '',
      itemCount: 0,
      selectedCount: 0,
      entries: [],
    },
    remote: {
      id: 'remote',
      title: 'Remote',
      location: '/srv',
      filter: '',
      itemCount: 0,
      selectedCount: 0,
      entries: [],
    },
  },
  transfers: [],
  shortcuts: [],
}

function formatSize(size: string) {
  return size === '' ? '--' : size
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
    case 'Running':
      return 'text-white'
    case 'Complete':
      return 'text-emerald-300'
    case 'Failed':
      return 'text-red-300'
    default:
      return 'text-zinc-400'
  }
}

function App() {
  const [appState, setAppState] = createSignal<AppBootstrap>(defaultState)
  const [activePane, setActivePane] = createSignal<PaneId>('remote')
  const [dividerRatio, setDividerRatio] = createSignal(0.5)
  const [dragging, setDragging] = createSignal(false)
  const [localFilter, setLocalFilter] = createSignal('')
  const [remoteFilter, setRemoteFilter] = createSignal('')

  onMount(async () => {
    const state = await bootstrapAppState()
    setAppState(state)
    setLocalFilter(state.panes.local.filter)
    setRemoteFilter(state.panes.remote.filter)
  })

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

  onMount(() => {
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', stopDragging)
  })

  onCleanup(() => {
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', stopDragging)
  })

  const filteredEntries = (paneId: PaneId) =>
    createMemo(() => {
      const pane = appState().panes[paneId]
      const filter = (paneId === 'local' ? localFilter() : remoteFilter()).trim().toLowerCase()

      if (filter.length === 0) return pane.entries

      return pane.entries.filter((entry) => {
        const haystack = `${entry.name} ${entry.permissions} ${entry.modified}`.toLowerCase()
        return haystack.includes(filter)
      })
    })

  const localEntries = filteredEntries('local')
  const remoteEntries = filteredEntries('remote')

  const paneClass = (paneId: PaneId) =>
    activePane() === paneId
      ? 'border-white/70 bg-white/[0.03] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.12)]'
      : 'border-white/10 bg-white/[0.015]'

  return (
    <div class="min-h-screen bg-[var(--warp-bg)] text-[var(--warp-text)]">
      <div class="flex min-h-screen flex-col border-x border-white/10 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.06),transparent_32%),linear-gradient(180deg,rgba(255,255,255,0.02),transparent_28%),var(--warp-bg)]">
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
                <div class="mt-1 text-white">{appState().session.connectionState}</div>
              </div>
              <div class="rounded-md border border-white/10 bg-black/40 px-3 py-2">
                <div class="text-[10px] uppercase tracking-[0.24em] text-zinc-500">Protocol</div>
                <div class="mt-1 text-white">{appState().session.protocolMode}</div>
              </div>
              <div class="rounded-md border border-white/10 bg-black/40 px-3 py-2">
                <div class="text-[10px] uppercase tracking-[0.24em] text-zinc-500">Target</div>
                <div class="mt-1 text-white">{appState().session.host}</div>
              </div>
              <div class="rounded-md border border-white/10 bg-black/40 px-3 py-2">
                <div class="text-[10px] uppercase tracking-[0.24em] text-zinc-500">Trust</div>
                <div class="mt-1 text-white">{appState().session.trustState}</div>
              </div>
            </div>
          </div>

          <div class="mt-4 flex flex-col gap-3 border-t border-white/10 pt-4 lg:flex-row lg:items-center lg:justify-between">
            <div class="flex flex-wrap gap-2">
              <button class="warp-button warp-button-primary">Quick Connect</button>
              <button class="warp-button">Saved Connections</button>
              <button class="warp-button">Refresh Active Pane</button>
              <button class="warp-button">New Directory</button>
            </div>
            <div class="flex flex-wrap gap-2 font-mono text-[11px] uppercase tracking-[0.22em] text-zinc-500">
              <For each={appState().shortcuts}>
                {(shortcut) => <span class="rounded-full border border-white/10 px-2 py-1">{shortcut}</span>}
              </For>
            </div>
          </div>
        </header>

        <main class="flex min-h-0 flex-1 flex-col">
          <section id="workspace-shell" class="min-h-0 flex-1 px-4 py-4 sm:px-6">
            <div class="flex h-full min-h-[560px] rounded-xl border border-white/10 bg-black/30 p-3 shadow-[0_30px_120px_rgba(0,0,0,0.45)] backdrop-blur-sm">
              <div class="min-w-0" style={{ width: `${dividerRatio() * 100}%` }}>
                <Pane
                  pane={appState().panes.local}
                  entries={localEntries()}
                  active={activePane() === 'local'}
                  paneClass={paneClass('local')}
                  filterValue={localFilter()}
                  onFilter={setLocalFilter}
                  onFocus={() => setActivePane('local')}
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
                  pane={appState().panes.remote}
                  entries={remoteEntries()}
                  active={activePane() === 'remote'}
                  paneClass={paneClass('remote')}
                  filterValue={remoteFilter()}
                  onFilter={setRemoteFilter}
                  onFocus={() => setActivePane('remote')}
                />
              </div>
            </div>
          </section>

          <section class="border-t border-white/10 bg-black/50 px-4 pb-4 pt-3 sm:px-6">
            <div class="mb-3 flex items-center justify-between">
              <div>
                <div class="font-mono text-[11px] uppercase tracking-[0.24em] text-zinc-500">Transfer Queue</div>
              </div>
              <div class="font-mono text-xs text-zinc-500">{appState().transfers.length} jobs</div>
            </div>

            <div class="grid gap-px overflow-hidden rounded-lg border border-white/10 bg-white/10">
              <For each={appState().transfers}>
                {(job) => (
                  <div class="grid gap-3 bg-black px-4 py-3 lg:grid-cols-[1.2fr_2fr_110px_100px_90px] lg:items-center">
                    <div>
                      <div class="font-mono text-xs uppercase tracking-[0.2em] text-zinc-500">{job.direction}</div>
                      <div class={`mt-1 font-mono text-sm ${transferTone(job.state)}`}>{job.protocol}</div>
                    </div>
                    <div class="min-w-0">
                      <div class="truncate font-mono text-sm text-white">{job.name}</div>
                      <div class="truncate font-mono text-xs text-zinc-500">{job.path}</div>
                    </div>
                    <div class="font-mono text-sm text-zinc-300">{job.rate}</div>
                    <div class="font-mono text-sm text-zinc-300">{job.progress}</div>
                    <div class={`font-mono text-sm ${transferTone(job.state)}`}>{job.state}</div>
                  </div>
                )}
              </For>
            </div>
          </section>
        </main>
      </div>
    </div>
  )
}

type PaneProps = {
  pane: AppBootstrap['panes']['local']
  entries: FileEntry[]
  active: boolean
  paneClass: string
  filterValue: string
  onFilter: (value: string) => void
  onFocus: () => void
}

function Pane(props: PaneProps) {
  const filteredCount = () => props.entries.length

  return (
    <section
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
          <div class="rounded-full border border-white/10 px-2 py-1 font-mono text-[10px] uppercase tracking-[0.18em] text-zinc-400">
            {props.active ? 'Focused' : 'Idle'}
          </div>
        </div>

        <div class="mt-3 flex flex-wrap gap-2">
          <button class="warp-button">Up</button>
          <button class="warp-button">Refresh</button>
          <button class="warp-button">Rename</button>
          <button class="warp-button">Delete</button>
        </div>

        <label class="mt-3 block">
          <span class="mb-2 block font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-500">Filter current pane</span>
          <input
            value={props.filterValue}
            onInput={(event) => props.onFilter(event.currentTarget.value)}
            placeholder="name, permissions, modified"
            class="w-full rounded-md border border-white/10 bg-white/[0.03] px-3 py-2 font-mono text-sm text-white outline-none transition placeholder:text-zinc-600 focus:border-white/40"
          />
        </label>
      </div>

      <div class="grid grid-cols-[minmax(0,1.8fr)_110px_130px_90px] gap-3 border-b border-white/10 px-4 py-2 font-mono text-[10px] uppercase tracking-[0.22em] text-zinc-500">
        <div>Name</div>
        <div>Size</div>
        <div>Modified</div>
        <div>Perms</div>
      </div>

      <div class="min-h-0 flex-1 overflow-auto">
        <Show
          when={props.entries.length > 0}
          fallback={
            <div class="flex h-full items-center justify-center px-6 text-center font-mono text-sm text-zinc-500">
              No entries match the current filter.
            </div>
          }
        >
          <div class="divide-y divide-white/5">
            <For each={props.entries}>
              {(entry) => (
                <div class="grid grid-cols-[minmax(0,1.8fr)_110px_130px_90px] gap-3 px-4 py-3 transition hover:bg-white/[0.03]">
                  <div class="min-w-0">
                    <div class={`truncate font-mono text-sm ${entryTone(entry.kind)}`}>{entry.name}</div>
                    <div class="mt-1 truncate font-mono text-[11px] uppercase tracking-[0.16em] text-zinc-600">
                      {entry.kind}
                    </div>
                  </div>
                  <div class="font-mono text-sm text-zinc-400">{formatSize(entry.size)}</div>
                  <div class="font-mono text-sm text-zinc-400">{entry.modified}</div>
                  <div class="font-mono text-sm text-zinc-300">{entry.permissions}</div>
                </div>
              )}
            </For>
          </div>
        </Show>
      </div>

      <div class="grid grid-cols-3 gap-3 border-t border-white/10 px-4 py-3 font-mono text-xs text-zinc-500">
        <div>{props.pane.itemCount} total</div>
        <div>{filteredCount()} visible</div>
        <div>{props.pane.selectedCount} selected</div>
      </div>
    </section>
  )
}

export default App
