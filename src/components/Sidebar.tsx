import { useState } from "react";
import type { Account, AlertLevel, Channel, ConnectionInfo, RaidModes } from "../types";
import { channelColor } from "../util";
import { ChannelPicker } from "./ChannelPicker";
import { ChannelSettings } from "./ChannelSettings";

interface Props {
  account: Account;
  channels: Channel[];
  connection: ConnectionInfo;
  openFlagCounts: Record<string, number>;
  raidModes: RaidModes;
  raidModeMinutes: number;
  now: number;
  selected: string | null;
  onSelect: (name: string | null) => void;
  onAdd: (name: string) => Promise<void>;
  onRemove: (name: string) => void;
  onToggleMute: (name: string, muted: boolean) => void;
  onAlerts: (name: string, level: AlertLevel, sound: boolean) => void;
  onColor: (name: string, color: string | null) => void;
  onShoutoutMessage: (name: string, message: string) => void;
  onOpenRuleAlerts: (name: string) => void;
  onRaidMode: (name: string, minutes: number) => void;
  onLogout: () => void;
  inboxConnected: boolean | null;
}

function countdown(ms: number) {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function Sidebar(p: Props) {
  const [picking, setPicking] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [settingsFor, setSettingsFor] = useState<string | null>(null);

  const statusLabel = {
    connected: "Connected to Twitch chat",
    connecting: "Connecting…",
    disconnected: "Disconnected, retrying…",
  }[p.connection.status];

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark">◆</span> Mod Pro
      </div>

      <div className="add-wrap">
        <button className="primary add-btn" onClick={() => setPicking((v) => !v)}>
          + Add channel
        </button>
        {picking && <ChannelPicker onAdd={p.onAdd} onClose={() => setPicking(false)} />}
      </div>

      <div className="channel-list">
        <button className={`channel-row all ${p.selected === null ? "active" : ""}`} onClick={() => p.onSelect(null)}>
          <span className="channel-name">All channels</span>
          <span className="count">{Object.values(p.openFlagCounts).reduce((a, b) => a + b, 0) || ""}</span>
        </button>
        {p.channels.length === 0 && <p className="empty-hint">Add the channels you moderate to start monitoring.</p>}
        {p.channels.map((c) => {
          const joined = p.connection.joined.includes(c.name);
          const count = p.openFlagCounts[c.name] ?? 0;
          const raidUntil = p.raidModes[c.name];
          const inRaid = raidUntil && raidUntil > p.now;
          return (
            <div key={c.name} className={`channel-wrap ${c.isMod ? "" : "not-mod"}`}>
              <div className={`channel-row ${p.selected === c.name ? "active" : ""}`}>
                <button
                  className="channel-main"
                  onClick={() => p.onSelect(c.name)}
                  title={!c.isMod ? "You're not a moderator here, so it isn't monitored" : joined ? "Monitoring" : "Joining…"}
                >
                  <span className={`dot ${!c.isMod ? "off" : joined ? "on" : "pending"}`} />
                  <span className="swatch" style={{ background: channelColor(c.name) }} />
                  <span className="channel-name">{c.name}</span>
                  {count > 0 && <span className="count">{count}</span>}
                </button>
                {c.isMod && !inRaid && p.raidModeMinutes > 0 && (
                  <button
                    className="icon-btn"
                    title={`Start raid mode for ${p.raidModeMinutes} min`}
                    onClick={() => p.onRaidMode(c.name, p.raidModeMinutes)}
                  >
                    ⚡
                  </button>
                )}
                <button
                  className={`icon-btn ${settingsFor === c.name ? "muted" : ""}`}
                  title="Channel settings: color, alerts, shoutout message"
                  onClick={() => setSettingsFor(settingsFor === c.name ? null : c.name)}
                >
                  ⚙
                </button>
                <button
                  className={`icon-btn ${c.muted ? "muted" : ""}`}
                  title={c.muted ? "Alerts muted: click to unmute" : "Mute alerts for this channel"}
                  onClick={() => p.onToggleMute(c.name, !c.muted)}
                >
                  {c.muted ? "🔕" : "🔔"}
                </button>
                {confirming === c.name ? (
                  <button
                    className="icon-btn danger"
                    title="Click again to stop monitoring"
                    onClick={() => {
                      setConfirming(null);
                      p.onRemove(c.name);
                    }}
                    onBlur={() => setConfirming(null)}
                    autoFocus
                  >
                    Remove
                  </button>
                ) : (
                  <button className="icon-btn" title="Stop monitoring this channel" onClick={() => setConfirming(c.name)}>
                    ×
                  </button>
                )}
              </div>
              {inRaid && (
                <div className="raid-mode-chip" title="Raid-only rules are active">
                  ⚡ Raid mode {countdown(raidUntil - p.now)}
                  <button className="ghost small" onClick={() => p.onRaidMode(c.name, 0)}>
                    End
                  </button>
                </div>
              )}
              {!c.isMod && <div className="not-mod-note">Not a mod here, so it isn't monitored</div>}
              {c.alertLevel !== "default" && c.alertLevel !== "off" && (
                <div className="alert-level-note">Alerts: {c.alertLevel === "all" ? "everything" : `${c.alertLevel}+`}</div>
              )}
              {settingsFor === c.name && (
                <ChannelSettings
                  channel={c}
                  onAlerts={(level, sound) => p.onAlerts(c.name, level, sound)}
                  onColor={(color) => p.onColor(c.name, color)}
                  onShoutoutMessage={(m) => p.onShoutoutMessage(c.name, m)}
                  onOpenRuleAlerts={() => {
                    setSettingsFor(null);
                    p.onOpenRuleAlerts(c.name);
                  }}
                  onClose={() => setSettingsFor(null)}
                />
              )}
            </div>
          );
        })}
      </div>

      <div className="sidebar-foot">
        <div className={`conn-status ${p.connection.status}`}>
          <span className="dot" /> {statusLabel}
        </div>
        {p.inboxConnected !== null && (
          <div className={`conn-status ${p.inboxConnected ? "connected" : "connecting"}`} title="AutoMod, unban requests and suspicious users">
            <span className="dot" /> {p.inboxConnected ? "Twitch inbox connected" : "Twitch inbox reconnecting…"}
          </div>
        )}
        <div className="account">
          <span className="channel-name">
            Logged in as <strong>{p.account.login}</strong>
          </span>
          <button className="ghost small" onClick={p.onLogout}>
            Log out
          </button>
        </div>
      </div>
    </aside>
  );
}
