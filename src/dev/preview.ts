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
  ToxicityInfo,
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
  fixationCount: 8,
  fixationWindowMins: 10,
  toxicityPercent: 85,
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
const ch = (name: string, id: string, color: string, extra: Partial<Channel> = {}): Channel => ({
  name, muted: false, broadcasterId: id, isMod: true, alertLevel: "default", sound: true, color, shoutoutMessage: "", ...extra,
});
let channels: Channel[] = [
  ch("pixelqueen", "10", "#4dabf7", { shoutoutMessage: "!so @{raider}" }),
  ch("speedrunsam", "11", "#ff8787", { alertLevel: "high" }),
  ch("kitdy", "12", "#69db7c", { alertLevel: "off", muted: true }),
  ch("panda", "14", "#ffd43b"),
  ch("oldfriend", "13", "#b197fc", { isMod: false }),
];
const moderated: ModChannelOption[] = [
  { login: "modmaria", name: "ModMaria", added: false, own: true },
  { login: "kitdy", name: "CozyCafe", added: true, own: false },
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
  alsoPatterns: [],
  addressedToStreamer: false,
  notToOtherChatters: false,
  exceptions: [],
  skipSubscribers: false,
  skipAccountsOlderThanDays: 0,
  mode: "active",
  wellbeing: false,
  pack: null,
  alert: "default",
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
  r({ id: 20, name: "Stalking / location", kind: "keyword", severity: "high", pack: "Parasocial", mode: "test", patterns: ["i know where you live", "your address", "saw your car"] }),
  r({ id: 21, name: "Romantic / possessive", kind: "keyword", severity: "medium", pack: "Parasocial", mode: "test", addressedToStreamer: true, patterns: ["marry me", "be my girlfriend", "my wife", "you're mine"] }),
  r({ id: 22, name: "Personal probing", kind: "keyword", severity: "medium", pack: "Parasocial", mode: "test", addressedToStreamer: true, patterns: ["are you single", "where do you live"] }),
  r({ id: 23, name: "Fixation on the streamer", kind: "fixation", severity: "low", pack: "Parasocial", mode: "test" }),
  r({ id: 24, name: "Explicit requests", kind: "keyword", severity: "high", pack: "Sexual content", mode: "test", patterns: ["send nudes", "feet pics", "onlyfans"] }),
  r({ id: 25, name: "Camera / pose requests", kind: "keyword", severity: "medium", pack: "Sexual content", mode: "test", patterns: ["show us your", "turn around"], alsoPatterns: ["feet", "body"] }),
  r({ id: 26, name: "Innuendo", kind: "keyword", severity: "low", pack: "Sexual content", mode: "test", enabled: false, patterns: ["daddy", "step on me"] }),
  r({ id: 27, name: "Self-harm statements", kind: "keyword", severity: "medium", pack: "Wellbeing", mode: "active", wellbeing: true, exemptPrivileged: false, patterns: ["i want to die", "kms"] }),
  r({ id: 28, name: "Telling someone to die", kind: "keyword", severity: "high", pack: "Harassment", mode: "active", patterns: ["kys", "kill yourself", "go die"] }),
  r({ id: 31, name: "Toxic messages (AI)", kind: "toxicity", severity: "medium", pack: "On-device AI", mode: "test" }),
];

const hit = (ruleId: number, display: string, text: string, terms: string[], ago: number, channel = "pixelqueen") => ({
  ruleId, channel, userLogin: display.toLowerCase(), displayName: display, text, terms, reason: null, timestamp: now - ago,
});
const ruleStats = [
  { ruleId: 1, flags: 24, actioned: 23, reviewed: 1, dismissed: 0, open: 0, lastFired: now - 2 * 3600_000, patterns: [{ pattern: "free followers", flags: 15, actioned: 15, dismissed: 0 }, { pattern: "cheap viewers", flags: 9, actioned: 8, dismissed: 0 }], testHits: 0, testSamples: [] },
  { ruleId: 2, flags: 14, actioned: 3, reviewed: 1, dismissed: 10, open: 0, lastFired: now - 3600_000, patterns: [{ pattern: "the ending", flags: 11, actioned: 1, dismissed: 9 }, { pattern: "spoiler", flags: 3, actioned: 2, dismissed: 1 }], testHits: 0, testSamples: [] },
  { ruleId: 3, flags: 6, actioned: 4, reviewed: 1, dismissed: 1, open: 0, lastFired: now - 5 * DAY, patterns: [{ pattern: "grow-fast.xyz", flags: 4, actioned: 4, dismissed: 0 }], testHits: 0, testSamples: [] },
  { ruleId: 28, flags: 3, actioned: 3, reviewed: 0, dismissed: 0, open: 0, lastFired: now - 2 * DAY, patterns: [{ pattern: "kys", flags: 3, actioned: 3, dismissed: 0 }], testHits: 0, testSamples: [] },
  { ruleId: 21, flags: 0, actioned: 0, reviewed: 0, dismissed: 0, open: 0, lastFired: null, patterns: [], testHits: 7, testSamples: [
    hit(21, "lonely_larry", "pixelqueen marry me pls 🥺", ["marry me"], 40 * 60_000),
    hit(21, "Mika", "you're mine forever queen", ["you're mine"], 3 * 3600_000),
    hit(21, "NightOwl", "@pixelqueen my wife would love this game", ["my wife"], 9 * 3600_000),
  ] },
  { ruleId: 25, flags: 0, actioned: 0, reviewed: 0, dismissed: 0, open: 0, lastFired: null, patterns: [], testHits: 2, testSamples: [
    hit(25, "creepy_carl", "show us your feet", ["show us your", "feet"], 2 * DAY, "kitdy"),
  ] },
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
  evasionCheck: true,
  pileOnEnabled: true,
  pileOnMinUsers: 4,
  pileOnWindowSecs: 120,
  toxicityEnabled: true,
};
let toxicity: ToxicityInfo = { status: { state: "ready" }, scored: 1842, skipped: 0 };
const ctx = (name: string, text: string, ago: number) => ({ displayName: name, text, timestamp: now - ago });
let flags: Flag[] = [
  {
    id: 8, channel: "kitdy", messageId: "wb1", userId: "u8", userLogin: "quiet_quinn", displayName: "quiet_quinn",
    text: "honestly some days i want to die lol", severity: "medium",
    matches: [{ ruleId: 27, ruleName: "Self-harm statements", severity: "medium", terms: ["i want to die"], wellbeing: true }],
    context: [], contextAfter: [], timestamp: now - 10_000, status: "open", resolution: null,
    kind: "rule", externalId: null, meta: {},
  },
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
    id: 1, channel: "kitdy", messageId: "m1", userId: "u1", userLogin: "x", displayName: "OldFlag", text: "best viewers here",
    severity: "high", matches: [{ ruleId: 1, ruleName: "Follower/viewer bot spam", severity: "high", terms: ["best viewers"] }],
    context: [], contextAfter: [], timestamp: now - 3600_000, status: "reviewed", resolution: "Timed out 10m",
    kind: "rule", externalId: null, meta: {},
  },
];
flags.find((f) => f.id === 4)!.meta = { accountCreatedAt: now - 2 * DAY };
flags.unshift(
  {
    id: 7, channel: "kitdy", messageId: "am1", userId: "u7", userLogin: "grumpy_gus", displayName: "Grumpy_Gus",
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
  {
    id: 9, channel: "kitdy", messageId: "ev1", userId: "u9", userLogin: "troll_guy2", displayName: "troll_guy2",
    text: "lol you can't get rid of me", severity: "high",
    matches: [{ ruleId: 0, ruleName: "Possible ban evasion", severity: "high", terms: [], reason: "Name looks like troll_guy, banned here 2d ago: the same name with different numbers or lookalike letters" }],
    context: [ctx("Mika", "finally a quiet stream", 50_000)], contextAfter: [], timestamp: now - 45_000, status: "open", resolution: null,
    kind: "evasion", externalId: null, meta: { evasionOf: "troll_guy", evasionChannel: "kitdy", bannedAt: now - 2 * DAY, accountCreatedAt: now - 3 * 3600_000, firstMessage: true },
  },
  {
    id: 10, channel: "speedrunsam", messageId: "tx1", userId: "u10", userLogin: "salty_sid", displayName: "salty_sid",
    text: "you are honestly the most useless runner i have ever watched", severity: "medium",
    matches: [{ ruleId: 31, ruleName: "Toxic messages (AI)", severity: "medium", terms: [], reason: "Toxicity model: 94% toxic" }],
    context: [], contextAfter: [], timestamp: now - 75_000, status: "open", resolution: null,
    kind: "rule", externalId: null, meta: { toxicity: { toxic: 0.94, severe: 0.02 } },
  },
);
let sanctions: ModAction[] = [
  { id: 100, channel: "kitdy", targetLogin: "hasty_hal", action: "timeout", durationSecs: 600, reason: "", ok: true, error: null, timestamp: now - 25 * 60_000, byApp: false, targetUserId: "u3" },
  { id: 101, channel: "speedrunsam", targetLogin: "hasty_hal", action: "warn", durationSecs: null, reason: "caps", ok: true, error: null, timestamp: now - 3 * DAY, byApp: true, targetUserId: "u3" },
  { id: 102, channel: "kitdy", targetLogin: "promo_bot_77", action: "ban", durationSecs: null, reason: "", ok: true, error: null, timestamp: now - 15 * 60_000, byApp: false, targetUserId: "u4" },
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
  {
    id: 2, kind: "pileOn", key: "new_viewer_nina", channels: ["pixelqueen"],
    members: [
      member("edgy_ed", "pixelqueen", "@new_viewer_nina nobody asked", 70_000, false),
      member("ratio_king", "pixelqueen", "@new_viewer_nina ratio + L + bozo", 62_000, false),
      member("sal_t", "pixelqueen", "@new_viewer_nina stfu", 50_000, false),
    ],
    firstSeen: now - 70_000, lastSeen: now - 50_000, status: "open", severity: "medium",
    detail: "5 chatters went after @new_viewer_nina within 2 min; 3 of them were hostile.",
  },
  {
    id: 3, kind: "raidRaiders", key: "h8_squad_tv", channels: ["speedrunsam"],
    members: [
      member("h8bot_101", "speedrunsam", "this streamer is trash", 25_000),
      member("h8bot_102", "speedrunsam", "trash stream", 24_000),
      member("h8bot_103", "speedrunsam", "LOL", 23_000),
      member("fresh_acct_x", "speedrunsam", "hi", 22_000),
    ],
    firstSeen: now - 25_000, lastSeen: now - 10_000, status: "open", severity: "high",
    detail: "Arrived with h8_squad_tv's raid. 7 of 9 raiders checked have accounts under a week old",
  },
];

const risk = (level: "low" | "medium" | "high", reasons: [number, string][], extra: Partial<NonNullable<Raid["risk"]>> = {}): NonNullable<Raid["risk"]> => ({
  level, score: reasons.reduce((a, [w]) => a + w, 0), reasons: reasons.map(([weight, text]) => ({ weight, text })), done: true,
  raiderCreatedAt: null, raiderFollowers: null, priorRaids: 0, chatters: 0, newAccounts: 0, knownAges: 0, firstTimers: 0,
  flagged: 0, similarNames: 0, waveId: null, ...extra,
});
let raids: Raid[] = [
  { id: 3, channel: "speedrunsam", fromLogin: "h8_squad_tv", fromDisplayName: "h8_squad_tv", viewers: 40, timestamp: now - 25_000, acknowledged: false, fromUserId: null, shoutoutAt: null,
    risk: risk("high", [[4, "7 of 9 raiders checked have accounts under a week old"], [3, "The raiding channel is only 2 days old"], [3, "3 raiders have near-identical names"], [2, "The raiding channel has only 3 followers"]], { done: false, chatters: 11, newAccounts: 7, knownAges: 9, similarNames: 3, waveId: 3 }) },
  { id: 1, channel: "pixelqueen", fromLogin: "bigstreamer", fromDisplayName: "BigStreamer", viewers: 2431, timestamp: now - 30 * 60_000, acknowledged: true, fromUserId: null, shoutoutAt: null,
    risk: risk("low", [[-2, "They've raided your channels 4 times before"], [-1, "The raiding channel is 6 years old"], [-1, "The raiding channel has 182000 followers"]], { chatters: 48, newAccounts: 1, knownAges: 40 }) },
  { id: 2, channel: "kitdy", fromLogin: "smallfriend", fromDisplayName: "SmallFriend", viewers: 12, timestamp: now - 2 * 3600_000, acknowledged: true, fromUserId: null, shoutoutAt: null },
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
          channels, rules, settings, flags, raids, raidModes, auth, waves, sanctions, toxicity,
          connection: { status: "connected", joined: ["pixelqueen", "speedrunsam", "kitdy"] },
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
        channels = [...channels, ch(args.name, "99", "#3bc9db")];
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
      case "set_channel_alerts":
        channels = channels.map((c) => (c.name === args.name ? { ...c, alertLevel: args.level, sound: args.sound, muted: args.level === "off" } : c));
        return channels;
      case "set_channel_color":
        channels = channels.map((c) => (c.name === args.name ? { ...c, color: args.color } : c));
        return channels;
      case "set_shoutout_message":
        channels = channels.map((c) => (c.name === args.name ? { ...c, shoutoutMessage: args.message } : c));
        return channels;
      case "shoutout": {
        const r = raids.find((x) => x.id === args.raidId)!;
        const c = channels.find((x) => x.name === r.channel)!;
        if (r.channel === "kitdy") return { native: "The broadcaster is not streaming live.", messageSent: null, messageError: null, raid: r };
        const updated = { ...r, shoutoutAt: Date.now() };
        raids = raids.map((x) => (x.id === r.id ? updated : x));
        return { native: null, messageSent: c.shoutoutMessage ? c.shoutoutMessage.replace("{raider}", r.fromLogin) : null, messageError: null, raid: updated };
      }
      case "get_mod_log":
        return [
          { id: 3, channel: "pixelqueen", moderator: "ModMaria", action: "timeout", targetLogin: "promo_bot_77", summary: "timed out promo_bot_77 for 10m (spam)", timestamp: now - 2 * 60_000 },
          { id: 2, channel: "panda", moderator: "NightBot", action: "slow", targetLogin: null, summary: "turned on slow mode (30s)", timestamp: now - 15 * 60_000 },
          { id: 1, channel: "kitdy", moderator: "kitdy_mod", action: "ban", targetLogin: "hasty_hal", summary: "banned hasty_hal (ban evasion)", timestamp: now - 40 * 60_000 },
        ];
      case "get_activity":
        return [
          { id: 5, channel: "pixelqueen", kind: "giftbomb", userLogin: "generous_gary", displayName: "generous_gary", text: "generous_gary is gifting 5 Tier 1 Subs to pixelqueen's community!", timestamp: now - 60_000 },
          { id: 4, channel: "panda", kind: "bits", userLogin: "cheery", displayName: "Cheery", text: "cheered 100 bits “Cheer100 gg”", timestamp: now - 4 * 60_000 },
          { id: 3, channel: "speedrunsam", kind: "resub", userLogin: "fan", displayName: "Fan", text: "Fan subscribed at Tier 1. They've subscribed for 5 months! “love the runs”", timestamp: now - 9 * 60_000 },
          { id: 2, channel: "pixelqueen", kind: "follow", userLogin: "newbie", displayName: "Newbie", text: "followed", timestamp: now - 12 * 60_000 },
          { id: 1, channel: "kitdy", kind: "announcement", userLogin: "kitdy", displayName: "kitdy", text: "Announcement: giveaway at 9pm!", timestamp: now - 30 * 60_000 },
        ];
      case "set_rule_channel_alert":
        rules = rules.map((x) =>
          x.id === args.ruleId ? { ...x, channelAlerts: { ...(x.channelAlerts ?? {}), [args.channel]: args.alert } } : x,
        );
        return rules;
      case "set_pack_channel_alert":
        rules = rules.map((x) =>
          args.pack === "*" || (x.pack ?? null) === args.pack
            ? { ...x, channelAlerts: { ...(x.channelAlerts ?? {}), [args.channel]: args.alert } }
            : x,
        );
        return rules;
      case "rule_stats":
        return ruleStats;
      case "restore_default_packs":
        return rules;
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
      case "get_toxicity_status":
        return toxicity;
      case "export_rules":
        return JSON.stringify(
          { format: "twitch-mod-pro/rules", version: 1, name: args.name, description: args.description, exportedAt: Date.now(), appVersion: "0.1.0", rules: rules.filter((x) => args.ids.includes(x.id)).map((x) => ({ ...x, id: 0, channels: [] })) },
          null,
          2,
        );
      case "plugin:dialog|save":
        return "/Users/you/Downloads/rules.json";
      case "plugin:dialog|open":
        return "/Users/you/Downloads/shared-rules.json";
      case "save_rule_file":
        return null;
      case "read_rule_file":
        return JSON.stringify({ format: "twitch-mod-pro/rules", version: 1, name: "Kitdy's mod rules", rules: [] });
      case "preview_rule_import":
        return {
          name: "Kitdy's mod rules", description: "What we use on cozy streams", exportedAt: now - 3 * DAY,
          items: [
            { index: 0, rule: r({ id: 0, name: "Cozy-stream negativity", kind: "keyword", severity: "low", patterns: ["boring", "mid"] }), status: "new", existingId: null, error: null },
            { index: 1, rule: r({ id: 0, name: "Spoilers", kind: "keyword", severity: "medium", patterns: ["ending"] }), status: "conflict", existingId: 2, error: null },
            { index: 2, rule: r({ id: 0, name: "Links", kind: "links", severity: "medium" }), status: "identical", existingId: 3, error: null },
            { index: 3, rule: r({ id: 0, name: "Broken regex", kind: "regex", severity: "low", patterns: ["(oops"] }), status: "invalid", existingId: null, error: "Invalid pattern `(oops`: unclosed group" },
          ],
        };
      case "import_rules":
        return rules;
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
