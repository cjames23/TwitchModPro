// Dev-only: run the UI in a normal browser with a fake backend and sample data.
// Open http://localhost:1420/preview.html while `npm run dev` (or `npm run tauri dev`) is running.
// Add ?view=login to see the login screen, or ?view=code for the "approve on Twitch" step.
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type {
  AuthState,
  Channel,
  Flag,
  LiveMessage,
  ModAction,
  ModChannelOption,
  Raid,
  RaidModes,
  Rule,
  RuleParams,
  Settings,
  Wave,
} from "../types";

const view = new URLSearchParams(location.search).get("view");
const now = Date.now();
const params: RuleParams = {
  minLength: 12,
  capsPercent: 75,
  maxRepeatedChars: 12,
  maxEmotes: 12,
  repeatCount: 3,
  repeatWindowSecs: 60,
  maxAccountAgeDays: 7,
};
const DAY = 86_400_000;

let auth: AuthState = {
  clientIdSet: view !== "login",
  clientIdFromBuild: false,
  account: view ? null : { userId: "1", login: "modmaria", scopes: [] },
  login: view === "code" ? { userCode: "WXYZ-2345", verificationUri: "https://www.twitch.tv/activate", expiresAt: now + 29 * 60_000 } : null,
  error: null,
  checking: false,
};
let channels: Channel[] = [
  { name: "pixelqueen", muted: false, broadcasterId: "10", isMod: true },
  { name: "speedrunsam", muted: false, broadcasterId: "11", isMod: true },
  { name: "cozycafe", muted: true, broadcasterId: "12", isMod: true },
  { name: "oldfriend", muted: false, broadcasterId: "13", isMod: false },
];
const moderated: ModChannelOption[] = [
  { login: "modmaria", name: "ModMaria", added: false, own: true },
  { login: "cozycafe", name: "CozyCafe", added: true, own: false },
  { login: "pixelqueen", name: "PixelQueen", added: true, own: false },
  { login: "retroroberto", name: "RetroRoberto", added: false, own: false },
  { login: "speedrunsam", name: "SpeedrunSam", added: true, own: false },
];
const r = (x: Partial<Rule> & Pick<Rule, "id" | "name" | "kind" | "severity">): Rule => ({
  enabled: true,
  patterns: [],
  wholeWord: true,
  caseSensitive: false,
  channels: [],
  exemptPrivileged: true,
  target: "message",
  normalize: true,
  activeDuring: "always",
  params,
  ...x,
});
let rules: Rule[] = [
  r({ id: 1, name: "Follower/viewer bot spam", kind: "keyword", severity: "high", patterns: ["free followers", "cheap viewers", "best viewers"] }),
  r({ id: 2, name: "Spoilers", kind: "keyword", severity: "medium", patterns: ["spoiler", "the ending"], channels: ["speedrunsam"] }),
  r({ id: 3, name: "Links", kind: "links", severity: "medium", patterns: ["twitch.tv", "youtube.com"] }),
  r({ id: 4, name: "Caps lock", kind: "caps", severity: "low" }),
  r({ id: 5, name: "Copy-paste spam", kind: "repeat", severity: "medium" }),
  r({ id: 6, name: "New chatters during raids", kind: "firstMessage", severity: "low", activeDuring: "raid" }),
  r({ id: 7, name: "Bot usernames", kind: "keyword", severity: "high", target: "username", wholeWord: false, patterns: ["hoss00312"] }),
];
let settings: Settings = {
  notifyFlags: true,
  notifyMinSeverity: "medium",
  notifyRaids: true,
  raidMinViewers: 0,
  playSounds: false,
  twitchClientId: "x",
  raidModeMinutes: 5,
  ladder: [
    { action: "warn" },
    { action: "timeout", durationSecs: 600 },
    { action: "timeout", durationSecs: 86400 },
    { action: "ban" },
  ],
  ladderLookbackDays: 30,
  waveMinAccounts: 5,
  waveWindowSecs: 60,
};
const ctx = (name: string, text: string, ago: number) => ({ displayName: name, text, timestamp: now - ago });
let flags: Flag[] = [
  {
    id: 4, channel: "pixelqueen", messageId: "m4", userId: "u4", userLogin: "promo_bot_77", displayName: "promo_bot_77",
    text: "Want to grow? Get FR33 F0LL0WERS and cheap viewers at grow-fast.xyz", severity: "high",
    matches: [
      { ruleId: 1, ruleName: "Follower/viewer bot spam", severity: "high", terms: ["FR33 F0LL0WERS", "cheap viewers"] },
      { ruleId: 3, ruleName: "Links", severity: "medium", terms: ["grow-fast.xyz"] },
    ],
    context: [ctx("Mika", "that boss fight was insane", 70_000), ctx("lurker42", "gg", 65_000)],
    contextAfter: [ctx("Mika", "ugh bots again", 55_000), ctx("NightOwl", "report it", 50_000)],
    timestamp: now - 60_000, status: "open", resolution: null,
    kind: "rule", externalId: null, meta: {},
  },
  {
    id: 3, channel: "speedrunsam", messageId: "m3", userId: "u3", userLogin: "hasty_hal", displayName: "Hasty_Hal",
    text: "THIS RUN IS GOING TO BE THE WORST EVER LOL", severity: "low",
    matches: [{ ruleId: 4, ruleName: "Caps lock", severity: "low", terms: [], reason: "100% capital letters" }],
    context: [], contextAfter: [], timestamp: now - 3 * 60_000, status: "open", resolution: null,
    kind: "rule", externalId: null, meta: {},
  },
  {
    id: 2, channel: "speedrunsam", messageId: "m2", userId: "u2", userLogin: "hasty_hal", displayName: "Hasty_Hal",
    text: "wait till you see the ending, huge spoiler incoming", severity: "medium",
    matches: [{ ruleId: 2, ruleName: "Spoilers", severity: "medium", terms: ["the ending", "spoiler"] }],
    context: [], contextAfter: [], timestamp: now - 5 * 60_000, status: "open", resolution: null,
    kind: "rule", externalId: null, meta: {},
  },
  {
    id: 1, channel: "cozycafe", messageId: "m1", userId: "u1", userLogin: "x", displayName: "OldFlag", text: "best viewers here",
    severity: "high", matches: [{ ruleId: 1, ruleName: "Follower/viewer bot spam", severity: "high", terms: ["best viewers"] }],
    context: [], contextAfter: [], timestamp: now - 3600_000, status: "reviewed", resolution: "Timed out 10m",
    kind: "rule", externalId: null, meta: {},
  },
];
flags[0].meta = { accountCreatedAt: now - 2 * DAY };
flags.unshift(
  {
    id: 7, channel: "cozycafe", messageId: "am1", userId: "u7", userLogin: "grumpy_gus", displayName: "Grumpy_Gus",
    text: "you are all absolute [held message text]", severity: "high",
    matches: [{ ruleId: 0, ruleName: "AutoMod", severity: "high", terms: [], reason: "AutoMod: aggression (level 4)" }],
    context: [], contextAfter: [], timestamp: now - 20_000, status: "open", resolution: null,
    kind: "automod", externalId: "am1", meta: { category: "aggression", level: 4 },
  },
  {
    id: 6, channel: "pixelqueen", messageId: "", userId: "u6", userLogin: "sorry_steve", displayName: "sorry_steve",
    text: "I was out of line last month, it won't happen again. Please let me back in.", severity: "medium",
    matches: [{ ruleId: 0, ruleName: "Unban request", severity: "medium", terms: [] }],
    context: [], contextAfter: [], timestamp: now - 40 * 60_000, status: "open", resolution: null,
    kind: "unbanRequest", externalId: "ur1", meta: {},
  },
  {
    id: 5, channel: "speedrunsam", messageId: "sm1", userId: "u5", userLogin: "totally_new_guy", displayName: "totally_new_guy",
    text: "hey sam remember me", severity: "high",
    matches: [{ ruleId: 0, ruleName: "Suspicious user", severity: "high", terms: [], reason: "Likely ban evader · Banned in 2 channels that share ban info" }],
    context: [], contextAfter: [], timestamp: now - 90_000, status: "open", resolution: null,
    kind: "suspicious", externalId: "sm1", meta: { banEvasion: "likely", sharedBans: 2, accountCreatedAt: now - 1 * DAY },
  },
);
let sanctions: ModAction[] = [
  { id: 100, channel: "cozycafe", targetLogin: "hasty_hal", action: "timeout", durationSecs: 600, reason: "", ok: true, error: null, timestamp: now - 25 * 60_000, byApp: false, targetUserId: "u3" },
  { id: 101, channel: "speedrunsam", targetLogin: "hasty_hal", action: "warn", durationSecs: null, reason: "caps", ok: true, error: null, timestamp: now - 3 * DAY, byApp: true, targetUserId: "u3" },
  { id: 102, channel: "cozycafe", targetLogin: "promo_bot_77", action: "ban", durationSecs: null, reason: "", ok: true, error: null, timestamp: now - 15 * 60_000, byApp: false, targetUserId: "u4" },
];
const member = (login: string, channel: string, text: string, ago: number, first = true) => ({
  login, displayName: login, userId: `id-${login}`, channel, messageId: `w-${login}`, text, timestamp: now - ago, firstMessage: first, outcome: null,
});
let waves: Wave[] = [
  {
    id: 1, kind: "sameMessage", key: "best viewers on cheapviewers", channels: ["pixelqueen", "speedrunsam"],
    members: [
      member("hoss00312_1", "pixelqueen", "Best viewers on cheapviewers dot ru 1", 40_000),
      member("hoss00312_2", "pixelqueen", "Best viewers on cheapviewers dot ru 2", 38_000),
      member("hoss00312_3", "speedrunsam", "Best viewers on cheapviewers dot ru 3", 36_000),
      member("xx_bot_xx9", "speedrunsam", "best viewers on cheapviewers dot ru", 30_000),
      member("lurker_bot_42", "pixelqueen", "Best viewers on cheapviewers dot ru!!", 25_000),
      member("follow4follow_", "speedrunsam", "Best viewers on cheapviewers dot ru 7", 20_000, false),
    ],
    firstSeen: now - 40_000, lastSeen: now - 20_000, status: "open", severity: "high",
  },
];

let raids: Raid[] = [
  { id: 1, channel: "pixelqueen", fromLogin: "bigstreamer", fromDisplayName: "BigStreamer", viewers: 2431, timestamp: now - 30_000, acknowledged: false },
  { id: 2, channel: "cozycafe", fromLogin: "smallfriend", fromDisplayName: "SmallFriend", viewers: 12, timestamp: now - 2 * 3600_000, acknowledged: true },
];
let raidModes: RaidModes = { pixelqueen: now + 4 * 60_000 + 30_000 };
const actions: ModAction[] = [];

function action(
  a: Omit<ModAction, "id" | "ok" | "error" | "timestamp" | "reason" | "byApp" | "targetUserId"> & { reason?: string },
): ModAction {
  const rec: ModAction = { id: 1000 + actions.length, ok: true, error: null, timestamp: Date.now(), reason: "", byApp: true, targetUserId: null, ...a };
  actions.unshift(rec);
  const resolution =
    a.action === "delete"
      ? "Message deleted"
      : a.action === "timeout"
        ? `Timed out ${a.durationSecs! / 60}m`
        : a.action === "ban"
          ? "Banned"
          : a.action === "warn"
            ? "Warned"
            : null;
  if (resolution) {
    const ids = flags.filter((f) => f.channel === a.channel && f.userLogin === a.targetLogin && f.status === "open").map((f) => f.id);
    flags = flags.map((f) => (ids.includes(f.id) ? { ...f, status: "reviewed", resolution } : f));
    emit("flags-resolved", { ids, resolution });
  }
  emit("mod-action", rec);
  return rec;
}

mockIPC(
  (cmd, args: any) => {
    switch (cmd) {
      case "get_snapshot":
        return {
          channels, rules, settings, flags, raids, raidModes, auth, waves, sanctions,
          connection: { status: "connected", joined: ["pixelqueen", "speedrunsam", "cozycafe"] },
        };
      case "set_client_id":
        auth = { ...auth, clientIdSet: !!args.clientId };
        return auth;
      case "start_login":
        auth = { ...auth, login: { userCode: "WXYZ-2345", verificationUri: "https://www.twitch.tv/activate", expiresAt: Date.now() + 30 * 60_000 } };
        return auth;
      case "cancel_login":
        auth = { ...auth, login: null };
        return auth;
      case "logout":
        auth = { ...auth, account: null };
        return auth;
      case "list_moderated_channels":
        return moderated.map((m) => ({ ...m, added: channels.some((c) => c.name === m.login) }));
      case "add_channel":
        if (!moderated.some((m) => m.login === args.name)) throw `You're not a moderator in #${args.name}, so it can't be added.`;
        channels = [...channels, { name: args.name, muted: false, broadcasterId: "99", isMod: true }];
        return channels;
      case "remove_channel":
        channels = channels.filter((c) => c.name !== args.name);
        return channels;
      case "set_channel_muted":
        channels = channels.map((c) => (c.name === args.name ? { ...c, muted: args.muted } : c));
        return channels;
      case "set_raid_mode":
        raidModes = { ...raidModes };
        if (args.minutes > 0) raidModes[args.channel] = Date.now() + args.minutes * 60_000;
        else delete raidModes[args.channel];
        return raidModes;
      case "save_rule": {
        const rule = args.rule as Rule;
        rules = rule.id ? rules.map((x) => (x.id === rule.id ? rule : x)) : [...rules, { ...rule, id: rules.length + 10 }];
        return rules;
      }
      case "delete_rule":
        rules = rules.filter((x) => x.id !== args.id);
        return rules;
      case "test_rule": {
        const rule = args.rule as Rule;
        const text = String(args.text);
        if (rule.kind === "caps") {
          const letters = text.replace(/[^a-z]/gi, "");
          const pct = letters.length ? Math.round((100 * letters.replace(/[^A-Z]/g, "").length) / letters.length) : 0;
          return letters.length >= rule.params.minLength && pct >= rule.params.capsPercent
            ? [{ ruleId: 0, ruleName: rule.name, severity: rule.severity, terms: [], reason: `${pct}% capital letters` }]
            : [];
        }
        const terms = rule.patterns.filter((p) => text.toLowerCase().includes(p.toLowerCase()));
        return terms.length ? [{ ruleId: 0, ruleName: rule.name, severity: rule.severity, terms }] : [];
      }
      case "set_flag_status":
        flags = flags.map((f) => (f.id === args.id ? { ...f, status: args.status } : f));
        return null;
      case "clear_closed_flags":
        flags = flags.filter((f) => f.status === "open");
        return flags;
      case "ack_raids":
        raids = raids.map((x) => ({ ...x, acknowledged: true }));
        return null;
      case "save_settings":
        settings = args.settings;
        return settings;
      case "get_user_history":
        return {
          channel: args.channel,
          login: args.login,
          displayName: flags.find((f) => f.userLogin === args.login)?.displayName ?? args.login,
          userId: "u",
          messages: [
            { id: "h3", text: "THIS RUN IS GOING TO BE THE WORST EVER LOL", timestamp: now - 3 * 60_000, flagSeverity: "low" },
            { id: "h2", text: "wait till you see the ending, huge spoiler incoming", timestamp: now - 5 * 60_000, flagSeverity: "medium" },
            { id: "h1", text: "hi chat", timestamp: now - 9 * 60_000, flagSeverity: null },
          ],
          flags: flags.filter((f) => f.userLogin === args.login && f.channel === args.channel),
          actions: [...actions, ...sanctions].filter((a) => a.targetLogin === args.login && a.channel === args.channel),
          otherChannels: sanctions.filter((a) => a.targetLogin === args.login && a.channel !== args.channel),
          accountCreatedAt: now - 400 * DAY,
        };
      case "get_account_ages":
        return { hasty_hal: now - 3 * 365 * DAY };
      case "warn_user":
        return action({ channel: args.channel, targetLogin: args.userLogin, action: "warn", durationSecs: null, reason: args.reason });
      case "ban_everywhere":
        return channels
          .filter((c) => c.isMod)
          .map((c) => (action({ channel: c.name, targetLogin: args.userLogin, action: "ban", durationSecs: null }), { channel: c.name, ok: true, error: null }));
      case "wave_action": {
        const w = waves.find((x) => x.id === args.id)!;
        const updated: Wave = {
          ...w,
          status: "actioned",
          members: w.members.map((m) =>
            args.exclude.includes(m.login) ? m : { ...m, outcome: args.action === "ban" ? "banned" : "timed out 10m" },
          ),
        };
        waves = waves.map((x) => (x.id === w.id ? updated : x));
        return { succeeded: updated.members.filter((m) => m.outcome).length, failed: [], wave: updated };
      }
      case "wave_undo": {
        const w = waves.find((x) => x.id === args.id)!;
        const updated: Wave = { ...w, status: "open", members: w.members.map((m) => (m.outcome ? { ...m, outcome: "unbanned" } : m)) };
        waves = waves.map((x) => (x.id === w.id ? updated : x));
        return { succeeded: w.members.filter((m) => m.outcome).length, failed: [], wave: updated };
      }
      case "dismiss_wave": {
        const w = { ...waves.find((x) => x.id === args.id)!, status: "dismissed" as const };
        waves = waves.map((x) => (x.id === w.id ? w : x));
        return w;
      }
      case "automod_decide":
      case "unban_request_decide": {
        const f = flags.find((x) => x.id === args.flagId)!;
        const ok = args.allow ?? args.approve;
        const resolution = f.kind === "automod" ? (ok ? "Approved" : "Denied") : ok ? "Unban approved" : "Unban denied";
        flags = flags.map((x) => (x.id === f.id ? { ...x, status: "reviewed", resolution } : x));
        emit("flags-resolved", { ids: [f.id], resolution });
        return null;
      }
      case "delete_message":
        return action({ channel: args.channel, targetLogin: args.userLogin, action: "delete", durationSecs: null });
      case "timeout_user":
        return action({ channel: args.channel, targetLogin: args.userLogin, action: "timeout", durationSecs: args.seconds, reason: args.reason });
      case "ban_user":
        if (args.userLogin === "hasty_hal") throw "The user specified in the user_id field is already banned.";
        return action({ channel: args.channel, targetLogin: args.userLogin, action: "ban", durationSecs: null, reason: args.reason });
      case "unban_user":
        return action({ channel: args.channel, targetLogin: args.userLogin, action: "unban", durationSecs: null });
    }
    return null;
  },
  { shouldMockEvents: true },
);

// Stream some fake chat so the Live chat tab has content.
const sample = ["hi chat", "W stream", "that was clean", "LUL", "gg", "how long is this run?", "first time here, hello!", "pog"];
const users = ["Mika", "lurker42", "NightOwl", "ZedZed", "kappa_kid"];
let n = 0;
setInterval(() => {
  const batch: LiveMessage[] = channels
    .filter((c) => c.isMod)
    .slice(0, 3)
    .map((c, i) => ({
      id: `live-${n}-${i}`,
      channel: c.name,
      roomId: c.broadcasterId,
      userId: `u${(n + i) % users.length}`,
      userLogin: users[(n + i) % users.length].toLowerCase(),
      displayName: users[(n + i) % users.length],
      color: ["#ff7f50", "#9acd32", "#1e90ff", "#ff69b4", "#daa520"][(n + i) % 5],
      text: sample[(n + i * 3) % sample.length],
      isAction: false,
      isMod: (n + i) % 7 === 0,
      isBroadcaster: false,
      isVip: (n + i) % 11 === 0,
      isSubscriber: true,
      isFirstMsg: (n + i) % 13 === 0,
      emoteCount: 0,
      timestamp: Date.now(),
      flagSeverity: (n + i) % 9 === 0 ? "medium" : null,
    }));
  n++;
  emit("chat-batch", batch);
}, 700);

import("../main");
