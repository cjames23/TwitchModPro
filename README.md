# Twitch Mod Pro

A desktop app for Twitch moderators who watch several chats at once. It flags messages that match your rules, tells you which channel they came from, and alerts you when a stream you're watching gets raided.

Built with [Tauri 2](https://tauri.app) (Rust backend) and React + TypeScript.

## What works today (MVP)

- **Watch many channels at once.** Add channels in the sidebar (`name`, `#name`, or a twitch.tv URL; separate several with spaces or commas). Chat is read **anonymously**, so you don't need to log in or register a Twitch app.
- **Rules.** Create rules from words/phrases or regular expressions. Each rule has:
  - whole-word matching (so "ass" doesn't match "class") and case sensitivity
  - a scope: all channels or only the ones you pick
  - a severity: low, medium or high
  - an option to ignore mods, VIPs and the broadcaster
  - a live tester for trying a sample message before you save

  A starter rule for follower/viewer-bot spam is created on first run.
- **Flag queue.** Shows every matched message with its channel, user, matched rules and highlighted terms, plus the 8 messages before it. You can mark a flag Reviewed, Dismiss it, Reopen it, or open that channel's chat. Filter by open/all, severity, or channel (click a channel in the sidebar).
- **Raid alerts.** A banner and desktop notification show which channel was raided, who raided it, and how many viewers came. A raid log keeps the history.
- **Live chat view.** Shows a column per channel, with flagged lines highlighted. Auto-scroll pauses when you scroll up.
- **Alerts.**
  - Desktop notifications and sounds
  - minimum severity before you're alerted for a flag
  - minimum raid size before you're alerted for a raid
  - per-channel mute (the bell icon)
  - a 4-second cooldown per channel so a spam wave doesn't bury you

Data is stored locally in SQLite under the OS app-data folder (`~/Library/Application Support/com.twitchmodpro.app/` on macOS).

## Running it

Prerequisites: [Rust](https://rustup.rs), Node.js 20+, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev      # run in development
npm run tauri build    # build an installer (.dmg/.app on macOS, .msi/.exe on Windows, .deb/.AppImage on Linux)
```

Tests:

```bash
cd src-tauri
cargo test                          # parser, rules engine, storage
cargo test -- --ignored live_       # also connects to real Twitch chat
```

## Project layout

```
src-tauri/src/
  irc.rs      Twitch IRC parser: chat messages and raid USERNOTICEs
  twitch.rs   WebSocket connection: anonymous login, JOIN rate limiting, reconnect with backoff
  rules.rs    Rules engine (Aho-Corasick for keywords, regex)
  db.rs       SQLite storage: channels, rules, flags, raids, settings
  lib.rs      App wiring: runs rules on each message, saves flags, sends notifications, Tauri commands
src/
  App.tsx               App state and event wiring
  components/           Sidebar, FlagQueue, LiveChats, Raids, Rules, SettingsPanel
```

## Roadmap

- **Rules v2:** built-in checks for links, excessive caps, emote/character spam, repeated messages and first-time chatters; leetspeak and lookalike-character normalization; showing messages after a flag in its context.
- **Actions:** Twitch login (OAuth device code flow), then delete, timeout and ban from the queue; a user history panel; a "raid mode" with stricter rules after a raid.
- **Polish:** rule import/export, auto-update, code-signed installers.
