// Dev-only: run the UI in a normal browser with a fake backend and sample data.
// Open http://localhost:1420/preview.html while `npm run dev` (or `npm run tauri dev`) is running.
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { Channel, Flag, LiveMessage, Raid, Rule, Settings } from "../types";

const now = Date.now();
let channels: Channel[] = [
  { name: "pixelqueen", muted: false },
  { name: "speedrunsam", muted: false },
  { name: "cozycafe", muted: true },
];
let rules: Rule[] = [
  { id: 1, name: "Follower/viewer bot spam", enabled: true, kind: "keyword", patterns: ["free followers", "cheap viewers", "best viewers"], wholeWord: true, caseSensitive: false, channels: [], severity: "high", exemptPrivileged: true },
  { id: 2, name: "Spoilers", enabled: true, kind: "keyword", patterns: ["spoiler", "the ending"], wholeWord: true, caseSensitive: false, channels: ["speedrunsam"], severity: "medium", exemptPrivileged: true },
  { id: 3, name: "Link shorteners", enabled: false, kind: "regex", patterns: ["bit\\.ly/\\w+"], wholeWord: false, caseSensitive: false, channels: [], severity: "low", exemptPrivileged: false },
];
let settings: Settings = { notifyFlags: true, notifyMinSeverity: "medium", notifyRaids: true, raidMinViewers: 0, playSounds: false };
let flags: Flag[] = [
  { id: 3, channel: "pixelqueen", messageId: "m3", userLogin: "promo_bot_77", displayName: "promo_bot_77", text: "Want to grow? Get FREE followers and cheap viewers at our site", severity: "high", matches: [{ ruleId: 1, ruleName: "Follower/viewer bot spam", severity: "high", terms: ["free followers", "cheap viewers"] }], context: [{ displayName: "Mika", text: "that boss fight was insane", timestamp: now - 70_000 }, { displayName: "lurker42", text: "gg", timestamp: now - 65_000 }], timestamp: now - 60_000, status: "open" },
  { id: 2, channel: "speedrunsam", messageId: "m2", userLogin: "hasty_hal", displayName: "Hasty_Hal", text: "lol wait till you see the ending, huge spoiler incoming", severity: "medium", matches: [{ ruleId: 2, ruleName: "Spoilers", severity: "medium", terms: ["the ending", "spoiler"] }], context: [], timestamp: now - 5 * 60_000, status: "open" },
  { id: 1, channel: "cozycafe", messageId: "m1", userLogin: "x", displayName: "OldFlag", text: "best viewers here", severity: "high", matches: [{ ruleId: 1, ruleName: "Follower/viewer bot spam", severity: "high", terms: ["best viewers"] }], context: [], timestamp: now - 3600_000, status: "reviewed" },
];
let raids: Raid[] = [
  { id: 1, channel: "pixelqueen", fromLogin: "bigstreamer", fromDisplayName: "BigStreamer", viewers: 2431, timestamp: now - 30_000, acknowledged: false },
  { id: 2, channel: "cozycafe", fromLogin: "smallfriend", fromDisplayName: "SmallFriend", viewers: 12, timestamp: now - 2 * 3600_000, acknowledged: true },
];

mockIPC(
  (cmd, args: any) => {
    switch (cmd) {
      case "get_snapshot":
        return { channels, rules, settings, flags, raids, connection: { status: "connected", joined: ["pixelqueen", "speedrunsam"] } };
      case "add_channel":
        channels = [...channels, { name: String(args.name).toLowerCase(), muted: false }];
        return channels;
      case "remove_channel":
        channels = channels.filter((c) => c.name !== args.name);
        return channels;
      case "set_channel_muted":
        channels = channels.map((c) => (c.name === args.name ? { ...c, muted: args.muted } : c));
        return channels;
      case "save_rule": {
        const r = args.rule as Rule;
        rules = r.id ? rules.map((x) => (x.id === r.id ? r : x)) : [...rules, { ...r, id: rules.length + 10 }];
        return rules;
      }
      case "delete_rule":
        rules = rules.filter((r) => r.id !== args.id);
        return rules;
      case "test_rule": {
        const r = args.rule as Rule;
        const text = String(args.text).toLowerCase();
        return r.patterns.filter((p) => text.includes(p.toLowerCase()));
      }
      case "set_flag_status":
        flags = flags.map((f) => (f.id === args.id ? { ...f, status: args.status } : f));
        return null;
      case "clear_closed_flags":
        flags = flags.filter((f) => f.status === "open");
        return flags;
      case "ack_raids":
        raids = raids.map((r) => ({ ...r, acknowledged: true }));
        return null;
      case "save_settings":
        settings = args.settings;
        return settings;
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
  const batch: LiveMessage[] = channels.slice(0, 3).map((c, i) => ({
    id: `live-${n}-${i}`,
    channel: c.name,
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
    timestamp: Date.now(),
    flagSeverity: (n + i) % 9 === 0 ? "medium" : null,
  }));
  n++;
  emit("chat-batch", batch);
}, 700);

import("../main");
