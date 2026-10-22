import { test, expect, mock } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On } from 'claude-code'
import { step } from './pet'
import type { PetEvent } from './pet'
import { COLS, FRAMES, ROWS, encode } from './sprites'
import type { PetState } from './sprites'

const NOW = 1_800_000_000_000
const STATES = Object.keys(FRAMES) as PetState[]
const SECOND = 1000
const SLEEP = 5 * 60 * SECOND

const BAND = { hasSurvey: false, isWorking: false, maxRows: 12, bodyColumns: 100, scroll: { offset: 0, bodyRows: 12 } }
const SURFACES = ['terminal', 'desktop', 'vscode', 'mobile'] as const

type Dollar = Engine
type Opts = { prefs?: { pet?: boolean; nudges?: boolean }; board?: unknown; verdict?: 'allow' | 'ask' | 'deny'; drop?: boolean; duplicate?: boolean }
type Found = { type: string; key: string | undefined; props: Record<string, unknown>; text: string }
type Band = { find: (q: object) => Promise<Found | undefined>; redraw: () => Promise<void>; unmount: () => Promise<void> }

// The widget's board with the app's switches on it: the pet is on unless a test says otherwise.
const boardWith = (prefs: { pet?: boolean; nudges?: boolean } = {}) => ({
  v: 1,
  app_version: '0.16.0',
  prefs: { pet: true, nudges: true, ...prefs },
  sessions: [],
  limits: [],
})

// Everything beneath the plugin: the clock, the widget's board (and its endpoint file), what the engine answers to the events the pet watches, core's own band
// (an empty Box keyed `core`) and the blits. The test steers what a blit answers through `w.blit` and the permission verdict through `w.verdict`, and holds a
// tool open with `w.gate`.
function world(on: On, opts: Opts = {}) {
  const clock = mock.clock(on, { now: NOW })
  mock.env(on, { USERPROFILE: 'C:\\Users\\tester' })
  on('fs.exists', ($$, e) => ({ value: opts.duplicate === true && e.path.split(String.fromCharCode(92)).join('/') === 'C:/Users/tester/.claude/skills/agent-pets/.claude-plugin/plugin.json' }))
  on('fs.read', ($$, e) => {
    if (e.path.split(String.fromCharCode(92)).join('/') === 'C:/Users/tester/.agent-pets/endpoint.json') return { value: JSON.stringify({ port: 4711, token: 'tok' }) }
    return { deny: 'ENOENT' }
  })
  on('session.start', ($$, e) => ({ cwd: e.cwd }))
  on('prompt.submit', ($$, e) => ({ text: e.text, ...(opts.drop ? { drop: 'no' } : {}) }) as never)
  on('turn.complete', ($$, e) => ({ text: e.answer }) as never)
  const w = {
    clock,
    // What the widget answers to `GET /v1/state`: a board, or 'down' for no widget. A test changes it between ticks.
    board: ('board' in opts ? opts.board : boardWith(opts.prefs)) as unknown,
    fetches: 0,
    blits: [] as { requestId: string; key: string; cells: string }[],
    attempts: 0,
    blit: 'ok' as 'ok' | 'deny' | 'throw',
    gate: undefined as Promise<void> | undefined,
    // The engine checks a call before it runs it: with `ask` a `$.tool.call` waits for the user and never runs.
    verdict: opts.verdict ?? 'allow',
    stall: false,
    stateFails: false,
  }
  on('state.get', async ($$, e, next) => {
    if (w.stall) await new Promise<never>(() => {})
    if (w.stateFails) return { deny: 'EIO' }
    return next(e)
  })
  on('state.set', async ($$, e, next) => (w.stateFails ? { deny: 'EIO' } : next(e)))
  on('tool.check', () => ({ decision: w.verdict }))
  on('tool.call', async () => {
    await w.gate
    return { result: 'ok' } as never
  })
  on('http.fetch', () => {
    w.fetches++
    if (w.board === 'down') return { deny: 'ECONNREFUSED' }
    return { value: { status: 200, ok: true, headers: {}, text: JSON.stringify(w.board) } }
  })
  on('ui.render', () => ({ type: 'Box', props: { key: 'core' }, children: [] }) as never)
  on('ui.blit', ($$, e) => {
    w.attempts++
    if (w.blit === 'throw') throw new Error('boom')
    if (w.blit === 'deny') return { value: { deny: 'not mounted' } }
    w.blits.push({ requestId: e.requestId, key: e.key, cells: (e as { cells: string }).cells })
    return { value: {} }
  })
  return w
}

const startTerminal = ($: Dollar) => $.session.start({ cwd: 'C:/w', surface: 'terminal', isInteractive: true } as never)
const submit = ($: Dollar) => $.prompt.submit({ text: 'fix it' } as never)
const complete = ($: Dollar, reason: 'answer' | 'aborted' | 'refusal' | 'error') =>
  $.turn.complete({ answer: 'x', durationMs: 1, isAborted: reason === 'aborted', turnId: 't1', reason } as never)
const mountBand = ($: Dollar, props: Record<string, unknown> = BAND, surface: (typeof SURFACES)[number] = 'terminal', requestId = 'band') =>
  $.ui.mount({ plugin: 'agent-pets', surface, component: 'AbovePrompt', requestId, props } as never) as never as Promise<Band>

// What the band says the pet does (one line of text, none while idle): the pet's state as the person sees it.
const says = async (ui: Band) => {
  await ui.redraw()
  return (await ui.find({ type: 'Text' }))?.text ?? ''
}

test('step: a prompt thinks, a tool works and its end thinks again', () => {
  expect(step('idle', 'prompt', 0)).toBe('thinking')
  expect(step('sleeping', 'prompt', 0)).toBe('thinking')
  expect(step('error', 'prompt', 0)).toBe('thinking')
  expect(step('done', 'prompt', 0)).toBe('thinking')
  expect(step('thinking', 'tool', 0)).toBe('working')
  expect(step('working', 'tool_done', 0)).toBe('thinking')
})

test('step: a permission or question waits, and the answer takes the turn back to thinking', () => {
  expect(step('working', 'ask', 0)).toBe('waiting')
  expect(step('thinking', 'ask', 0)).toBe('waiting')
  expect(step('waiting', 'tool_done', 0)).toBe('thinking')
  expect(step('waiting', 'tool', 0)).toBe('working')
})

test('step: an answer is done, an interrupt is idle, an error or refusal is an error until the next prompt', () => {
  expect(step('thinking', 'answer', 0)).toBe('done')
  expect(step('working', 'aborted', 0)).toBe('idle')
  expect(step('waiting', 'aborted', 0)).toBe('idle')
  expect(step('thinking', 'error', 0)).toBe('error')
  expect(step('error', 'tick', 10 * SLEEP)).toBe('error')
  expect(step('error', 'prompt', 0)).toBe('thinking')
})

test('step: news that arrives after the turn ended changes nothing', () => {
  for (const state of ['idle', 'done', 'error', 'thinking'] as const) expect(step(state, 'tool_done', 0)).toBe(state)
})

test('step: done goes idle after 3 s of ticks, idle sleeps after 5 minutes, only those', () => {
  expect(step('done', 'tick', 2999)).toBe('done')
  expect(step('done', 'tick', 3000)).toBe('idle')
  expect(step('idle', 'tick', SLEEP - 1)).toBe('idle')
  expect(step('idle', 'tick', SLEEP)).toBe('sleeping')
  for (const state of ['thinking', 'working', 'waiting'] as const) expect(step(state, 'tick', 10 * SLEEP)).toBe(state)
  expect(step('sleeping', 'tick', 10 * SLEEP)).toBe('sleeping')
})

test('step: any event wakes a sleeping pet', () => {
  const events: PetEvent[] = ['prompt', 'tool', 'tool_done', 'ask', 'answer', 'aborted', 'error']
  for (const ev of events) expect(step('sleeping', ev, 0)).not.toBe('sleeping')
})

test('every state has two to four frames of 16 x 16 palette pixels', () => {
  expect([...STATES].sort()).toEqual(['done', 'error', 'idle', 'sleeping', 'thinking', 'waiting', 'working'])
  for (const state of STATES) {
    expect(FRAMES[state].length).toBeGreaterThanOrEqual(2)
    expect(FRAMES[state].length).toBeLessThanOrEqual(4)
    for (const frame of FRAMES[state]) {
      expect(frame.length).toBe(COLS)
      expect(frame.length).toBe(ROWS * 2)
      for (const row of frame) expect(row).toMatch(/^[kmshewx.]{16}$/)
      expect(frame.join('')).toMatch(/m/)
    }
  }
})

test('the frames of a state differ from one another, and the states from each other', () => {
  for (const state of STATES) expect(new Set(FRAMES[state].map(f => f.join('/'))).size).toBeGreaterThan(1)
  const firsts = STATES.map(state => FRAMES[state][0]!.join('/'))
  expect(new Set(firsts).size).toBe(STATES.length)
})

test('encode packs two pixel rows into one cell row, as base64 of 16 x 8 u32 triplets', () => {
  const rows = Array.from({ length: 16 }, () => '.'.repeat(16))
  const put = (row: number, col: number, ch: string) => {
    rows[row] = rows[row]!.slice(0, col) + ch + rows[row]!.slice(col + 1)
  }
  put(0, 0, 'k') // top only
  put(3, 1, 'm') // bottom only
  put(4, 2, 'h') // both: h over e
  put(5, 2, 'e')
  const bytes = Uint8Array.from(atob(encode(rows)), ch => ch.charCodeAt(0))
  expect(bytes.length).toBe(16 * 8 * 12)
  const cell = (col: number, row: number) => {
    const view = new DataView(bytes.buffer, (row * COLS + col) * 12, 12)
    return [view.getUint32(0, true), view.getUint32(4, true), view.getUint32(8, true)]
  }
  const DEFAULT = 0x01000000
  expect(cell(0, 0)).toEqual([0x2580, 0x2b1d16, DEFAULT])
  expect(cell(1, 1)).toEqual([0x2584, 0xd97757, DEFAULT])
  expect(cell(2, 2)).toEqual([0x2580, 0xf2ae92, 0x1e1410])
  expect(cell(5, 5)).toEqual([0x20, DEFAULT, DEFAULT])
})

test('every encoded frame is the size the Raster is mounted at', () => {
  for (const state of STATES) for (const frame of FRAMES[state]) expect(atob(encode(frame)).length).toBe(COLS * ROWS * 12)
})

for (const surface of SURFACES) {
  test(`the band on ${surface} draws the pet on a terminal only, and never throws`, async ($, on) => {
    world(on)
    const ui = await mountBand($, BAND, surface)
    const raster = await ui.find({ type: 'Raster', key: 'pet' })
    if (surface === 'terminal') {
      expect(raster).toBeDefined()
      expect(await ui.find({ type: 'Box', key: 'core' })).toBeUndefined()
    } else {
      expect(raster).toBeUndefined()
      expect(await ui.find({ type: 'Box', key: 'core' })).toBeDefined()
    }
    await ui.unmount()
  })
}

test('the pet starts as the first idle frame, at the Raster size', async ($, on) => {
  world(on)
  const ui = await mountBand($)
  const raster = (await ui.find({ type: 'Raster', key: 'pet' }))!
  expect(raster.props.columns).toBe(COLS)
  expect(raster.props.rows).toBe(ROWS)
  expect(raster.props.cells).toBe(encode(FRAMES.idle[0]!))
  await ui.unmount()
})

test('a terminal narrower than 60 columns draws nothing, and 60 is enough', async ($, on) => {
  world(on)
  const narrow = await mountBand($, { ...BAND, bodyColumns: 50 }, 'terminal', 'narrow')
  expect(await narrow.find({ type: 'Raster' })).toBeUndefined()
  expect(await narrow.find({ type: 'Box', key: 'core' })).toBeDefined()
  await narrow.unmount()
  const edge = await mountBand($, { ...BAND, bodyColumns: 60 }, 'terminal', 'edge')
  expect(await edge.find({ type: 'Raster' })).toBeDefined()
  await edge.unmount()
})

test('a survey, or too few rows, leaves the band to the engine', async ($, on) => {
  world(on)
  const survey = await mountBand($, { ...BAND, hasSurvey: true }, 'terminal', 'survey')
  expect(await survey.find({ type: 'Raster' })).toBeUndefined()
  expect(await survey.find({ type: 'Box', key: 'core' })).toBeDefined()
  await survey.unmount()
  const short = await mountBand($, { ...BAND, maxRows: 5 }, 'terminal', 'short')
  expect(await short.find({ type: 'Raster' })).toBeUndefined()
  await short.unmount()
})

test('a board with the pet on draws it', async ($, on) => {
  world(on, { prefs: { pet: true } })
  const ui = await mountBand($)
  expect(await ui.find({ type: 'Raster' })).toBeDefined()
  expect(await ui.find({ type: 'Box', key: 'core' })).toBeUndefined()
  await ui.unmount()
})

// Each draws nothing and leaves the band to the engine: the pet is off, there is no widget, or an older app sends no prefs.
const WITHOUT_PET: [string, Opts][] = [
  ['the pet off on the board', { prefs: { pet: false } }],
  ['no widget', { board: 'down' }],
  ['a board from an older app without prefs', { board: { v: 1, app_version: '0.16.0', sessions: [], limits: [] } }],
  ['a board whose pet is not a boolean', { board: { ...boardWith(), prefs: { pet: 'yes' } } }],
]
for (const [name, opts] of WITHOUT_PET) {
  test(`${name} draws no pet, and never throws`, async ($, on) => {
    world(on, opts)
    const ui = await mountBand($)
    expect(await ui.find({ type: 'Raster' })).toBeUndefined()
    expect(await ui.find({ type: 'Box', key: 'core' })).toBeDefined()
    await ui.unmount()
  })
}

test('a switch changed in the app while the session runs brings the band, and takes it away', async ($, on) => {
  const w = world(on, { prefs: { pet: false } })
  await startTerminal($)
  const ui = await mountBand($)
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()
  // the timer has no band to blit to yet: the switch must still be noticed (the board is read at most every 30 s)
  w.board = boardWith({ pet: true })
  await w.clock.advance(31_000)
  expect(await ui.find({ type: 'Raster' })).toBeDefined()
  w.board = boardWith({ pet: false })
  await w.clock.advance(31_000)
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()
  expect(await ui.find({ type: 'Box', key: 'core' })).toBeDefined()
  await ui.unmount()
})

test('the board is read at most every 30 s for the pet, not on every 200 ms tick', async ($, on) => {
  // nudges off: they poll the board on their own and would be counted too
  const w = world(on, { prefs: { nudges: false } })
  await startTerminal($)
  const ui = await mountBand($)
  await w.clock.advance(10_000)
  expect(w.fetches).toBe(1)
  await w.clock.advance(21_000)
  expect(w.fetches).toBe(2)
  await ui.unmount()
})

test('a prompt, a tool, a permission ask and an answer take the pet through its states', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  const ui = await mountBand($)
  expect(await says(ui)).toBe('')
  await submit($)
  expect(await says(ui)).toBe('thinking\u2026')
  let release: () => void = () => {}
  w.gate = new Promise<void>(resolve => (release = resolve))
  const call = $.tool.call({ tool: 'Bash', command: 'ls' } as never)
  await w.clock.advance(0)
  expect(await says(ui)).toBe('working\u2026')
  release()
  await call
  expect(await says(ui)).toBe('thinking\u2026')
  w.verdict = 'ask'
  await $.tool.check({ tool: 'Bash', input: { command: 'rm -rf x' }, tool_use_id: 'tu1' } as never)
  expect(await says(ui)).toBe('waiting for you')
  await complete($, 'answer')
  expect(await says(ui)).toBe('done')
  await ui.unmount()
})

test('an allowed tool does not make the pet wait', async ($, on) => {
  world(on, { verdict: 'allow' })
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  await $.tool.check({ tool: 'Read', input: { file_path: 'a' } } as never)
  expect(await says(ui)).toBe('thinking\u2026')
  await ui.unmount()
})

test('a plugin asking whether a tool would be allowed does not make the pet wait', async ($, on) => {
  world(on, { verdict: 'ask' })
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  await $.tool.check({ tool: 'Bash', input: { command: 'rm -rf x' } } as never)
  expect(await says(ui)).toBe('thinking…')
  await ui.unmount()
})

test('with tools in parallel the pet works until the last one ends', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  let releaseFirst: () => void = () => {}
  let releaseSecond: () => void = () => {}
  w.gate = new Promise<void>(resolve => (releaseFirst = resolve))
  const first = $.tool.call({ tool: 'Read', file_path: 'a' } as never)
  await w.clock.advance(0)
  w.gate = new Promise<void>(resolve => (releaseSecond = resolve))
  const second = $.tool.call({ tool: 'Read', file_path: 'b' } as never)
  await w.clock.advance(0)
  expect(await says(ui)).toBe('working…')
  releaseFirst()
  await first
  expect(await says(ui)).toBe('working…')
  releaseSecond()
  await second
  expect(await says(ui)).toBe('thinking…')
  await ui.unmount()
})

test('AskUserQuestion waits while it is open and thinks once it is answered', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  let release: () => void = () => {}
  w.gate = new Promise<void>(resolve => (release = resolve))
  const call = $.tool.call({ tool: 'AskUserQuestion', questions: [] } as never)
  await w.clock.advance(0)
  expect(await says(ui)).toBe('waiting for you')
  release()
  await call
  expect(await says(ui)).toBe('thinking\u2026')
  await ui.unmount()
})

test('a turn that is interrupted is idle, one that errors or is refused is an error until the next prompt', async ($, on) => {
  world(on)
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  await complete($, 'aborted')
  expect(await says(ui)).toBe('')
  await submit($)
  await complete($, 'error')
  expect(await says(ui)).toBe('something went wrong')
  await submit($)
  expect(await says(ui)).toBe('thinking\u2026')
  await complete($, 'refusal')
  expect(await says(ui)).toBe('something went wrong')
  await submit($)
  expect(await says(ui)).toBe('thinking\u2026')
  await ui.unmount()
})

test('a prompt the engine dropped does not start the pet thinking', async ($, on) => {
  world(on, { drop: true })
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  expect(await says(ui)).toBe('')
  await ui.unmount()
})

test('after a finished turn the pet waves for 3 s and then is idle, and falls asleep after 5 minutes; a prompt wakes it', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  await complete($, 'answer')
  await w.clock.advance(2800)
  expect(await says(ui)).toBe('done')
  await w.clock.advance(400)
  expect(await says(ui)).toBe('')
  await w.clock.advance(SLEEP - 3200 - 1000)
  expect(await says(ui)).toBe('')
  await w.clock.advance(2000)
  expect(await says(ui)).toBe('zzz')
  await submit($)
  expect(await says(ui)).toBe('thinking\u2026')
  await ui.unmount()
})

test('a long turn never falls asleep', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  await w.clock.advance(SLEEP + 10 * SECOND)
  expect(await says(ui)).toBe('thinking\u2026')
  await ui.unmount()
})

test('the timer blits the next frame to the mounted band every 200 ms', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  const ui = await mountBand($)
  await w.clock.advance(200)
  expect(w.blits).toEqual([{ requestId: 'band', key: 'pet', cells: encode(FRAMES.idle[1]!) }])
  await w.clock.advance(200)
  expect(w.blits[1]).toEqual({ requestId: 'band', key: 'pet', cells: encode(FRAMES.idle[2]!) })
  await submit($)
  await w.clock.advance(200)
  expect(w.blits.at(-1)!.cells).toBe(encode(FRAMES.thinking[3 % FRAMES.thinking.length]!))
  await ui.unmount()
})

test('nothing is blitted while no band is mounted, and a refused blit stops them until the band draws again', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  await w.clock.advance(1000)
  expect(w.attempts).toBe(0)
  const ui = await mountBand($)
  w.blit = 'deny'
  await w.clock.advance(200)
  expect(w.attempts).toBe(1)
  w.blit = 'ok'
  await w.clock.advance(1000)
  expect(w.attempts).toBe(1)
  expect(w.blits).toEqual([])
  await ui.redraw()
  await w.clock.advance(200)
  expect(w.blits.length).toBe(1)
  await ui.unmount()
})

test('a blit that throws does not stop the animation', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  const ui = await mountBand($)
  w.blit = 'throw'
  await w.clock.advance(600)
  expect(w.attempts).toBe(3)
  w.blit = 'ok'
  await w.clock.advance(200)
  expect(w.blits.length).toBe(1)
  await ui.unmount()
})

test('a session that is not an interactive terminal starts no animation', async ($, on) => {
  const w = world(on)
  await $.session.start({ cwd: 'C:/w', surface: null, isInteractive: false } as never)
  const ui = await mountBand($)
  await w.clock.advance(1000)
  expect(w.attempts).toBe(0)
  await ui.unmount()
})

test('a subagent turn or tool does not move the pet', async ($, on) => {
  world(on)
  await startTerminal($)
  const ui = await mountBand($)
  await submit($)
  await $.tool.call({ tool: 'Bash', command: 'ls', agentId: 'sub1' } as never)
  expect(await says(ui)).toBe('thinking\u2026')
  await $.turn.complete({ answer: 'x', durationMs: 1, isAborted: false, turnId: 't2', reason: 'answer', agentId: 'sub1' } as never)
  expect(await says(ui)).toBe('thinking\u2026')
  await ui.unmount()
})

test('a duplicate copy of the mod draws no band, blits nothing and leaves the band to the engine', async ($, on) => {
  const w = world(on, { duplicate: true })
  await startTerminal($)
  const ui = await mountBand($)
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()
  expect(await ui.find({ type: 'Box', key: 'core' })).toBeDefined()
  await submit($)
  await w.clock.advance(1000)
  await ui.redraw()
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()
  expect(w.attempts).toBe(0)
  expect(w.blits).toEqual([])
  await ui.unmount()
})

test('a tool call is handed on without waiting for the pet', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  // the pet cannot reach its state: it never answers
  w.stall = true
  const out = await Promise.race([
    $.tool.call({ tool: 'Bash', command: 'ls' } as never),
    new Promise(resolve => setTimeout(() => resolve('stuck'), 200)),
  ])
  expect(out).not.toBe('stuck')
})

test('a state that fails never throws out of a tool call and the result still comes back', async ($, on) => {
  const w = world(on)
  await startTerminal($)
  w.stateFails = true
  const out = (await $.tool.call({ tool: 'Bash', command: 'ls' } as never)) as { result?: unknown }
  expect(out.result).toBe('ok')
  const ask = (await $.tool.call({ tool: 'AskUserQuestion', questions: [] } as never)) as { result?: unknown }
  expect(ask.result).toBe('ok')
})
