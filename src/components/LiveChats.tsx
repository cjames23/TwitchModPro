import { useEffect, useRef, useState } from "react";
import type { Channel, LiveMessage } from "../types";
import { api } from "../api";
import { channelColor, clockTime } from "../util";

interface Props {
  channels: Channel[];
  messages: Record<string, LiveMessage[]>;
  channelFilter: string | null;
}

function ChatColumn({ channel, messages }: { channel: string; messages: LiveMessage[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pinned, setPinned] = useState(true);

  useEffect(() => {
    const el = ref.current;
    if (el && pinned) el.scrollTop = el.scrollHeight;
  }, [messages, pinned]);

  return (
    <div className="chat-col">
      <div className="chat-col-head" style={{ borderTopColor: channelColor(channel) }}>
        <span>#{channel}</span>
        <button className="ghost small" onClick={() => api.openChat(channel)}>
          ↗
        </button>
      </div>
      <div
        className="chat-scroll"
        ref={ref}
        onScroll={(e) => {
          const el = e.currentTarget;
          setPinned(el.scrollHeight - el.scrollTop - el.clientHeight < 40);
        }}
      >
        {messages.length === 0 && <div className="muted-text pad">Waiting for messages…</div>}
        {messages.map((m) => (
          <div key={m.id} className={`chat-line ${m.flagSeverity ? `flagged sev-${m.flagSeverity}` : ""}`}>
            <span className="ctx-time">{clockTime(m.timestamp)}</span>
            {m.isBroadcaster && <span className="badge b">B</span>}
            {m.isMod && <span className="badge m">M</span>}
            {m.isVip && <span className="badge v">V</span>}
            {m.isFirstMsg && <span className="badge f" title="First message in this channel">1st</span>}
            <strong style={{ color: m.color ?? undefined }}>{m.displayName}</strong>
            {m.isAction ? " " : ": "}
            <span className={m.isAction ? "action" : ""}>{m.text}</span>
          </div>
        ))}
      </div>
      {!pinned && (
        <button className="jump" onClick={() => setPinned(true)}>
          Paused: jump to latest ↓
        </button>
      )}
    </div>
  );
}

export function LiveChats({ channels, messages, channelFilter }: Props) {
  const shown = channels.filter((c) => !channelFilter || c.name === channelFilter);
  if (shown.length === 0) {
    return (
      <section className="panel">
        <div className="empty">
          <p>Add a channel in the sidebar to watch its chat here.</p>
        </div>
      </section>
    );
  }
  return (
    <section className="panel">
      <div className="chat-grid">
        {shown.map((c) => (
          <ChatColumn key={c.name} channel={c.name} messages={messages[c.name] ?? []} />
        ))}
      </div>
    </section>
  );
}
