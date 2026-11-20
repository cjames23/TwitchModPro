import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Channel, ConnectionInfo, Flag, LiveMessage, Raid, Rule, Settings, Snapshot } from "./types";

export const api = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  addChannel: (name: string) => invoke<Channel[]>("add_channel", { name }),
  removeChannel: (name: string) => invoke<Channel[]>("remove_channel", { name }),
  setChannelMuted: (name: string, muted: boolean) => invoke<Channel[]>("set_channel_muted", { name, muted }),
  saveRule: (rule: Rule) => invoke<Rule[]>("save_rule", { rule }),
  deleteRule: (id: number) => invoke<Rule[]>("delete_rule", { id }),
  testRule: (rule: Rule, text: string, channel?: string) =>
    invoke<string[]>("test_rule", { rule, text, channel: channel ?? null }),
  setFlagStatus: (id: number, status: string) => invoke<void>("set_flag_status", { id, status }),
  clearClosedFlags: () => invoke<Flag[]>("clear_closed_flags"),
  ackRaids: () => invoke<void>("ack_raids"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  openChat: (channel: string) => openUrl(`https://www.twitch.tv/popout/${channel}/chat`),
  openChannel: (channel: string) => openUrl(`https://www.twitch.tv/${channel}`),
};

export interface Handlers {
  onFlag: (e: { flag: Flag; alert: boolean }) => void;
  onRaid: (e: { raid: Raid; alert: boolean }) => void;
  onChat: (batch: LiveMessage[]) => void;
  onConnection: (info: ConnectionInfo) => void;
  onNotice: (e: { channel: string | null; text: string }) => void;
}

export async function subscribe(h: Handlers): Promise<UnlistenFn> {
  const offs = await Promise.all([
    listen<{ flag: Flag; alert: boolean }>("flag", (e) => h.onFlag(e.payload)),
    listen<{ raid: Raid; alert: boolean }>("raid", (e) => h.onRaid(e.payload)),
    listen<LiveMessage[]>("chat-batch", (e) => h.onChat(e.payload)),
    listen<ConnectionInfo>("connection", (e) => h.onConnection(e.payload)),
    listen<{ channel: string | null; text: string }>("notice", (e) => h.onNotice(e.payload)),
  ]);
  return () => offs.forEach((off) => off());
}
