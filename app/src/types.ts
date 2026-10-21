// Mirrors the model in crates/pets-core/src/model.rs (serde: snake_case, Option → null).
export type Agent = 'claude' | 'codex' | 'opencode' | 'antigravity' | 'copilot' | 'cursor' | 'grok' | 'zcode' | 'other';
/** App running the session (`App` in core). */
export type App = 'terminal' | 'claude_desktop' | 'codex_app' | 'vscode' | 't3code' | 'cursor' | 'antigravity' | 'zed' | 'jetbrains' | 'zcode' | 'other';
export type Origin = 'cli' | 'desktop' | 'router';
export type State = 'thinking' | 'working' | 'needs_you' | 'done' | 'error' | 'idle' | 'sleep' | 'compacting' | 'ended';
export type Tool = 'edit' | 'bash' | 'read' | 'grep' | 'web' | 'agent' | 'mcp' | 'other';

export interface Progress { done: number; total: number }
export interface Context { used: number; max: number }

export interface Session {
  id: string;
  agent: Agent;
  origin: Origin;
  title: string;
  cwd: string;
  state: State;
  tool: Tool | null;
  progress: Progress | null;
  context: Context | null;
  started_at: number;
  last_activity: number;
  state_since: number;
  turn_started_at: number | null;
  jump: { pid: number | null; session_id: string; cwd: string; app: App | null; app_name?: string | null; host_pid?: number | null };
  /** usage from the agent database (since 0.11, opencode only) */
  usage?: Usage | null;
  /** Agent Router task linked to this Codex thread */
  router_task?: RouterTask | null;
  /** parent session ID; absent = ordinary session */
  parent?: string | null;
  /** only for child sessions (subagents) */
  sub?: SubInfo | null;
  /** current action text (only in core and window memory) */
  action?: string | null;
  /** question text, only in `needs_you` */
  question?: string | null;
  /** pinned by the user: sorts right after the sessions that need you */
  pinned?: boolean;
  /** `title` is the user's own name (the menu offers to reset it) */
  renamed?: boolean;
  /** model ID (e.g. `claude-opus-5-5`), since 0.10 */
  model?: string | null;
  /** `other` agent name (from the bridge), since 0.10 */
  agent_name?: string | null;
}

export type SubKind = 'claude' | 'codex' | 'router' | 'opencode' | 'copilot' | 'cursor';
export interface SubInfo { kind: SubKind; agent_type: string | null; description: string | null; background: boolean }

export interface RouterTask { task_id: string; status: string; last_activity_at: number | null; blocked: boolean; stall_ms: number }

/** `stale_since`: time of the last real reading when the value is old (the Claude app stopped polling). */
export interface Limit { agent: Agent; window: 'five_hour' | 'weekly' | 'spend' | 'monthly'; used_pct: number; resets_at: number | null; stale_since?: number | null }
/** Session tokens and cost from the agent database (opencode only since 0.11); `account` = account whose limits apply. */
export interface Usage { tokens: number; cost: number; account: Agent | null }
/** Agent's daily total (since local midnight). */
export interface AgentUsage { agent: Agent; tokens_today: number; cost_today: number }
/** A limit window that runs out at `runs_out_at` (ms) before it resets, if the pace holds. */
export interface Forecast { agent: Agent; window: Limit['window']; runs_out_at: number }
export interface Snapshot { sessions: Session[]; limits: Limit[]; now: number; agent_usage?: AgentUsage[]; forecasts?: Forecast[] }
/** Layout from Rust; `mode` and `light` since 0.7 (absent = taskbar, dark). */
export interface StageLayout { max_css: number; height_css: number; scale: number; mode?: 'taskbar' | 'floating'; light?: boolean; left_fallback?: boolean; vertical_bar?: boolean }
export type PointerMsg =
  | { kind: 'move'; x: number; y: number }
  | { kind: 'leave' }
  | { kind: 'click'; x: number; y: number }
  | { kind: 'context'; x: number; y: number };
export interface TooltipContent { title: string; subtitle: string; lines: string[] }

export type AppId = 'claude_code' | 'codex' | 'agent_router' | 'opencode' | 'copilot' | 'antigravity' | 'cursor' | 'grok' | 'zcode';
export type StyleId = 'sketch' | 'clean' | 'sticker' | 'pixel' | 'neon' | 'ink' | 'pastel';
export type MotionId = 'calm' | 'dynamic';
export interface Look { style: StyleId; motion: MotionId }
/** `react_to_media` since 0.9.1 (absent = enabled): an idle pet listens to system music. */
export interface Pets { style: StyleId; motion: MotionId; overrides: Partial<Record<AppId, Partial<Look>>>; max_visible: number; react_to_media?: boolean }
/** Mirrors `media::Media` (`pets://media` event): whether something is playing in Windows (GSMTC), and in which app. */
export interface Media { playing: boolean; app: string | null }
/** `opencode` and `generic` (bridge) since 0.10, `copilot` and `antigravity` since 0.11, `cursor`, `grok`, `zcode` since 0.12; absent in older files = core defaults. */
export interface AppsSettings { claude_code: boolean; codex: boolean; agent_router: boolean; opencode?: boolean; generic?: boolean; copilot?: boolean; antigravity?: boolean;
  cursor?: boolean; grok?: boolean; zcode?: boolean }
export interface Settings {
  version: number;
  apps: AppsSettings;
  claude_plan_usage: boolean;
  /** Claude Code mod in `~/.claude/skills/agent-pets` (since 0.16); on by default. */
  claude_mod: boolean;
  /** The mod's pixel pet above the Claude Code prompt (0.16.1); off by default. */
  claude_mod_pet: boolean;
  /** The mod's nudges about other agents (0.16.1); on by default. */
  claude_mod_nudges: boolean;
  /** `muted_until`: toasts are silenced until then (ms since epoch; a huge value = until turned back on); absent or null = not muted (0.17). */
  notifications: { needs_you: boolean; done: boolean; limits: boolean; weekly: boolean; sound: boolean; muted_until?: number | null };
  pets: Pets;
  power_saving: 'auto' | 'always' | 'never';
  autostart: boolean;
  language: Language;
  theme: Theme;
  updates: Updates;
  stage: StageSettings;
  /** System-wide shortcuts (0.17), accelerator strings like `Ctrl+Alt+Shift+J`; `null` = off. */
  hotkeys: Hotkeys;
  [extra: string]: unknown;
}
export interface Hotkeys { jump: string | null; panel: string | null }
/** Mirrors `hotkeys::HotkeyStatus` (command `hotkeys_status`, event `pets://hotkeys`): why a shortcut could not be registered, or `null`. */
export interface HotkeyStatus { jump: string | null; panel: string | null }
export type Theme = 'system' | 'light' | 'dark';
export type Updates = 'notify' | 'auto' | 'off';
/** Lustro `placement::MonitorInfo` (komenda `monitors_list`). */
export interface MonitorInfo { id: string; primary: boolean; width: number; height: number; index: number; has_bar: boolean }
/** Lustro `updater::UpdateStatus` (zdarzenie `pets://update`). */
export type UpdateStatus =
  | { state: 'idle' } | { state: 'checking' } | { state: 'latest' }
  | { state: 'available'; version: string; notes: string | null }
  | { state: 'downloading'; version: string; pct: number | null }
  | { state: 'ready'; version: string }
  /** `verify`: problem with the update itself (panel and settings); otherwise a check error (settings only) */
  | { state: 'error'; message: string; verify: boolean };
/** Mirrors `notify::center::Entry` (event `pets://notifications`). */
export type NotificationKind = 'needs_you' | 'done' | 'limit' | 'update' | 'updated' | 'weekly' | 'problem';
export interface NotificationEntry { id: number; kind: NotificationKind; title: string; body: string; session_id: string | null; at: number; read: boolean }
export type StagePosition = 'right' | 'left' | 'custom' | 'floating';
export type StageAlign = 'left' | 'center' | 'right';
export type StageOrder = 'start' | 'attention' | 'agent';
export interface StageBackground { kind: 'none' | 'glass' | 'solid'; color?: string | null; opacity?: number | null; radius: number }
/** Mirrors core `settings::Stage` (Taskbar tab). */
export interface StageSettings {
  position: StagePosition;
  /** taskbar anchor as a fraction of taskbar width (0–1) */
  custom_at?: number | null;
  dock?: 'left_start' | 'left_end' | 'right_start' | 'right_end' | 'top' | 'bottom' | null;
  /** floating window anchor (CSS px relative to the monitor work area) */
  floating_at?: { x: number; y: number } | null;
  monitor: string;
  background: StageBackground;
  size: number;
  gap: number;
  padding: number;
  align: StageAlign;
  order: StageOrder;
  show: { progress: boolean; limits: boolean; badge: boolean };
  bubbles: { questions: boolean; actions: boolean };
  /** mini pets for subagents working longer than 5 s */
  minis: boolean;
}
export type Language = 'auto' | 'pl' | 'en';
/** `lang`: language of Rust text (setting or Windows language), for UI `auto`. */
export interface SettingsView { settings: Settings; first_run: boolean; load_error: string | null; lang?: 'pl' | 'en'; system_lang?: 'pl' | 'en' }
export interface AppRow {
  id: AppId;
  detected: { found: boolean; path: string | null; note: string | null };
  status: { installed: boolean; detail: string };
  enabled: boolean;
}
export interface Diagnostics {
  version: string;
  endpoint_port: number | null;
  settings_path: string;
  settings_error: string | null;
  hook_exe: string | null;
  autostart_registered: boolean;
  last_seen: Record<string, number>;
  apps: [AppId, boolean, string][];
  stats_files: number;
  stats_scanned_bytes: number;
  stats_total_bytes: number;
  log_path: string;
  log_tail: string[];
}

// Statystyki (0.9): lustro `pets_core::stats::summary::StatsView` i `scan::Progress`.
export type StatAgent = 'claude' | 'codex' | 'router' | 'opencode';
export type StatsPeriod = 'today' | 'week' | 'month' | 'all';
export type StatsMetric = 'time' | 'tokens';
export type StatsRace = 'agents' | 'projects';
export type BadgeKind = 'glutton' | 'cache_master' | 'night_owl' | 'marathon';
export interface StatsPlace { project: string; value: number; agent: StatAgent }
export interface StatsTiles {
  tokens: number; tokens_change: number | null; cache_pct: number | null; cache_read: number;
  active_ms: number; longest_ms: number; sessions: number; subagents: number; questions: number;
}
export interface StatsLane { key: string; agent: StatAgent | null; value: number }
export interface StatsDay { date: string; active_ms: number; level: number }
export interface StatsBadge { kind: BadgeKind; project: string | null; agent: StatAgent | null; value: number }
export interface WeekTotals { active_ms: number; tokens: number; sessions: number; top_project: string | null; top_agent: StatAgent | null }
export interface WeekCompare { this: WeekTotals; last: WeekTotals; days_into_week: number }
export interface StatsView { empty: boolean; podium: StatsPlace[]; tiles: StatsTiles; race: StatsLane[]; calendar: StatsDay[]; badges: StatsBadge[]; record: boolean; week: WeekCompare }
export interface StatsProgress { files: number; scanned: number; total: number; done: boolean }
