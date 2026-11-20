import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  AuthState,
  Channel,
  ConnectionInfo,
  ContextLine,
  Flag,
  LiveMessage,
  ModAction,
  ModChannelOption,
  Raid,
  RaidModes,
  Rule,
  RuleMatch,
  RuleStats,
  Settings,
  Snapshot,
  UserHistory,
  Wave,
  BulkResult,
  ChannelResult,
} from "./types";

export const api = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),

  // auth
  getAuth: () => invoke<AuthState>("get_auth"),
  setClientId: (clientId: string) => invoke<AuthState>("set_client_id", { clientId }),
  startLogin: () => invoke<AuthState>("start_login"),
  cancelLogin: () => invoke<AuthState>("cancel_login"),
  logout: () => invoke<AuthState>("logout"),

  // channels
  moderatedChannels: (refresh: boolean) => invoke<ModChannelOption[]>("list_moderated_channels", { refresh }),
  addChannel: (name: string) => invoke<Channel[]>("add_channel", { name }),
  removeChannel: (name: string) => invoke<Channel[]>("remove_channel", { name }),
  setChannelMuted: (name: string, muted: boolean) => invoke<Channel[]>("set_channel_muted", { name, muted }),
  setRaidMode: (channel: string, minutes: number) => invoke<RaidModes>("set_raid_mode", { channel, minutes }),

  // rules
  saveRule: (rule: Rule) => invoke<Rule[]>("save_rule", { rule }),
  deleteRule: (id: number) => invoke<Rule[]>("delete_rule", { id }),
  testRule: (rule: Rule, text: string, username: string, firstMessage: boolean) =>
    invoke<RuleMatch[]>("test_rule", { rule, text, username, firstMessage }),
  ruleStats: (days: number) => invoke<RuleStats[]>("rule_stats", { days }),
  restoreDefaultPacks: () => invoke<Rule[]>("restore_default_packs"),

  // flags, raids, settings
  setFlagStatus: (id: number, status: string) => invoke<void>("set_flag_status", { id, status }),
  clearClosedFlags: () => invoke<Flag[]>("clear_closed_flags"),
  ackRaids: () => invoke<void>("ack_raids"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),

  // moderation
  userHistory: (channel: string, login: string) => invoke<UserHistory>("get_user_history", { channel, login }),
  deleteMessage: (channel: string, userLogin: string, messageId: string) =>
    invoke<ModAction>("delete_message", { channel, userLogin, messageId }),
  timeout: (channel: string, userLogin: string, userId: string | null, seconds: number, reason: string) =>
    invoke<ModAction>("timeout_user", { channel, userLogin, userId, seconds, reason }),
  ban: (channel: string, userLogin: string, userId: string | null, reason: string) =>
    invoke<ModAction>("ban_user", { channel, userLogin, userId, reason }),
  unban: (channel: string, userLogin: string, userId: string | null) =>
    invoke<ModAction>("unban_user", { channel, userLogin, userId }),
  warn: (channel: string, userLogin: string, userId: string | null, reason: string) =>
    invoke<ModAction>("warn_user", { channel, userLogin, userId, reason }),
  banEverywhere: (userLogin: string, userId: string | null, reason: string) =>
    invoke<ChannelResult[]>("ban_everywhere", { userLogin, userId, reason }),
  accountAges: (logins: string[]) => invoke<Record<string, number>>("get_account_ages", { logins }),

  // spam waves
  waveAction: (id: number, action: "ban" | "timeout", seconds: number | null, reason: string, exclude: string[]) =>
    invoke<BulkResult>("wave_action", { id, action, seconds, reason, exclude }),
  waveUndo: (id: number) => invoke<BulkResult>("wave_undo", { id }),
  dismissWave: (id: number) => invoke<Wave>("dismiss_wave", { id }),

  // Twitch inbox
  automodDecide: (flagId: number, allow: boolean) => invoke<void>("automod_decide", { flagId, allow }),
  unbanRequestDecide: (flagId: number, approve: boolean, text: string) =>
    invoke<void>("unban_request_decide", { flagId, approve, text }),

  open: (url: string) => openUrl(url),
  openChat: (channel: string) => openUrl(`https://www.twitch.tv/popout/${channel}/chat`),
  openChannel: (channel: string) => openUrl(`https://www.twitch.tv/${channel}`),
};

export interface Handlers {
  onFlag: (e: { flag: Flag; alert: boolean }) => void;
  onFlagContext: (e: { id: number; line: ContextLine }) => void;
  onFlagsResolved: (e: { ids: number[]; resolution: string }) => void;
  onRaid: (e: { raid: Raid; alert: boolean }) => void;
  onRaidMode: (modes: RaidModes) => void;
  onChat: (batch: LiveMessage[]) => void;
  onConnection: (info: ConnectionInfo) => void;
  onChannels: (channels: Channel[]) => void;
  onAuth: (auth: AuthState) => void;
  onModAction: (a: ModAction) => void;
  onNotice: (e: { channel: string | null; text: string }) => void;
  onWave: (e: { wave: Wave; new: boolean }) => void;
  onAccountAges: (ages: Record<string, number>) => void;
  onMessageDeleted: (e: { channel: string; messageId: string }) => void;
  onInboxStatus: (e: { connected: boolean }) => void;
}

export async function subscribe(h: Handlers): Promise<UnlistenFn> {
  const offs = await Promise.all([
    listen<{ flag: Flag; alert: boolean }>("flag", (e) => h.onFlag(e.payload)),
    listen<{ id: number; line: ContextLine }>("flag-context", (e) => h.onFlagContext(e.payload)),
    listen<{ ids: number[]; resolution: string }>("flags-resolved", (e) => h.onFlagsResolved(e.payload)),
    listen<{ raid: Raid; alert: boolean }>("raid", (e) => h.onRaid(e.payload)),
    listen<RaidModes>("raid-mode", (e) => h.onRaidMode(e.payload)),
    listen<LiveMessage[]>("chat-batch", (e) => h.onChat(e.payload)),
    listen<ConnectionInfo>("connection", (e) => h.onConnection(e.payload)),
    listen<Channel[]>("channels", (e) => h.onChannels(e.payload)),
    listen<AuthState>("auth", (e) => h.onAuth(e.payload)),
    listen<ModAction>("mod-action", (e) => h.onModAction(e.payload)),
    listen<{ channel: string | null; text: string }>("notice", (e) => h.onNotice(e.payload)),
    listen<{ wave: Wave; new: boolean }>("wave", (e) => h.onWave(e.payload)),
    listen<Record<string, number>>("account-ages", (e) => h.onAccountAges(e.payload)),
    listen<{ channel: string; messageId: string }>("message-deleted", (e) => h.onMessageDeleted(e.payload)),
    listen<{ connected: boolean }>("inbox-status", (e) => h.onInboxStatus(e.payload)),
  ]);
  return () => offs.forEach((off) => off());
}
