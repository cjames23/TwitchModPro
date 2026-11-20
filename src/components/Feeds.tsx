import { useState } from "react";
import type { Activity, ModLogEntry } from "../types";
import { channelColor, clockTime, timeAgo } from "../util";

const ACTIVITY_LABEL: Record<Activity["kind"], string> = {
  follow: "Follow",
  sub: "Sub",
  resub: "Resub",
  subgift: "Gift",
  giftbomb: "Gift bomb",
  upgrade: "Upgrade",
  bits: "Bits",
  announcement: "Announcement",
  other: "Other",
};

const ACTIVITY_GROUPS: { id: string; label: string; kinds: Activity["kind"][] }[] = [
  { id: "all", label: "Everything", kinds: [] },
  { id: "follow", label: "Follows", kinds: ["follow"] },
  { id: "subs", label: "Subs & gifts", kinds: ["sub", "resub", "subgift", "giftbomb", "upgrade"] },
  { id: "bits", label: "Bits", kinds: ["bits"] },
  { id: "announcement", label: "Announcements", kinds: ["announcement", "other"] },
];

function Limitation({ children }: { children: React.ReactNode }) {
  return <div className="limitation">{children}</div>;
}

export function ActivityFeed({ items, channelFilter, now }: { items: Activity[]; channelFilter: string | null; now: number }) {
  const [group, setGroup] = useState("all");
  const kinds = ACTIVITY_GROUPS.find((g) => g.id === group)?.kinds ?? [];
  const shown = items.filter((a) => (!channelFilter || a.channel === channelFilter) && (!kinds.length || kinds.includes(a.kind)));
  return (
    <section className="panel">
      <header className="panel-header">
        <h2>Activity {channelFilter && <span className="sub">in #{channelFilter}</span>}</h2>
        <div className="seg">
          {ACTIVITY_GROUPS.map((g) => (
            <button key={g.id} className={group === g.id ? "on" : ""} onClick={() => setGroup(g.id)}>
              {g.label}
            </button>
          ))}
        </div>
      </header>
      <Limitation>
        <strong>Not shown: hype trains, predictions, polls and channel point redemptions.</strong> Twitch only gives these to
        the broadcaster's own login, not to moderators, so no mod tool can show them. Follows need the channel's EventSub
        access. Everything else comes from chat.
      </Limitation>
      <div className="feed">
        {shown.length === 0 && <div className="empty"><p>No activity yet.</p></div>}
        {shown.map((a) => (
          <div key={a.id} className={`feed-row activity-${a.kind}`}>
            <span className="ctx-time" title={new Date(a.timestamp).toLocaleString()}>
              {clockTime(a.timestamp)}
            </span>
            <span className="chan-tag small" style={{ borderColor: channelColor(a.channel), color: channelColor(a.channel) }}>
              #{a.channel}
            </span>
            <span className={`activity-tag ${a.kind}`}>{ACTIVITY_LABEL[a.kind]}</span>
            <span className="feed-text">
              {a.kind === "follow" || a.kind === "bits" ? (
                <>
                  <strong>{a.displayName}</strong> {a.text}
                </>
              ) : (
                a.text
              )}
            </span>
            <span className="muted-text feed-ago">{timeAgo(a.timestamp, now)}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

const MOD_GROUPS: { id: string; label: string; actions: string[] }[] = [
  { id: "all", label: "Everything", actions: [] },
  { id: "bans", label: "Bans & timeouts", actions: ["ban", "timeout", "unban", "untimeout", "shared_chat_ban", "shared_chat_timeout", "shared_chat_unban", "shared_chat_untimeout"] },
  { id: "messages", label: "Deletes & warns", actions: ["delete", "warn", "clear", "shared_chat_delete"] },
  { id: "settings", label: "Chat settings", actions: ["slow", "slowoff", "followers", "followersoff", "subscribers", "subscribersoff", "emoteonly", "emoteonlyoff", "uniquechat", "uniquechatoff"] },
  { id: "roles", label: "Roles & terms", actions: ["mod", "unmod", "vip", "unvip", "add_blocked_term", "remove_blocked_term", "add_permitted_term", "remove_permitted_term", "approve_unban_request", "deny_unban_request", "raid", "unraid"] },
];

export function ModLog({
  entries,
  channelFilter,
  now,
  onOpenUser,
}: {
  entries: ModLogEntry[];
  channelFilter: string | null;
  now: number;
  onOpenUser: (channel: string, login: string) => void;
}) {
  const [group, setGroup] = useState("all");
  const [mod, setMod] = useState("");
  const actions = MOD_GROUPS.find((g) => g.id === group)?.actions ?? [];
  const mods = [...new Set(entries.map((e) => e.moderator))].sort();
  const shown = entries.filter(
    (e) =>
      (!channelFilter || e.channel === channelFilter) &&
      (!actions.length || actions.includes(e.action)) &&
      (!mod || e.moderator === mod),
  );
  return (
    <section className="panel">
      <header className="panel-header">
        <h2>Mod log {channelFilter && <span className="sub">in #{channelFilter}</span>}</h2>
        <div className="filters">
          <div className="seg">
            {MOD_GROUPS.map((g) => (
              <button key={g.id} className={group === g.id ? "on" : ""} onClick={() => setGroup(g.id)}>
                {g.label}
              </button>
            ))}
          </div>
          <select value={mod} onChange={(e) => setMod(e.target.value)}>
            <option value="">All mods</option>
            {mods.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </div>
      </header>
      <Limitation>
        Every mod action in your channels, by any moderator, as Twitch reports it. It starts recording once the app is running
        and you're logged in. Twitch doesn't provide history from before that, and entries are kept for 30 days.
      </Limitation>
      <div className="feed">
        {shown.length === 0 && <div className="empty"><p>No mod actions recorded yet.</p></div>}
        {shown.map((e) => (
          <div key={e.id} className="feed-row">
            <span className="ctx-time" title={new Date(e.timestamp).toLocaleString()}>
              {clockTime(e.timestamp)}
            </span>
            <span className="chan-tag small" style={{ borderColor: channelColor(e.channel), color: channelColor(e.channel) }}>
              #{e.channel}
            </span>
            <span className="feed-text">
              <strong>{e.moderator}</strong> {e.summary}
              {e.targetLogin && (
                <button className="ghost small" onClick={() => onOpenUser(e.channel, e.targetLogin!)} title="Open user history">
                  history
                </button>
              )}
            </span>
            <span className="muted-text feed-ago">{timeAgo(e.timestamp, now)}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
