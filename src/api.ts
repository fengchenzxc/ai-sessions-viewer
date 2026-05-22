import { invoke } from '@tauri-apps/api/core'
import type { Agent, ProjectInfo, SessionPage, Msg, TrashItem } from './types'

export const listProjects = (agent: Agent) =>
  invoke<ProjectInfo[]>('list_projects', { agent })

export const listSessions = (
  agent: Agent,
  projectKey: string,
  offset: number,
  limit: number,
) => invoke<SessionPage>('list_sessions', { agent, projectKey, offset, limit })

export const readSession = (agent: Agent, path: string) =>
  invoke<Msg[]>('read_session', { agent, path })

export const renameSession = (agent: Agent, path: string, name: string) =>
  invoke<void>('rename_session', { agent, path, name })

export const softDeleteSession = (
  agent: Agent,
  path: string,
  projectLabel: string,
) => invoke<void>('soft_delete_session', { agent, path, projectLabel })

export const listTrash = () => invoke<TrashItem[]>('list_trash')

export const restoreSession = (trashFile: string) =>
  invoke<void>('restore_session', { trashFile })

export const permanentDeleteTrash = (trashFile: string) =>
  invoke<void>('permanent_delete_trash', { trashFile })

export const emptyTrash = () => invoke<void>('empty_trash')

export const revealInFinder = (path: string) =>
  invoke<void>('reveal_in_finder', { path })

/** 在系统默认浏览器中打开一个外部链接（仅 http/https）。 */
export const openUrl = (url: string) => invoke<void>('open_url', { url })

/** 写入用户指定的绝对路径（覆盖同名）。返回最终路径以便后续 reveal。 */
export const writeFile = (path: string, content: string) =>
  invoke<string>('write_file', { path, content })

export const resumeSession = (agent: Agent, sessionId: string, cwd: string) =>
  invoke<void>('resume_session', { agent, sessionId, cwd })

/** 在终端里为某个项目目录开一个全新会话（不带 --resume）。 */
export const newSession = (agent: Agent, cwd: string) =>
  invoke<void>('new_session', { agent, cwd })

export interface UpdateInfo {
  current: string
  latest: string
  hasUpdate: boolean
}
export const appVersion = () => invoke<string>('app_version')
export const checkUpdate = () => invoke<UpdateInfo>('check_update')
