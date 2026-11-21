import { Fragment, useState } from "react";
import type { Raid, RaidRisk } from "../types";
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

const RISK_LABEL = { low: "Looks fine", medium: "Keep an eye on it", high: "Likely hate or bot raid" } as const;

/** A short risk badge: "Checking…" for the first 90 seconds, then the verdict. */
export function RiskBadge({ risk }: { risk: RaidRisk | null | undefined }) {
  if (!risk) return <span className="risk-badge checking">Checking…</span>;
  const label = RISK_LABEL[risk.level];
  return (
    <span
      className={`risk-badge ${risk.level} ${risk.done ? "" : "live"}`}
      title={risk.reasons.length ? risk.reasons.map((r) => `${r.weight > 0 ? "▲" : "▼"} ${r.text}`).join("\n") : "Nothing unusual so far"}
    >
      {risk.done ? label : `${label} · watching`}
    </span>
  );
}

/** Why the raid got its score, strongest reason first. */
export function RiskReasons({ risk }: { risk: RaidRisk }) {
  const facts = [
    `${risk.chatters} new chatter${risk.chatters === 1 ? "" : "s"}`,
    risk.knownAges ? `${risk.newAccounts} of ${risk.knownAges} under a week old` : null,
    risk.flagged ? `${risk.flagged} flagged` : null,
  ].filter(Boolean);
  return (
    <div className="risk-reasons">
      {risk.reasons.length === 0 ? (
        <span className="muted-text">Nothing unusual so far.</span>
      ) : (
        <ul>
          {risk.reasons.map((r) => (
            <li key={r.text} className={r.weight > 0 ? "up" : "down"}>
              {r.text}
            </li>
          ))}
        </ul>
      )}
      <span className="muted-text">{facts.join(" · ")}</span>
    </div>
  );
}

export function RaidBanner({
  raids,
  onDismiss,
  onShoutout,
  onReviewRaiders,
}: {
  raids: Raid[];
  onDismiss: () => void;
  onShoutout: (raid: Raid) => Promise<void>;
  onReviewRaiders: () => void;
}) {
  const pending = raids.filter((r) => !r.acknowledged);
  if (!pending.length) return null;
  const latest = pending[0];
  const risky = latest.risk?.level === "high";
  return (
    <div className={`raid-banner ${risky ? "risky" : ""}`} role="alert">
      <span className="raid-icon">{risky ? "⚠️" : "🚨"}</span>
      <div className="raid-text">
        <strong>{latest.fromDisplayName}</strong> is raiding <strong>#{latest.channel}</strong> with{" "}
        <strong>{latest.viewers.toLocaleString()}</strong> viewers <RiskBadge risk={latest.risk} />
        {pending.length > 1 && <span className="more"> · +{pending.length - 1} more</span>}
        {risky && latest.risk?.reasons[0] && <div className="raid-why">{latest.risk.reasons[0].text}</div>}
      </div>
      {risky && latest.risk?.waveId != null && (
        <button className="danger solid" onClick={onReviewRaiders} title="The suspicious raiders are listed in the flag queue, ready to ban or time out">
          Review raiders
        </button>
      )}
      {!risky && <ShoutoutButton raid={latest} onShoutout={onShoutout} />}
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
  onReviewRaiders,
}: {
  raids: Raid[];
  channelFilter: string | null;
  now: number;
  onShoutout: (raid: Raid) => Promise<void>;
  onReviewRaiders: () => void;
}) {
  const [open, setOpen] = useState<number | null>(null);
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
      <p className="muted-text panel-note">
        For 90 seconds after a raid, the app checks the raiding channel (age, followers, earlier raids) and the people
        who start chatting (new accounts, look-alike names, rule hits). Twitch doesn't say who came with a raid, so
        anyone new to chat in that time counts as a raider. Click a row to see why it got its score.
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
              <th>Risk</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <Fragment key={r.id}>
              <tr className={`${r.acknowledged ? "" : "unread"} clickable`} onClick={() => setOpen(open === r.id ? null : r.id)}>
                <td title={new Date(r.timestamp).toLocaleString()}>{timeAgo(r.timestamp, now)}</td>
                <td>
                  <span style={{ color: channelColor(r.channel) }}>#{r.channel}</span>
                </td>
                <td>
                  <strong>{r.fromDisplayName}</strong>
                </td>
                <td className="num">{r.viewers.toLocaleString()}</td>
                <td>{r.risk || now - r.timestamp < 120_000 ? <RiskBadge risk={r.risk} /> : <span className="muted-text">—</span>}</td>
                <td className="row-actions" onClick={(e) => e.stopPropagation()}>
                  {r.risk?.waveId != null && r.risk.level === "high" && (
                    <button className="danger small" onClick={onReviewRaiders}>
                      Review raiders
                    </button>
                  )}
                  <ShoutoutButton raid={r} onShoutout={onShoutout} />
                  <button className="ghost small" onClick={() => api.openChannel(r.fromLogin)}>
                    Raider's channel ↗
                  </button>
                </td>
              </tr>
              {open === r.id && r.risk && (
                <tr className="risk-row">
                  <td colSpan={6}>
                    <RiskReasons risk={r.risk} />
                  </td>
                </tr>
              )}
              </Fragment>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
