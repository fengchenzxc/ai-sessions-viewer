import { beforeEach, describe, expect, it, vi } from 'vitest'

// api.ts is a thin typed wrapper over Tauri's `invoke`; we assert each helper
// maps to the right command name and argument shape.
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import * as api from '../src/api'

beforeEach(() => {
  invoke.mockReset()
  invoke.mockResolvedValue(undefined)
})

describe('api wrappers', () => {
  it('listProjects → list_projects', () => {
    api.listProjects('claude')
    expect(invoke).toHaveBeenCalledWith('list_projects', { agent: 'claude' })
  })

  it('listSessions → list_sessions with pagination', () => {
    api.listSessions('codex', 'proj-key', 10, 20)
    expect(invoke).toHaveBeenCalledWith('list_sessions', {
      agent: 'codex',
      projectKey: 'proj-key',
      offset: 10,
      limit: 20,
    })
  })

  it('readSession → read_session', () => {
    api.readSession('claude', '/p/s.jsonl')
    expect(invoke).toHaveBeenCalledWith('read_session', {
      agent: 'claude',
      path: '/p/s.jsonl',
    })
  })

  it('renameSession → rename_session', () => {
    api.renameSession('claude', '/p/s.jsonl', 'New name')
    expect(invoke).toHaveBeenCalledWith('rename_session', {
      agent: 'claude',
      path: '/p/s.jsonl',
      name: 'New name',
    })
  })

  it('softDeleteSession → soft_delete_session', () => {
    api.softDeleteSession('codex', '/p/s.jsonl', 'My Project')
    expect(invoke).toHaveBeenCalledWith('soft_delete_session', {
      agent: 'codex',
      path: '/p/s.jsonl',
      projectLabel: 'My Project',
    })
  })

  it('listTrash → list_trash with no args', () => {
    api.listTrash()
    expect(invoke).toHaveBeenCalledWith('list_trash')
  })

  it('restoreSession → restore_session', () => {
    api.restoreSession('trash-1.jsonl')
    expect(invoke).toHaveBeenCalledWith('restore_session', {
      trashFile: 'trash-1.jsonl',
    })
  })

  it('permanentDeleteTrash → permanent_delete_trash', () => {
    api.permanentDeleteTrash('trash-1.jsonl')
    expect(invoke).toHaveBeenCalledWith('permanent_delete_trash', {
      trashFile: 'trash-1.jsonl',
    })
  })

  it('emptyTrash → empty_trash', () => {
    api.emptyTrash()
    expect(invoke).toHaveBeenCalledWith('empty_trash')
  })

  it('revealInFinder → reveal_in_finder', () => {
    api.revealInFinder('/some/path')
    expect(invoke).toHaveBeenCalledWith('reveal_in_finder', { path: '/some/path' })
  })

  it('writeFile → write_file', () => {
    api.writeFile('/out.md', '# content')
    expect(invoke).toHaveBeenCalledWith('write_file', {
      path: '/out.md',
      content: '# content',
    })
  })

  it('resumeSession → resume_session', () => {
    api.resumeSession('claude', 'abc-123', '/work/dir')
    expect(invoke).toHaveBeenCalledWith('resume_session', {
      agent: 'claude',
      sessionId: 'abc-123',
      cwd: '/work/dir',
    })
  })

  it('appVersion → app_version', () => {
    api.appVersion()
    expect(invoke).toHaveBeenCalledWith('app_version')
  })

  it('checkUpdate → check_update', () => {
    api.checkUpdate()
    expect(invoke).toHaveBeenCalledWith('check_update')
  })

  it('passes the invoke result back to the caller', async () => {
    invoke.mockResolvedValue('1.2.3')
    await expect(api.appVersion()).resolves.toBe('1.2.3')
  })
})
