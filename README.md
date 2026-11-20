# Twitch Mod Pro

A desktop app for Twitch moderators who watch several chats at once. It flags messages that match your rules, tells you which channel they came from, alerts you when a stream you're watching gets raided, and lets you delete, time out or ban without leaving the app.

Built with [Tauri 2](https://tauri.app) (Rust backend) and React + TypeScript.

## Features

- **Log in with Twitch.** Uses the OAuth device code flow: the app shows a code, you approve it on twitch.tv, and your login is stored in the OS keychain. The token is checked at startup and every 30 minutes, and refreshed automatically.
- **Only channels you moderate.** The channel picker lists only the channels Twitch says you moderate, plus your own, and the backend refuses anything else. Mod status is re-checked every 30 minutes. If you lose mod status in a channel, the app stops monitoring it and marks it in the sidebar.
- **Rules**, each with a severity, a channel scope, an option to ignore mods/VIPs/the broadcaster, and a live tester:
  - **Words & phrases** and **regular expressions**, matched against the message or the sender's **username**
  - **Catch disguised spellings:** undoes leetspeak (`fr33`), full-width and stylized letters (`ｆｒｅｅ`, `𝐟𝐫𝐞𝐞`), Cyrillic/Greek lookalikes, accents and hidden zero-width characters. Matches still highlight the original text.
  - **Links**, with a list of allowed domains (catches `site dot com` too)
  - **Excessive caps** (percentage and minimum length)
  - **Symbol & emote spam** (long runs of one character, too many emotes, ASCII art)
  - **Repeated messages** (the same user sending the same text N times within a time window)
  - **First-time chatters**
  - **Raid mode only:** a rule runs only during raid mode
- **Flag queue.** Shows the channel, user, matched rules, highlighted terms and the reason a built-in check fired. Context includes the 8 messages before the flag and the 5 after it. Delete, Timeout (1m/10m/1h/24h/7d) and Ban work right from the card, and a flag closes itself with a note such as "Timed out 10m".
- **User panel.** Click any username to see their recent messages, past flags, and the actions taken against them from this app. You can delete individual messages, time out, ban or unban.
- **Cross-channel awareness.** Every flag shows the account's age and any warns, timeouts or bans of that user in your *other* channels, whether you did them or another mod did (seen in chat). The live chat marks users with a record elsewhere, and the user panel lists their history across channels.
- **Ban everywhere.** The ban prompt has an "All my channels" option that bans the user in every monitored channel you moderate.
- **New-account rule.** Flags accounts younger than N days. The app looks up account ages from Twitch in batches, about a second after someone's first message, and flags a user once per channel.
- **Spam-wave detection.** Finds groups of accounts that post the same message (after undoing disguises and ignoring numbers), have look-alike names (`hoss00312_1`, `_2`…), or chat for the first time all at once. Each wave shows as one card with **Ban all / Time out all**, a list where you can untick accounts, **Undo** (unbans everyone the app acted on), and **Not spam** (ignores that pattern for 30 minutes). You set the sensitivity in Alerts.
- **Twitch inbox.** Through EventSub, the queue also gets **AutoMod held messages** (Approve/Deny), **unban requests** (Unban/Deny, with an optional reply), and **suspicious-user messages** (likely ban evaders, restricted users, shared-ban matches). Pending unban requests load at startup. When another mod handles an item on Twitch, the card closes by itself.
- **Escalation ladder.** The suggested next action is based on the user's prior offenses in that channel within a time window. The default ladder is Warn → Timeout 10m → Timeout 24h → Ban, and you can edit it. Warn uses Twitch's warning, which the user has to acknowledge before chatting again.
- **Keyboard shortcuts** in the flag queue:
  - `J`/`K` to move between flags
  - `Enter` for the suggested action
  - `W` Warn, `T` Timeout, `B` Ban, `X` Delete message
  - `R` Reviewed, `D` Dismiss
  - `A`/`N` to approve or deny AutoMod messages and unban requests
  - `U` to open the user panel, `C` for context
  - `?` to show the list
- **Default rule packs**, which start in test mode:
  - **Parasocial:** stalking/location, romantic/possessive, personal probing, jealousy/entitlement, and fixation (one user talking to the streamer over and over)
  - **Sexual content:** explicit requests, body comments, camera/pose requests, and innuendo (off by default)
  - **Harassment:** telling someone to die
  - **Wellbeing:** self-harm statements, shown as a check-in with no punishment
  - **Chat quality:** backseating (off by default)

  Deleted pack rules can be brought back from the Tuning view.
- **Rule options:**
  - "Must also contain" (a second word list that also has to match)
  - "Only messages to the streamer" (their name, @, or "you"; replies to other chatters don't count)
  - exception phrases
  - ignore subscribers
  - ignore accounts older than N days
  - wellbeing mode
- **Test mode:** a rule in test mode records what it *would* have flagged without touching your queue. New rules start in test mode.
- **Tuning view:**
  - per-rule and per-word stats over 7 or 30 days: flags, acted on, dismissed, accuracy
  - test-mode examples
  - one-click suggestions: remove a noisy word, lower or raise severity, send a rule back to test mode, turn on a tested rule
  - quick exception phrases

  Cleared flags are archived instead of deleted, so they still count, and tuning data is kept for 30 days.
- **Raid alerts and raid mode.** A banner and notification show which channel was raided, by whom, and with how many viewers. Raid mode turns on automatically for a few minutes after a raid (configurable), and you can start or end it by hand with ⚡ in the sidebar.
- **Live chat.** One column per channel, with flagged lines highlighted. Click a name to open the user panel.
- **Alerts.**
  - desktop notifications and sounds
  - minimum severity for flag alerts
  - minimum raid size for raid alerts
  - per-channel mute
  - a per-channel cooldown

Data is stored locally in SQLite under the OS app-data folder (`~/Library/Application Support/com.twitchmodpro.app/` on macOS). Login tokens are kept in the OS keychain, never in the database.

## First-time setup: Twitch Client ID

Twitch only shares mod status with registered applications, so each install needs a Client ID:

1. Go to <https://dev.twitch.tv/console/apps/create> and register an application:
   - **OAuth Redirect URL:** `http://localhost` (unused by the device code flow, but required by the form)
   - **Category:** Application Integration
   - **Client Type:** **Public**
2. Paste the Client ID into the app's login screen.

To ship a build with your Client ID baked in, so users skip this step, set it at build time:

```bash
TWITCH_CLIENT_ID=your_client_id npm run tauri build
```

Scopes requested: `user:read:moderated_channels`, `moderator:manage:chat_messages`, `moderator:manage:banned_users`, `moderator:manage:warnings`, `moderator:manage:automod`, `moderator:manage:unban_requests`, `moderator:read:suspicious_users`. If you logged in before these were added, the app asks you to log in again.

## Running it

Prerequisites: [Rust](https://rustup.rs), Node.js 20+, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev      # run in development
npm run tauri build    # build an installer (.dmg/.app on macOS, .msi/.exe on Windows, .deb/.AppImage on Linux)
```

UI preview with a fake backend (no Twitch account needed): run `npm run dev` and open `http://localhost:1420/preview.html`. Add `?view=login` or `?view=code` to see the login steps.

Tests:

```bash
cd src-tauri
cargo test                          # parser, normalizer, rules, storage, Twitch API (against a mock server)
cargo test -- --ignored live_       # also connects to real Twitch chat
```

## Project layout

```
src-tauri/src/
  irc.rs         Twitch IRC parser: chat messages (with user/room IDs, emote counts) and raids
  twitch.rs      Chat WebSocket: JOIN rate limiting, reconnect with backoff
  twitch_api.rs  Device code login, token refresh/validation, keychain, Helix moderation API
  eventsub.rs    EventSub WebSocket: AutoMod, unban requests, suspicious users
  waves.rs       Spam-wave detector
  intel.rs       Account ages, other mods' actions, waves, inbox, ban-everywhere, bulk actions
  normalize.rs   Anti-evasion text normalization, with mapping back to the original text
  packs.rs       Default rule packs (parasocial, sexual content, harassment, wellbeing)
  rules.rs       Rules engine (keywords, regex, links, caps, spam, repeats, first message)
  db.rs          SQLite storage and migrations: channels, rules, flags, raids, mod actions, settings
  core.rs        Session, mod-verified channels, chat processing, raid mode, user history, actions
  commands.rs    Tauri commands used by the UI
src/
  App.tsx        App state and event wiring
  components/    LoginScreen, Sidebar, ChannelPicker, FlagQueue, ModActions, UserPanel, LiveChats, Raids, Rules, SettingsPanel
  dev/preview.ts Fake backend for the browser UI preview
```

## Roadmap

- Rule import/export for sharing between mod teams
- Raid vetting (raider's account age and history) and switching on followers-only / slow mode / Shield Mode during raid mode
- Shift summaries
- A shared queue for whole mod teams (needs a server)
- Auto-update and code-signed installers
