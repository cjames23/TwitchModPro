import type { Raid } from "../types";
import { api } from "../api";
import { channelColor, timeAgo } from "../util";

export function RaidBanner({ raids, onDismiss }: { raids: Raid[]; onDismiss: () => void }) {
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
      <button onClick={() => api.openChat(latest.channel)}>Open chat ↗</button>
      <button className="primary" onClick={onDismiss}>
        Got it
      </button>
    </div>
  );
}

export function RaidLog({ raids, channelFilter, now }: { raids: Raid[]; channelFilter: string | null; now: number }) {
  const shown = raids.filter((r) => !channelFilter || r.channel === channelFilter);
  return (
    <section className="panel">
      <header className="panel-header">
        <h2>Raids {channelFilter && <span className="sub">in #{channelFilter}</span>}</h2>
      </header>
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
