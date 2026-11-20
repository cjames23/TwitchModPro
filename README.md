# Twitch Mod Pro

**One window for moderating all the Twitch chats you mod.**

If you mod several streamers, you know the drill: a dozen chat tabs, scrolling fast, hoping you spot the problem message in time. Twitch Mod Pro watches every chat you moderate at once and pulls anything that needs your attention into **one queue**: rule matches, AutoMod holds, unban requests, spam waves and raids. You can deal with each item right there: delete, warn, time out or ban, without switching tabs.

**[⬇ Download the latest version](https://github.com/cjames23/TwitchModPro/releases)** for macOS, Windows or Linux.

> **Early test version.** Things may break. Please report problems or ideas in [Issues](https://github.com/cjames23/TwitchModPro/issues).

---

- [Getting started](#getting-started)
- [A quick tour](#a-quick-tour)
- [How it works](#how-it-works): flags, rules, test mode, **raid mode**, spam waves, alerts…
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Your data and privacy](#your-data-and-privacy)
- [What Twitch doesn't allow](#what-twitch-doesnt-allow)
- [Troubleshooting](#troubleshooting)
- [For developers](#for-developers)

---

## Getting started

### 1. Install

Download the file for your computer from the [Releases page](https://github.com/cjames23/TwitchModPro/releases):

| Your computer | Download | Then |
|---|---|---|
| **Mac** (any) | the `.dmg` | Open it and drag **Twitch Mod Pro** into Applications. |
| **Windows** | the `-setup.exe` | Run it. |
| **Linux** | the `.AppImage` (or `.deb` / `.rpm`) | Make the AppImage executable and run it. |

The app isn't code-signed yet, so the first time you open it your computer will warn you:

- **Mac:** if it says it can't check the app, go to **System Settings → Privacy & Security**, scroll down and click **Open Anyway**. If it says the app is *damaged*, run this once in Terminal, then open it again:
  ```bash
  xattr -dr com.apple.quarantine "/Applications/Twitch Mod Pro.app"
  ```
- **Windows:** if you see "Windows protected your PC", click **More info → Run anyway**.

### 2. Connect it to Twitch (about a minute, once)

Twitch only tells approved apps which channels you moderate, so the app needs a **Client ID**. It's free:

1. Go to **[dev.twitch.tv/console/apps/create](https://dev.twitch.tv/console/apps/create)** and log in.
2. Fill in the form:
   - **Name:** anything, e.g. "My Mod Tool". Twitch may reject names containing "Twitch".
   - **OAuth Redirect URLs:** `http://localhost`
   - **Category:** Application Integration
   - **Client Type:** **Public** (important)
3. Click **Create**, then **Manage**, and copy the **Client ID**.
4. Paste it into the app.

### 3. Log in and add your channels

1. Click **Log in with Twitch**. A Twitch page opens in your browser showing a code. Check it matches the one in the app, then click **Authorize**.
2. Click **+ Add channel**. You'll see every channel you moderate, plus your own. Add the ones you want to watch.

That's it. Flags start appearing as chat happens.

> **Tip:** the built-in rule packs (parasocial, sexual content, harassment and more) start in **test mode**, so they won't fill your queue on day one. After a stream, open the **Tuning** tab to see what they *would* have caught and switch on the ones that look right.

---

## A quick tour

| Tab | What it's for |
|---|---|
| **Flags** | Your to-do list. Every message that needs a look, from every channel, newest first. Act on it right from the card. |
| **Live chat** | Every channel's chat side by side, with flagged lines highlighted. Click a name to see that person's history. |
| **Activity** | Follows, subs, gift subs, bits and announcements across your channels. |
| **Mod log** | Everything any mod did in your channels (bans, timeouts, deletes, chat setting changes), with who did it. |
| **Raids** | Every raid on your channels, with a **📣 Shoutout** button. |
| **Rules** | What gets flagged. Edit the built-in rules or make your own. |
| **Tuning** | How well each rule is working, with suggestions to make it less noisy. |
| **Alerts** | When the app should pop up a notification or play a sound, overall and for each streamer. |

The **sidebar** lists your channels. Click one to show only that channel everywhere. Each channel has:
- a **⚙** button for its color, alert settings and shoutout message
- a **🔔** button to quickly mute its alerts
- a **⚡** button to start raid mode by hand

---

## How it works

### Flags

A **flag** is a message the app thinks you should look at. Each one shows:

- **which channel** it's from (color-coded) and **who** sent it
- **why** it was flagged, with the matching words highlighted
- the **severity**: low, medium or high
- **notes about the user:** how old their account is, and whether they've been warned, timed out or banned in your *other* channels
- the conversation **before and after** (click *Context*)

From the card you can **Delete** the message, **Warn**, **Timeout** or **Ban** the user, or mark the flag **Reviewed** or **Dismiss** it. When you take an action, the flag closes itself with a note like "Timed out 10m". If another mod deals with the user on Twitch, the flag closes too.

### The Twitch inbox

Besides your own rules, the flag queue also collects items Twitch is already holding for mods:

- **AutoMod** messages waiting for approval: **Approve** or **Deny**
- **Unban requests**: **Unban** or **Deny**, with an optional reply
- **Suspicious users**: people Twitch thinks may be evading a ban, or who are restricted or banned in channels that share ban info

### Rules

A **rule** describes what to flag. There are several kinds:

| Kind | Flags… |
|---|---|
| **Words & phrases** | messages (or usernames) containing any word on your list |
| **Links** | any link, except to domains you allow |
| **Excessive caps** | MESSAGES THAT ARE MOSTLY CAPITALS |
| **Symbol & emote spam** | aaaaaaaaaa, emote walls, ASCII art |
| **Repeated messages** | the same person sending the same thing over and over |
| **First-time chatters** | someone's first-ever message in the channel |
| **New accounts** | accounts created in the last few days |
| **Fixation on the streamer** | one person talking to the streamer over and over in a short time |
| **Regular expression** | advanced pattern matching |

Useful options on a rule:
- **Catch disguised spellings:** also catches tricks like `fr33`, `ｆｒｅｅ` and lookalike letters.
- **Who it's aimed at:** *anyone*, *not another chatter* (skips replies to other people), or *clearly the streamer*.
- **Exceptions:** phrases that should never trigger it, e.g. "die hard" for a rule about "die".
- **Ignore mods, VIPs and the broadcaster**, ignore subscribers, or ignore older accounts.
- **Only during raid mode:** see [Raid mode](#raid-mode).

Every rule has a **Try it** box, so you can type a sample message and see whether it would be flagged before saving.

### Built-in rule packs

The app comes with ready-made rules, grouped into packs:

- **Parasocial:** stalking or location questions ("I know where you live"), romantic or possessive messages ("marry me", "you're mine"), personal probing ("are you single?"), jealousy and entitlement ("why are you ignoring me"), and fixation (one person messaging the streamer again and again).
- **Sexual content:** explicit requests, comments about the streamer's body, camera or pose requests, and innuendo (off by default).
- **Harassment:** telling someone to die.
- **Wellbeing:** self-harm statements. These show up as a **wellbeing check**, a gentle prompt to check in with the person, never a punishment.
- **Chat quality:** backseating ("go left", "wrong way"; off by default).

All packs start in **test mode**. If you delete one by accident, **Tuning → Restore default rules** brings it back.

### Test mode and tuning

A rule in **test mode** doesn't create flags. It quietly records what it *would* have flagged. That way you can see how a rule behaves in *your* chats before it starts adding to your queue. New rules start in test mode too.

The **Tuning** tab shows, for each rule:
- how many times it fired
- how often you **acted on** those flags compared with how often you **dismissed** them (accuracy)
- the same numbers for **each word** in the rule

It also makes suggestions you can apply with one click:
- "this word gets dismissed 9 times out of 10, remove it?"
- "this rule is mostly false alarms, lower its severity?"
- "this test rule looks good, turn it on?"

### Raid mode

**Raid mode is a short "high alert" period for one channel, usually right after it's been raided.**

When a big group of new viewers arrives at once, it's a good time for extra caution: brand-new accounts, people chatting for the first time, and in a bad raid, a wave of trolls. Raid mode lets you have **rules that only run during that window**, so they don't bother you the rest of the stream.

- **Turning it on:** it starts **automatically** when a channel you watch is raided, and lasts **5 minutes** by default. You can change the length (or turn automatic raid mode off with 0) in **Alerts → Raids**. You can also start it **by hand** with the **⚡** button next to a channel, for example if a raid-like flood happens without an actual raid.
- **While it's on:** the channel shows **⚡ Raid mode** with a countdown in the sidebar. Every rule with **"Only during raid mode"** ticked is switched on for that channel only.
- **Turning it off:** it ends when the countdown runs out, or right away with **End**.

To use raid mode, open a rule and tick **Only during raid mode**. Good candidates are **First-time chatters** and **New accounts** rules, which would be far too noisy all the time but are useful in the minutes after a raid. None of the built-in packs use raid mode yet, so add these yourself.

> Raid mode only changes **which rules are active in this app**. It doesn't change the channel's settings on Twitch (followers-only, slow mode, etc.).

### Raids and shoutouts

When a channel you watch is raided, a banner appears at the top of the app, along with a desktop notification. Both show **which channel** was raided, **who** raided, and with **how many viewers**. Click **📣 Shoutout** to send Twitch's own shoutout for the raider. If the channel uses a bot command instead, set a chat message such as `!so @{raider}` in the channel's ⚙ settings and the app sends it too.

Twitch's rules apply:
- the channel has to be live
- one shoutout every 2 minutes per channel
- the same streamer once an hour

### Spam waves

A **spam wave** is many different accounts doing the same thing at once, the usual pattern of bot attacks and hate raids. The app spots:
- many accounts posting the **same message**, even with small changes
- many accounts with **look-alike names** (`hoss00312_1`, `hoss00312_2`…)
- a sudden **burst of first-time chatters**

A wave shows up as one card at the top of the flag queue. From it you can:
- **Ban all** or **Time out all**, after unticking anyone who shouldn't be included
- **Undo** to unban everyone the app just acted on
- mark it **Not spam** to ignore that pattern for 30 minutes

### Repeat offenders

The app remembers what's happened to each user, in this channel and your others. The highlighted button on each flag suggests the **next step** for that person, based on how many times they've been dealt with recently:

**1st offense:** Warn → **2nd:** Timeout 10 min → **3rd:** Timeout 24 h → **4th:** Ban

You can change these steps in **Alerts → Escalation ladder**. A **Warn** is Twitch's official warning, which the user has to acknowledge before they can chat again.

To ban someone from **every** channel you moderate at once, tick **All my channels** in the ban prompt.

### Alerts

Every flag lands in the queue. **Alerts** decide which ones also pop up a desktop notification and play a sound.

- **Your default** (Alerts tab): e.g. "medium and high".
- **Per streamer** (⚙ next to a channel): *my default*, *everything*, *medium and high*, *high only*, or *off*. Off means queue only.
- **Per rule, per streamer** (Alerts → **Alerts by channel**): set any rule, or a whole pack, to *always* or *never* alert in that streamer's chat. A 🔔 column shows exactly what will alert.

For example: backseating could alert you in a speedrunner's chat but stay quiet in a chill art stream.

---

## Keyboard shortcuts

In the **Flags** tab (press **?** to see this list in the app):

| Key | Does |
|---|---|
| **J** / **K** (or ↓ / ↑) | Next / previous flag |
| **Enter** | The suggested action |
| **W** · **T** · **B** | Warn · Timeout · Ban |
| **X** | Delete the message |
| **R** · **D** | Mark reviewed · Dismiss |
| **A** · **N** | Approve · Deny (AutoMod and unban requests) |
| **U** | Open the user's history |
| **C** | Show the conversation around the message |

---

## Your data and privacy

- Everything (your channels, rules, flags and history) is stored **only on your computer**:
  - macOS: `~/Library/Application Support/com.twitchmodpro.app/`
  - Windows: `%APPDATA%\com.twitchmodpro.app\`
  - Linux: `~/.local/share/com.twitchmodpro.app/`
- Your Twitch login is kept in your system's **keychain** (macOS Keychain, Windows Credential Manager, or the Linux keyring), never in a plain file.
- The app only talks to **Twitch**. There's no server, account or analytics.
- It only asks Twitch for **moderator** permissions. It never gets access to a streamer's account.

---

## What Twitch doesn't allow

Because the app logs in as a **moderator**, some things in Twitch's Mod View can't be offered: **predictions, polls, the channel point reward queue and hype trains**. Twitch only gives those to the streamer's own login. **[docs/twitch-limitations.md](docs/twitch-limitations.md)** lists each limit and explains why.

---

## Troubleshooting

**I can't add a channel.**
You can only add channels where you're a moderator, or your own. If you were just made a mod, click **↻** in the channel picker to refresh.

**A channel says "Not a mod here".**
Twitch says you're no longer a moderator there, so the app stopped watching it. Remove it, or ask the streamer to re-mod you.

**It asks me to log in again.**
This is normal after an update that needs a new Twitch permission, or roughly once a month, when Twitch's login expires for apps like this one.

**I'm not getting notifications.**
Check the channel isn't muted (🔔 icon) or set to **Off**, check your Alerts tab defaults, and make sure your system allows notifications from Twitch Mod Pro.

**"Couldn't turn on the AutoMod inbox / mod log" for a channel.**
Usually the broadcaster has limited what mods can do, or you need to log in again for a new permission.

**Linux: it won't remember my login.**
The app stores your login in the system keyring. Make sure GNOME Keyring, KWallet or another Secret Service provider is running.

---

## For developers

<details>
<summary>Building, testing and releasing</summary>

Built with [Tauri 2](https://tauri.app) (Rust) and React + TypeScript.

**Requirements:** [Rust](https://rustup.rs), Node.js 20+, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev      # run in development
npm run tauri build    # build installers for this OS
```

**UI preview without Twitch:** run `npm run dev`, then open `http://localhost:1420/preview.html`. Add `?view=login` or `?view=code` to see the login steps.

**Tests:**

```bash
cd src-tauri
cargo test                       # parser, rules, storage, Twitch API (against a mock server)
cargo test -- --ignored live_    # also connects to real Twitch chat
```

**Releasing:** bump the version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, then push a tag with the same version, e.g. `git tag 0.2.0 && git push origin 0.2.0`. GitHub Actions runs the tests, builds macOS, Windows and Linux installers, and publishes them as a release. Versions starting with 0 are marked as pre-releases.

**Built-in Client ID:** set a `TWITCH_CLIENT_ID` repository variable (or environment variable for local builds), and users can skip step 2 of setup.

**Twitch permissions requested** (all moderator-level, explained in [docs/twitch-limitations.md](docs/twitch-limitations.md)): `user:read:moderated_channels`, `moderator:manage:chat_messages`, `moderator:manage:banned_users`, `moderator:manage:warnings`, `moderator:manage:automod`, `moderator:manage:unban_requests`, `moderator:read:suspicious_users`, `moderator:manage:shoutouts`, `user:write:chat`, `moderator:read:followers`, `moderator:read:blocked_terms`, `moderator:read:chat_settings`, `moderator:read:moderators`, `moderator:read:vips`.

**Project layout:**

```
src-tauri/src/
  irc.rs         Twitch chat parser: messages, raids, subs/bits, mod events
  twitch.rs      Chat connection: rate limiting, reconnects
  twitch_api.rs  Login (device code flow), token refresh, keychain, Twitch moderation API
  eventsub.rs    Twitch event connection: AutoMod, unban requests, suspicious users, mod log, follows
  rules.rs       Rules engine
  normalize.rs   Disguised-spelling detection
  packs.rs       Built-in rule packs
  waves.rs       Spam-wave detection
  alerts.rs      When flags alert (channel levels, per-rule overrides)
  intel.rs       Account ages, other mods' actions, waves, inbox, bulk actions
  core.rs        Session, channels, chat processing, raid mode, shoutouts, user history
  db.rs          Local SQLite storage and migrations
  commands.rs    Commands called by the UI
src/
  App.tsx        App state and wiring
  components/    The screens (flag queue, rules, tuning, alerts, feeds…)
  dev/preview.ts Fake backend for the UI preview
.github/workflows/release.yml   Builds and publishes releases on version tags
```

</details>

---

## Roadmap

- Raid vetting (the raider's account age and history), and switching on followers-only, slow mode or Shield Mode during raid mode
- Sharing rule sets between mod teams
- Shift summaries for handing over to the next mod
- A shared queue for whole mod teams
- Automatic updates and code-signed installers
