import type { ModPayload } from './report'

export type Endpoint = { port: number; token: string }
/** The mod's two switches, set in the app's Settings → Apps and carried on the board. */
export type Prefs = { pet: boolean; nudges: boolean }
export type Board = { v: 1; app_version: string; prefs?: Prefs; sessions: BoardSession[]; limits: BoardLimit[] }
export type BoardSession = { id: string; agent: string; state: string; title: string; question?: string | null; cwd: string; since: number }
export type BoardLimit = { agent: string; window: 'five_hour' | 'weekly' | 'spend' | 'monthly'; used_pct: number; resets_at?: number | null; stale_since?: number | null }

/**
 * What the bridge needs from the engine. The plugin validator follows `$` only inside the file that
 * hooks, never across an import, so the hook file builds this from its `$` (see `ioOf` in report.ts).
 */
export type BridgeIo = {
  /** This copy's folder (`$.plugin.root`). */
  pluginRoot: string
  /** The user's home directory (USERPROFILE, else HOME); undefined when neither is set. */
  home(): Promise<string | undefined>
  read(path: string): Promise<string>
  exists(path: string): Promise<boolean>
  /** Milliseconds since the epoch. */
  now(): Promise<number>
  fetch(url: string, init: { method: string; headers: Record<string, string>; body?: string }): Promise<BridgeResponse>
}

const BACKOFF_MS = 30_000
const DUPLICATE_TTL_MS = 60_000
const PREFS_TTL_MS = 30_000
const EVENTS_PATH = '/v1/events/claude-mod'
const STATE_PATH = '/v1/state'
const ACTIONS_PATH = '/v1/actions/claude-mod'
const SKILLS_COPY = '/.claude/skills/agent-pets'

/** A `/pets` command for the widget (`crates/pets-core/src/mod_action.rs`). */
export type Action =
  | { action: 'jump' | 'panel'; session_id: string }
  | { action: 'mute'; choice: 'off' | 'hour' | 'morning' | 'forever' }
  | { action: 'rename'; session_id: string; name: string | null }
  | { action: 'pin'; session_id: string; pinned: boolean }
  | { action: 'clear' }
  | { action: 'prefs'; pet?: boolean; nudges?: boolean }

/**
 * The widget's answer. Besides its own errors (`not_found`, `bad_choice`, `save_failed`) the bridge adds `unsupported`
 * (an app too old for commands), `busy` (it did not answer in time) and `rejected` (it did not understand the command).
 */
export type ActionReply = { ok: boolean; title?: string; agent?: string; until?: number; count?: number; error?: string }

/** Forward slashes, no trailing slash. Compare with `.toLowerCase()` where case does not matter. */
const normalise = (path: string) => path.replace(/\\/g, '/').replace(/\/+$/, '')

type BridgeResponse = { status: number; ok: boolean; text: string }

/** What counts when the board has no say: no widget, an older app without `prefs`, or a board that cannot be read. */
export const DEFAULT_PREFS: Prefs = { pet: false, nudges: true }

// A switch that is not a boolean falls back to its default, so one bad value cannot flip the other.
const prefsOf = (board: Board): Prefs => {
  const p: unknown = board.prefs
  const raw = typeof p === 'object' && p !== null ? (p as Partial<Record<keyof Prefs, unknown>>) : {}
  return {
    pet: typeof raw.pet === 'boolean' ? raw.pet : DEFAULT_PREFS.pet,
    nudges: typeof raw.nudges === 'boolean' ? raw.nudges : DEFAULT_PREFS.nudges,
  }
}

/**
 * The only door to the widget: its endpoint file, a bearer token and three failure rules.
 * Nothing here throws to a caller; the token never leaves the Authorization header.
 */
export function createBridge(io: BridgeIo) {
  let endpoint: Endpoint | null = null
  let backoffUntil = 0
  const stoppedSessions = new Set<string>()
  let duplicate: { value: boolean; at: number } | undefined
  let cached: { value: Prefs; at: number } | undefined

  const home = async () => normalise((await io.home()) ?? '')

  // Another copy of this plugin (the app's own, in the skills folder) already reports and draws: this one stays silent.
  // The answer is kept for a minute, so a running session follows the app's Settings toggle without a restart. Never throws.
  const isDuplicateCopy = async (): Promise<boolean> => {
    try {
      const now = await io.now()
      if (duplicate && now - duplicate.at >= 0 && now - duplicate.at < DUPLICATE_TTL_MS) return duplicate.value
      const value = await (async () => {
        const base = await home()
        if (!base) return false
        const own = `${base}${SKILLS_COPY}`
        if (normalise(io.pluginRoot).toLowerCase() === own.toLowerCase()) return false
        return await io.exists(`${own}/.claude-plugin/plugin.json`)
      })()
      duplicate = { value, at: now }
      return value
    } catch {
      return false
    }
  }

  const readEndpoint = async (): Promise<Endpoint> => {
    const text = await io.read(`${await home()}/.agent-pets/endpoint.json`)
    const parsed: unknown = JSON.parse(text)
    const { port, token } = (parsed ?? {}) as Partial<Endpoint>
    if (typeof port !== 'number' || !Number.isInteger(port) || port < 1 || port > 65535 || typeof token !== 'string' || token === '') {
      throw new Error('bad endpoint file')
    }
    return { port, token }
  }

  // A 2xx or a 400 is an answer; anything else (or no answer) drops the endpoint and pauses every call.
  // `answers`: more statuses that are an answer for this call, not a sign the widget is gone.
  const request = async (path: string, init: { method: string; body?: string }, answers: number[] = []): Promise<BridgeResponse | null> => {
    if (await isDuplicateCopy()) return null
    const now = await io.now()
    if (now < backoffUntil) return null
    try {
      endpoint ??= await readEndpoint()
      const headers: Record<string, string> = { authorization: `Bearer ${endpoint.token}` }
      if (init.body !== undefined) headers['content-type'] = 'application/json'
      const res = await io.fetch(`http://127.0.0.1:${endpoint.port}${path}`, { method: init.method, headers, body: init.body })
      if (res.ok || res.status === 400 || answers.includes(res.status)) return res
    } catch {
      // fall through to the backoff
    }
    endpoint = null
    backoffUntil = now + BACKOFF_MS
    return null
  }

  // The widget's board; null when it is unreachable or the bridge is backing off. A board refreshes the cached switches.
  const state = async (): Promise<Board | null> => {
    try {
      const res = await request(STATE_PATH, { method: 'GET' })
      if (!res?.ok) return null
      const board = JSON.parse(res.text) as Board
      if (!board || board.v !== 1) return null
      cached = { value: prefsOf(board), at: await io.now() }
      return board
    } catch {
      return null
    }
  }

  return {
    /** True when this is not the app's copy and the app's copy is installed: draw and report nothing. Cached for 60 s; never throws. */
    isDuplicateCopy,

    /** Fire-and-forget; never throws. */
    send(payload: ModPayload): void {
      if (stoppedSessions.has(payload.session_id)) return
      request(EVENTS_PATH, { method: 'POST', body: JSON.stringify(payload) })
        .then(res => {
          if (res?.status === 400) stoppedSessions.add(payload.session_id)
        })
        .catch(() => {})
    },

    /** The widget's board; null when it is unreachable or the bridge is backing off. */
    state,

    /**
     * The mod's switches from the latest board, looked up again when the last one is older than 30 s (a clock that went
     * backwards counts as older). The defaults when there is no board to read. Never throws.
     */
    async prefs(): Promise<Prefs> {
      try {
        const now = await io.now()
        if (cached && now - cached.at >= 0 && now - cached.at < PREFS_TTL_MS) return cached.value
        // `state` refreshes the cache itself; with no board the switches are the defaults, and nothing is cached.
        if (!(await state())) return DEFAULT_PREFS
        return cached?.value ?? DEFAULT_PREFS
      } catch {
        return DEFAULT_PREFS
      }
    },

    /** Carries out a command in the widget; null when it is unreachable or the bridge is backing off. Never throws. */
    async action(body: Action): Promise<ActionReply | null> {
      try {
        // an older app has no such route (404): that says nothing about the widget, so no backoff either
        const res = await request(ACTIONS_PATH, { method: 'POST', body: JSON.stringify({ v: 1, ...body }) }, [404, 503])
        if (!res) return null
        if (res.status === 404) return { ok: false, error: 'unsupported' }
        if (res.status === 503) return { ok: false, error: 'busy' }
        if (!res.ok) return { ok: false, error: 'rejected' }
        const reply: unknown = JSON.parse(res.text)
        if (typeof reply !== 'object' || reply === null || typeof (reply as ActionReply).ok !== 'boolean') return { ok: false, error: 'rejected' }
        // a switch just changed: the next look reads the board instead of the cached switches
        if (body.action === 'prefs') cached = undefined
        return reply as ActionReply
      } catch {
        return { ok: false, error: 'rejected' }
      }
    },

    /** True after the widget answered 400 for that session: stop sending it anything. */
    stopped(sessionId: string): boolean {
      return stoppedSessions.has(sessionId)
    },
  }
}

export type Bridge = ReturnType<typeof createBridge>

let shared: Bridge | undefined

/**
 * The one bridge of this module instance, so the report hooks and the pane share one backoff and one stopped list.
 * The first caller's `io` wins; later calls get the same bridge. Tests inject their own instead.
 */
export function sharedBridge(io: BridgeIo): Bridge {
  return (shared ??= createBridge(io))
}
