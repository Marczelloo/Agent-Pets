import type { Action, ActionReply } from './bridge'

/** Shown dim after `/pets` in the typeahead. */
export const ARGUMENT_HINT = '[jump | panel | mute [1h|8|forever] | unmute | name [text] | pin | unpin | clear | pet on|off | nudges on|off | help]'

export const HELP = [
  '/pets                   sessions and limits of every agent',
  '/pets jump              go to the agent that needs you (else the latest one that finished)',
  '/pets panel             open the Agent Pets panel on this session',
  '/pets mute [1h|8|forever]  mute notifications for an hour (default), until 8:00 or until unmuted',
  '/pets unmute            notifications back on',
  '/pets name [text]       name this session in Agent Pets; without text, back to the automatic name',
  '/pets pin | unpin       keep this session near the top of the panel',
  '/pets clear             remove the pets of finished and idle sessions',
  '/pets pet on|off        the pixel pet above the prompt',
  '/pets nudges on|off     nudges about other agents',
].join('\n')

/** What `/pets <args>` asks for: the pane, the help, a command for the widget, or a mistake to point out. */
export type Command =
  | { kind: 'pane' }
  | { kind: 'help' }
  | { kind: 'usage'; text: string }
  | { kind: 'action'; action: (sessionId: string) => Action }

const MUTE: Record<string, 'hour' | 'morning' | 'forever'> = {
  '': 'hour', '1h': 'hour', hour: 'hour', '8': 'morning', '8:00': 'morning', morning: 'morning', forever: 'forever',
}
const SWITCH: Record<string, boolean> = { on: true, off: false }

const action = (make: (sessionId: string) => Action): Command => ({ kind: 'action', action: make })
const usage = (text: string): Command => ({ kind: 'usage', text: `${text} — /pets help lists the commands` })

export function parse(args: string): Command {
  const text = args.trim()
  if (text === '') return { kind: 'pane' }
  const [head = '', ...rest] = text.split(/\s+/)
  const verb = head.toLowerCase()
  const tail = text.slice(head.length).trim()
  const arg = (rest[0] ?? '').toLowerCase()
  switch (verb) {
    case 'help':
    case '?':
      return { kind: 'help' }
    case 'jump':
    case 'next':
      return action(id => ({ action: 'jump', session_id: id }))
    case 'panel':
      return action(id => ({ action: 'panel', session_id: id }))
    case 'mute': {
      const choice = MUTE[arg]
      if (!choice || rest.length > 1) return usage('Mute for 1h, until 8 or forever')
      return action(() => ({ action: 'mute', choice }))
    }
    case 'unmute':
      return action(() => ({ action: 'mute', choice: 'off' }))
    case 'name':
    case 'rename':
      return action(id => ({ action: 'rename', session_id: id, name: tail === '' ? null : tail }))
    case 'pin':
    case 'unpin':
      return action(id => ({ action: 'pin', session_id: id, pinned: verb === 'pin' }))
    case 'clear':
      return action(() => ({ action: 'clear' }))
    case 'pet':
    case 'nudges': {
      const on = SWITCH[arg]
      if (on === undefined || rest.length > 1) return usage(`Say /pets ${verb} on or /pets ${verb} off`)
      return action(() => (verb === 'pet' ? { action: 'prefs', pet: on } : { action: 'prefs', nudges: on }))
    }
    default:
      return usage(`Unknown command "${head.slice(0, 24)}"`)
  }
}

/** Beyond this the deadline is "until turned back on" (the app sends i64::MAX). */
const FOREVER = 1e15
const DAY_MS = 24 * 60 * 60 * 1000

function clock(at: number, now: number): string {
  try {
    const d = new Date(at)
    const time = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(d)
    return at - now < DAY_MS ? time : `${new Intl.DateTimeFormat(undefined, { weekday: 'short' }).format(d)} ${time}`
  } catch {
    return ''
  }
}

const FAILURE: Record<string, string> = {
  unsupported: 'This version of Agent Pets takes no commands yet — update the app',
  busy: 'Agent Pets did not answer in time',
  not_found: 'Agent Pets does not know this session yet — try again after its first turn',
  bad_choice: 'Agent Pets did not recognise that mute choice',
  save_failed: 'Agent Pets could not save the setting',
}

/**
 * The line `/pets <command>` prints for the widget's reply. `clean` tidies text that came from the widget
 * (a session title) before it is printed.
 */
export function describe(sent: Action, reply: ActionReply | null, now: number, clean: (text: unknown) => string): string {
  if (!reply) return 'Agent Pets widget not running'
  if (!reply.ok) return FAILURE[reply.error ?? ''] ?? 'Agent Pets did not take that command'
  switch (sent.action) {
    case 'jump': {
      const title = clean(reply.title)
      if (title === '') return 'Nobody is waiting for you — opened the Agent Pets panel'
      const agent = clean(reply.agent)
      return `Jumping to ${title}${agent ? ` (${agent})` : ''}`
    }
    case 'panel':
      return 'Opened the Agent Pets panel'
    case 'mute': {
      if (sent.choice === 'off' || typeof reply.until !== 'number') return 'Notifications back on'
      if (reply.until >= FOREVER) return 'Notifications muted until you turn them back on (/pets unmute)'
      const at = clock(reply.until, now)
      return at ? `Notifications muted until ${at}` : 'Notifications muted'
    }
    case 'rename':
      return sent.name === null ? 'This session has its automatic name again' : `Named this session "${clean(sent.name)}"`
    case 'pin':
      return sent.pinned ? 'Pinned this session' : 'Unpinned this session'
    case 'clear': {
      const n = typeof reply.count === 'number' ? reply.count : 0
      return n === 0 ? 'No inactive pets to clear' : `Cleared ${n} inactive ${n === 1 ? 'pet' : 'pets'}`
    }
    case 'prefs':
      if (sent.pet !== undefined) return `Pet above the prompt ${sent.pet ? 'on' : 'off'}`
      return `Nudges ${sent.nudges ? 'on' : 'off'}`
  }
}
