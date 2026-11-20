import { useEffect, useState } from "react";
import type { AlertLevel, Channel } from "../types";
import { CHANNEL_PALETTE } from "../util";

interface Props {
  channel: Channel;
  onAlerts: (level: AlertLevel, sound: boolean) => void;
  onColor: (color: string | null) => void;
  onShoutoutMessage: (message: string) => void;
  onClose: () => void;
}

const LEVELS: { value: AlertLevel; label: string }[] = [
  { value: "default", label: "My default" },
  { value: "all", label: "Everything" },
  { value: "medium", label: "Medium and high" },
  { value: "high", label: "High only" },
  { value: "off", label: "Off (queue only)" },
];

export function ChannelSettings({ channel, onAlerts, onColor, onShoutoutMessage, onClose }: Props) {
  const [message, setMessage] = useState(channel.shoutoutMessage);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="channel-settings" role="dialog" aria-label={`Settings for #${channel.name}`}>
      <div className="cs-head">
        <strong>#{channel.name}</strong>
        <button className="ghost small" onClick={onClose} aria-label="Close">
          ×
        </button>
      </div>

      <div className="cs-section">
        <span className="cs-label">Color</span>
        <div className="swatches">
          {CHANNEL_PALETTE.map((c) => (
            <button
              key={c}
              className={`swatch-btn ${channel.color?.toLowerCase() === c ? "on" : ""}`}
              style={{ background: c }}
              title={c}
              onClick={() => onColor(c)}
            />
          ))}
          <label className="swatch-btn custom" title="Pick any color">
            <input type="color" value={channel.color ?? "#4dabf7"} onChange={(e) => onColor(e.target.value)} />+
          </label>
        </div>
      </div>

      <div className="cs-section">
        <span className="cs-label">Alerts for flags in this channel</span>
        <select value={channel.alertLevel} onChange={(e) => onAlerts(e.target.value as AlertLevel, channel.sound)}>
          {LEVELS.map((l) => (
            <option key={l.value} value={l.value}>
              {l.label}
            </option>
          ))}
        </select>
        <label className="check">
          <input
            type="checkbox"
            checked={channel.sound}
            disabled={channel.alertLevel === "off"}
            onChange={(e) => onAlerts(channel.alertLevel, e.target.checked)}
          />
          Play sounds for this channel
        </label>
        <span className="hint">
          “My default” uses the Alerts tab. Every flag still lands in the queue, and rules can override this in the rule editor.
        </span>
      </div>

      <div className="cs-section">
        <span className="cs-label">Shoutout chat message (optional)</span>
        <form
          className="row-gap"
          onSubmit={(e) => {
            e.preventDefault();
            onShoutoutMessage(message);
          }}
        >
          <input value={message} onChange={(e) => setMessage(e.target.value)} placeholder="!so @{raider}" />
          <button disabled={message === channel.shoutoutMessage}>Save</button>
        </form>
        <span className="hint">
          Sent as you along with Twitch's shoutout, for channels that use a bot command. Use {"{raider}"}, {"{raider_name}"} and{" "}
          {"{viewers}"}. Leave empty to only send Twitch's shoutout.
        </span>
      </div>
    </div>
  );
}
