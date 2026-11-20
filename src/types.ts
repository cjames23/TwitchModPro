export type Severity = "low" | "medium" | "high";
export type RuleKind = "keyword" | "regex" | "links" | "caps" | "spam" | "repeat" | "firstMessage" | "newAccount";
export type FlagKind = "rule" | "automod" | "unbanRequest" | "suspicious";
export type FlagStatus = "open" | "reviewed" | "dismissed";
export type ConnStatus = "connecting" | "connected" | "disconnected";

export interface Channel {
  name: string;
  muted: boolean;
  broadcasterId: string | null;
  isMod: boolean;
}

export interface RuleParams {
  minLength: number;
  capsPercent: number;
  maxRepeatedChars: number;
  maxEmotes: number;
  repeatCount: number;
  repeatWindowSecs: number;
  maxAccountAgeDays: number;
}

export interface Rule {
  id: number;
  name: string;
  enabled: boolean;
  kind: RuleKind;
  patterns: string[];
  wholeWord: boolean;
  caseSensitive: boolean;
  channels: string[];
  severity: Severity;
  exemptPrivileged: boolean;
  target: "message" | "username";
  normalize: boolean;
  activeDuring: "always" | "raid";
  params: RuleParams;
}

export interface RuleMatch {
  ruleId: number;
  ruleName: string;
  severity: Severity;
  terms: string[];
  reason?: string;
}

export interface ContextLine {
  displayName: string;
  text: string;
  timestamp: number;
}

export interface Flag {
  id: number;
  channel: string;
  messageId: string;
  userId: string | null;
  userLogin: string;
  displayName: string;
  text: string;
  severity: Severity;
  matches: RuleMatch[];
  context: ContextLine[];
  contextAfter: ContextLine[];
  timestamp: number;
  status: FlagStatus;
  resolution: string | null;
  kind: FlagKind;
  externalId: string | null;
  meta: {
    accountCreatedAt?: number | null;
    firstMessage?: boolean;
    category?: string;
    level?: number | null;
    banEvasion?: string;
    lowTrustStatus?: string;
    sharedBans?: number;
    [k: string]: unknown;
  };
}

export interface Raid {
  id: number;
  channel: string;
  fromLogin: string;
  fromDisplayName: string;
  viewers: number;
  timestamp: number;
  acknowledged: boolean;
}

export interface LadderStep {
  action: "warn" | "timeout" | "ban";
  durationSecs?: number | null;
}

export interface Settings {
  notifyFlags: boolean;
  notifyMinSeverity: Severity;
  notifyRaids: boolean;
  raidMinViewers: number;
  playSounds: boolean;
  twitchClientId: string;
  raidModeMinutes: number;
  ladder: LadderStep[];
  ladderLookbackDays: number;
  waveMinAccounts: number;
  waveWindowSecs: number;
}

export interface ConnectionInfo {
  status: ConnStatus;
  joined: string[];
}

export interface Account {
  userId: string;
  login: string;
  scopes: string[];
}

export interface AuthState {
  clientIdSet: boolean;
  clientIdFromBuild: boolean;
  account: Account | null;
  login: { userCode: string; verificationUri: string; expiresAt: number } | null;
  error: string | null;
  checking: boolean;
}

export interface ModChannelOption {
  login: string;
  name: string;
  added: boolean;
  own: boolean;
}

export interface ModAction {
  id: number;
  channel: string;
  targetLogin: string;
  action: string;
  durationSecs: number | null;
  reason: string;
  ok: boolean;
  error: string | null;
  timestamp: number;
  byApp: boolean;
  targetUserId: string | null;
}

export interface WaveMember {
  login: string;
  displayName: string;
  userId: string | null;
  channel: string;
  messageId: string;
  text: string;
  timestamp: number;
  firstMessage: boolean;
  outcome: string | null;
}

export interface Wave {
  id: number;
  kind: "sameMessage" | "similarNames" | "firstTimeBurst";
  key: string;
  channels: string[];
  members: WaveMember[];
  firstSeen: number;
  lastSeen: number;
  status: "open" | "actioned" | "dismissed";
  severity: Severity;
}

export interface BulkResult {
  succeeded: number;
  failed: [string, string][];
  wave: Wave | null;
}

export interface ChannelResult {
  channel: string;
  ok: boolean;
  error: string | null;
}

export interface HistoryLine {
  id: string;
  text: string;
  timestamp: number;
  flagSeverity: Severity | null;
}

export interface UserHistory {
  channel: string;
  login: string;
  displayName: string;
  userId: string | null;
  messages: HistoryLine[];
  flags: Flag[];
  actions: ModAction[];
  otherChannels: ModAction[];
  accountCreatedAt: number | null;
}

export interface LiveMessage {
  id: string;
  channel: string;
  roomId: string | null;
  userId: string | null;
  userLogin: string;
  displayName: string;
  color: string | null;
  text: string;
  isAction: boolean;
  isMod: boolean;
  isBroadcaster: boolean;
  isVip: boolean;
  isSubscriber: boolean;
  isFirstMsg: boolean;
  emoteCount: number;
  timestamp: number;
  flagSeverity: Severity | null;
}

export type RaidModes = Record<string, number>;

export interface Snapshot {
  channels: Channel[];
  rules: Rule[];
  settings: Settings;
  flags: Flag[];
  raids: Raid[];
  connection: ConnectionInfo;
  auth: AuthState;
  raidModes: RaidModes;
  waves: Wave[];
  sanctions: ModAction[];
}

/** Who a mod action is aimed at. */
export interface ActionTarget {
  channel: string;
  userLogin: string;
  userId: string | null;
  messageId?: string;
}

export const SEVERITY_RANK: Record<Severity, number> = { low: 0, medium: 1, high: 2 };
