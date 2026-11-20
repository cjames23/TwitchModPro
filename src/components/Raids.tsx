import { useState } from "react";
import type { Raid } from "../types";
import { api } from "../api";
import { channelColor, timeAgo } from "../util";

export function ShoutoutButton({ raid, onShoutout }: { raid: Raid; onShoutout: (raid: Raid) => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  if (raid.shoutoutAt) {
    return (
      <span className="shouted" title={new Date(raid.shoutoutAt).toLocaleString()}>
        ✓ Shouted out
      </span>
    );
  }
  return (
    <button
      disabled={busy}
      title={`Send Twitch's /shoutout for ${raid.fromDisplayName} in #${raid.channel}`}
      onClick={async () => {
        setBusy(true);
        try {
          await onShoutout(raid);
        } finally {
          setBusy(false);
        }
      }}
    >
      {busy ? "Sending…" : "📣 Shoutout"}
    </button>
  );
}

export function RaidBanner({
  raids,
  onDismiss,
  onShoutout,
}: {
  raids: Raid[];
  onDismiss: () => void;
  onShoutout: (raid: Raid) => Promise<void>;
}) {
  const pending = raids.filter((r) => !r.acknowledged);
  if (!pending.length) return null;
  const latest = pending[0];
  return (
    <div className="raid-banner" role="alert">
      <span className="raid-icon">🚨</span>
      <div className="raid-text">
        <strong>{latest.fromDisplayName}</strong> is raiding <strong>#{latest.channel}</strong> with{" "}
        <strong>{latest.viewers.toLocaleString()}</strong> viewers
        {pending.length > 1 && <span className="more"> · +{pending.length - 1} more</span>}
      </div>
      <ShoutoutButton raid={latest} onShoutout={onShoutout} />
      <button onClick={() => api.openChat(latest.channel)}>Open chat ↗</button>
      <button className="primary" onClick={onDismiss}>
        Got it
      </button>
    </div>
  );
}

export function RaidLog({
  raids,
  channelFilter,
  now,
  onShoutout,
}: {
  raids: Raid[];
  channelFilter: string | null;
  now: number;
  onShoutout: (raid: Raid) => Promise<void>;
}) {
  const shown = raids.filter((r) => !channelFilter || r.channel === channelFilter);
  return (
    <section className="panel">
      <header className="panel-header">
        <h2>Raids {channelFilter && <span className="sub">in #{channelFilter}</span>}</h2>
      </header>
      <p className="muted-text panel-note">
        Shoutouts use Twitch's own /shoutout. Twitch only allows them while the channel is live, once every 2 minutes per
        channel, and once an hour for the same streamer. A custom chat message (e.g. <code>!so @{"{raider}"}</code>) can be set per
        channel from the ⚙ button.
      </p>
      {shown.length === 0 ? (
        <div className="empty">
          <p>No raids yet.</p>
          <p className="muted-text">You'll be alerted when someone raids a channel you're watching.</p>
        </div>
      ) : (
        <table className="raid-table">
          <thead>
            <tr>
              <th>When</th>
              <th>Raided channel</th>
              <th>Raider</th>
              <th className="num">Viewers</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <tr key={r.id} className={r.acknowledged ? "" : "unread"}>
                <td title={new Date(r.timestamp).toLocaleString()}>{timeAgo(r.timestamp, now)}</td>
                <td>
                  <span style={{ color: channelColor(r.channel) }}>#{r.channel}</span>
                </td>
                <td>
                  <strong>{r.fromDisplayName}</strong>
                </td>
                <td className="num">{r.viewers.toLocaleString()}</td>
                <td className="row-actions">
                  <ShoutoutButton raid={r} onShoutout={onShoutout} />
                  <button className="ghost small" onClick={() => api.openChannel(r.fromLogin)}>
                    Raider's channel ↗
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
