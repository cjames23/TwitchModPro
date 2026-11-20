export type Severity = "low" | "medium" | "high";
export type RuleKind = "keyword" | "regex";
export type FlagStatus = "open" | "reviewed" | "dismissed";
export type ConnStatus = "connecting" | "connected" | "disconnected";

export interface Channel {
  name: string;
  muted: boolean;
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
}

export interface RuleMatch {
  ruleId: number;
  ruleName: string;
  severity: Severity;
  terms: string[];
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
  userLogin: string;
  displayName: string;
  text: string;
  severity: Severity;
  matches: RuleMatch[];
  context: ContextLine[];
  timestamp: number;
  status: FlagStatus;
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

export interface Settings {
  notifyFlags: boolean;
  notifyMinSeverity: Severity;
  notifyRaids: boolean;
  raidMinViewers: number;
  playSounds: boolean;
}

export interface ConnectionInfo {
  status: ConnStatus;
  joined: string[];
}

export interface LiveMessage {
  id: string;
  channel: string;
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
  timestamp: number;
  flagSeverity: Severity | null;
}

export interface Snapshot {
  channels: Channel[];
  rules: Rule[];
  settings: Settings;
  flags: Flag[];
  raids: Raid[];
  connection: ConnectionInfo;
}

export const SEVERITY_RANK: Record<Severity, number> = { low: 0, medium: 1, high: 2 };
