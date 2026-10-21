import { test, expect, mock } from 'claude-code/testing'
import type { On } from 'claude-code'
import { describe, parse } from './commands'
import type { Action } from './bridge'

const NOW = 1_800_000_000_000
const same = (t: unknown) => String(t ?? '')
const sent = (args: string, id = 'me'): Action | undefined => {
  const c = parse(args)
  return c.kind === 'action' ? c.action(id) : undefined
}

test('no arguments open the pane, help lists the commands', () => {
  expect(parse('').kind).toBe('pane')
  expect(parse('   ').kind).toBe('pane')
  expect(parse('help').kind).toBe('help')
})

test('commands name this session where they act on it', () => {
  expect(sent('jump')).toEqual({ action: 'jump', session_id: 'me' })
  expect(sent('panel')).toEqual({ action: 'panel', session_id: 'me' })
  expect(sent('pin')).toEqual({ action: 'pin', session_id: 'me', pinned: true })
  expect(sent('UNPIN')).toEqual({ action: 'pin', session_id: 'me', pinned: false })
  expect(sent('clear')).toEqual({ action: 'clear' })
})

test('mute takes an hour by default, 8 for the morning or forever', () => {
  expect(sent('mute')).toEqual({ action: 'mute', choice: 'hour' })
  expect(sent('mute 1h')).toEqual({ action: 'mute', choice: 'hour' })
  expect(sent('mute 8')).toEqual({ action: 'mute', choice: 'morning' })
  expect(sent('mute forever')).toEqual({ action: 'mute', choice: 'forever' })
  expect(sent('unmute')).toEqual({ action: 'mute', choice: 'off' })
  expect(parse('mute 2h').kind).toBe('usage')
})

test('a name keeps its own words and spacing, an empty one clears it', () => {
  expect(sent('name  Fix the  Parser ')).toEqual({ action: 'rename', session_id: 'me', name: 'Fix the  Parser' })
  expect(sent('name')).toEqual({ action: 'rename', session_id: 'me', name: null })
})

test('the pet and the nudges switch on or off, nothing else', () => {
  expect(sent('pet on')).toEqual({ action: 'prefs', pet: true })
  expect(sent('nudges off')).toEqual({ action: 'prefs', nudges: false })
  expect(parse('pet maybe').kind).toBe('usage')
  expect(parse('pet').kind).toBe('usage')
})

test('an unknown command says how to find the right one', () => {
  const c = parse('dance now')
  expect(c.kind).toBe('usage')
  expect(c.kind === 'usage' && c.text).toContain('/pets help')
})

test('replies are put into words', () => {
  const jump: Action = { action: 'jump', session_id: 'me' }
  expect(describe(jump, { ok: true, title: 'website', agent: 'codex' }, NOW, same)).toBe('Jumping to website (codex)')
  expect(describe(jump, { ok: true }, NOW, same)).toMatch(/Nobody is waiting/)
  expect(describe(jump, null, NOW, same)).toBe('Agent Pets widget not running')
  expect(describe(jump, { ok: false, error: 'unsupported' }, NOW, same)).toMatch(/update the app/)
  expect(describe({ action: 'clear' }, { ok: true, count: 2 }, NOW, same)).toBe('Cleared 2 inactive pets')
  expect(describe({ action: 'clear' }, { ok: true, count: 0 }, NOW, same)).toBe('No inactive pets to clear')
  const forever: Action = { action: 'mute', choice: 'forever' }
  expect(describe(forever, { ok: true, until: 9223372036854775807 }, NOW, same)).toMatch(/until you turn them back on/)
  expect(describe({ action: 'mute', choice: 'off' }, { ok: true }, NOW, same)).toBe('Notifications back on')
  expect(describe({ action: 'rename', session_id: 'me', name: 'x' }, { ok: false, error: 'not_found' }, NOW, same)).toMatch(/does not know this session/)
})

// The widget behind the bridge: endpoint file, this session's id and one answer to the command.
function world(on: On, answer: { status: number; text?: string } | 'down', bodies: unknown[]) {
  mock.clock(on, { now: NOW })
  mock.env(on, { USERPROFILE: 'C:\\Users\\tester' })
  on('fs.exists', () => ({ value: false }))
  on('fs.read', ($$, e) => {
    if (e.path.split('\\').join('/') === 'C:/Users/tester/.agent-pets/endpoint.json') return { value: JSON.stringify({ port: 4711, token: 'tok' }) }
    return { deny: 'ENOENT' }
  })
  on('session.id', () => ({ value: 'me' }))
  on('http.fetch', ($$, e) => {
    bodies.push({ url: e.url, body: JSON.parse(e.init?.body ?? 'null') })
    if (answer === 'down') return { deny: 'ECONNREFUSED' }
    return { value: { status: answer.status, ok: answer.status < 300, headers: {}, text: answer.text ?? '' } }
  })
}

const run = (args: string) =>
  ({ command: 'pets', args, origin: { kind: 'composer' }, presentation: { isFullscreen: false, columns: 80 } }) as never

test('/pets jump sends the command with this session and prints the answer', async ($, on) => {
  const bodies: unknown[] = []
  world(on, { status: 200, text: JSON.stringify({ ok: true, title: 'web\u202esite', agent: 'codex' }) }, bodies)
  const out = await $.command.run(run('jump'))
  expect(out.text).toBe('Jumping to website (codex)')
  expect(bodies).toEqual([{ url: 'http://127.0.0.1:4711/v1/actions/claude-mod', body: { v: 1, action: 'jump', session_id: 'me' } }])
})

test('/pets mute on an app without commands says to update', async ($, on) => {
  world(on, { status: 404 }, [])
  expect((await $.command.run(run('mute'))).text).toMatch(/update the app/)
})

test('/pets clear without the widget says it is not running', async ($, on) => {
  world(on, 'down', [])
  expect((await $.command.run(run('clear'))).text).toBe('Agent Pets widget not running')
})

test('/pets help and mistakes never reach the widget', async ($, on) => {
  const bodies: unknown[] = []
  world(on, { status: 200, text: '{"ok":true}' }, bodies)
  expect((await $.command.run(run('help'))).text).toContain('/pets jump')
  expect((await $.command.run(run('mute 3h'))).text).toContain('/pets help')
  expect(bodies).toEqual([])
})

test('/pets where no surface places the pane prints the commands instead', async ($, on) => {
  world(on, { status: 200, text: '{"ok":true}' }, [])
  on('ui.open', () => ({ value: { isPlaced: false, reason: 'the desktop places no panes' } }))
  const out = await $.command.run(run(''))
  expect(out.text).toContain('does not show here')
  expect(out.text).toContain('/pets jump')
})
