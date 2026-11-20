import { useState, type FormEvent } from "react";
import type { Channel, ConnectionInfo } from "../types";
import { channelColor } from "../util";

interface Props {
  channels: Channel[];
  connection: ConnectionInfo;
  openFlagCounts: Record<string, number>;
  selected: string | null;
  onSelect: (name: string | null) => void;
  onAdd: (name: string) => Promise<void>;
  onRemove: (name: string) => void;
  onToggleMute: (name: string, muted: boolean) => void;
}

export function Sidebar(p: Props) {
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const names = input.split(/[\s,]+/).filter(Boolean);
    if (!names.length) return;
    setBusy(true);
    setError(null);
    try {
      for (const n of names) await p.onAdd(n);
      setInput("");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

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

      <form className="add-channel" onSubmit={submit}>
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="Add channel(s)…"
          aria-label="Channel name"
          disabled={busy}
        />
        <button type="submit" disabled={busy || !input.trim()}>
          Add
        </button>
      </form>
      {error && <div className="form-error">{error}</div>}

      <div className="channel-list">
        <button className={`channel-row all ${p.selected === null ? "active" : ""}`} onClick={() => p.onSelect(null)}>
          <span className="channel-name">All channels</span>
          <span className="count">{Object.values(p.openFlagCounts).reduce((a, b) => a + b, 0) || ""}</span>
        </button>
        {p.channels.length === 0 && (
          <p className="empty-hint">Add the channels you moderate. Chat is read anonymously, so no login is needed.</p>
        )}
        {p.channels.map((c) => {
          const joined = p.connection.joined.includes(c.name);
          const count = p.openFlagCounts[c.name] ?? 0;
          return (
            <div key={c.name} className={`channel-row ${p.selected === c.name ? "active" : ""}`}>
              <button className="channel-main" onClick={() => p.onSelect(c.name)} title={joined ? "Joined" : "Joining…"}>
                <span className={`dot ${joined ? "on" : "pending"}`} />
                <span className="swatch" style={{ background: channelColor(c.name) }} />
                <span className="channel-name">{c.name}</span>
                {count > 0 && <span className="count">{count}</span>}
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
          );
        })}
      </div>

      <div className={`conn-status ${p.connection.status}`}>
        <span className="dot" /> {statusLabel}
      </div>
    </aside>
  );
}
