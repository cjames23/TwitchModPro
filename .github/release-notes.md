**Twitch Mod Pro {{VERSION}}** is an early test build. It's a desktop app for Twitch moderators who watch several chats at once: one flag queue across all your channels, rules you control, raid alerts, AutoMod and unban requests, spam-wave detection, and delete/warn/timeout/ban without leaving the app.

Please report anything that breaks or feels wrong in [Issues]({{REPO_URL}}/issues).

## Download

| System | File |
|---|---|
| **macOS** (Apple Silicon and Intel) | `Twitch.Mod.Pro_{{VERSION}}_universal.dmg` |
| **Windows** | `Twitch.Mod.Pro_{{VERSION}}_x64-setup.exe` (or the `.msi`) |
| **Linux** | `.AppImage` (any distro), `.deb` (Ubuntu/Debian) or `.rpm` (Fedora) |

## Installing

The app isn't code-signed yet, so your system will warn you the first time:

- **macOS:** open the `.dmg` and drag the app to Applications. If macOS says it can't verify the app, open **System Settings → Privacy & Security**, scroll down and click **Open Anyway**. If it says the app is "damaged", run this once in Terminal:
  `xattr -dr com.apple.quarantine "/Applications/Twitch Mod Pro.app"`
- **Windows:** if SmartScreen says "Windows protected your PC", click **More info → Run anyway**.
- **Linux:** for the AppImage, `chmod +x` it and run it. The app keeps your login in the system keychain, so a keyring service such as GNOME Keyring or KWallet needs to be running.

## First run

1. If the app asks for a **Twitch Client ID**, follow the steps on screen. It takes about a minute: register a free app at dev.twitch.tv with Client Type **Public** and OAuth Redirect URL `http://localhost`.
2. Click **Log in with Twitch** and approve the code in your browser.
3. Click **+ Add channel**. You can only add channels you moderate.

The default rule packs (parasocial, sexual content, harassment and others) start in **test mode**. They record what they *would* flag without filling your queue. After a stream, check the **Tuning** tab and switch on the ones that look right.

## Known limits

Predictions, the channel point reward queue, polls and hype trains aren't available. Twitch only gives them to the broadcaster's own login, not to moderators. See [What Twitch doesn't allow]({{REPO_URL}}/blob/{{TAG}}/docs/twitch-limitations.md) for the full list and the reasons.
