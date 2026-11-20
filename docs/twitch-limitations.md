# What Twitch doesn't let a moderator tool do (and why)

Twitch Mod Pro signs in with **your** Twitch account, the moderator's, not the broadcaster's. Twitch decides what each kind of account can see and change through its API. Some Mod View panels rely on access that Twitch only gives to the broadcaster's own login, so a mod tool can't offer them, no matter how it's built.

This page lists what's missing, the reason, and what the app does instead.

## Not available to moderators

| Mod View feature | Why the app can't show it |
|---|---|
| **Predictions** | Twitch's Predictions API and prediction events need the `channel:read:predictions` / `channel:manage:predictions` permissions. Twitch only grants those to the **broadcaster's** token: the API requires the logged-in user to *be* the broadcaster. Mod View can show predictions because it's Twitch's own site, not a third-party app. |
| **Channel point reward requests (redemption queue)** | Reading redemptions needs `channel:read:redemptions`, which is broadcaster-only. Even with the broadcaster's login, Twitch only lets an app fulfill or refund redemptions for rewards **that same app created**. Rewards made on the Twitch dashboard can't be managed by any third-party app. |
| **Polls** | Same as predictions: `channel:read:polls` / `channel:manage:polls` are broadcaster-only. |
| **Hype trains** | Hype train events need `channel:read:hype_train`, which is broadcaster-only. |
| **Subscriber list, bits leaderboard, goals, ad schedule** | Each needs a broadcaster-only permission (`channel:read:subscriptions`, `bits:read`, `channel:read:goals`, `channel:read:ads`). |

For these, the Activity tab says so directly, so nobody goes looking for them.

## Available, with limits

| Feature | Limit | Reason |
|---|---|---|
| **Shoutouts** | Only while the channel is **live**; at most **one every 2 minutes** per channel, and the **same streamer once an hour**. | These are Twitch's rules for `/shoutout`, and they apply to Mod View too. The app shows Twitch's error when a shoutout is refused. |
| **Shoutout chat message** (`!so @{raider}`) | Sent **as you**, the moderator, not as the channel's bot. Subject to the channel's chat settings (slow mode, followers-only, etc.). | It uses Twitch's Send Chat Message API with your login (`user:write:chat`). |
| **Mod log** | Starts recording when the app is running and you're logged in. **No history** from before that. Kept for 30 days. | Twitch sends mod actions as live events (`channel.moderate`). There's no API to read a channel's past moderation history. It also needs several read-only permissions (`moderator:read:blocked_terms`, `chat_settings`, `moderators`, `vips`, and others), which the app asks for at login. |
| **Activity feed** | Follows, subs, resubs, gift subs, bits and announcements only, and only while the app is running. | Subs, gifts, bits and announcements come from chat, which any account can read. Follows come from Twitch's `channel.follow` event with `moderator:read:followers`. Everything else in Mod View's activity feed is broadcaster-only (see above). |
| **Twitch inbox and mod log channel count** | About 40 channels per running app. | Twitch allows 300 event subscriptions per connection, and the app uses 7 per channel. Past that, the extra channels still get chat monitoring, flags and raids. Only the inbox, mod log and follow events stop. |
| **Bans, timeouts, deletes, warns, AutoMod, unban requests** | Only in channels where you're a moderator. | Twitch checks this on every request. The app also only lets you add channels you moderate. |

## Permissions the app asks for

All are moderator-level. None of them give access to the broadcaster's account.

- `user:read:moderated_channels`: list the channels you moderate (to restrict which can be added)
- `moderator:manage:chat_messages`, `moderator:manage:banned_users`, `moderator:manage:warnings`: delete, timeout, ban, unban, warn
- `moderator:manage:automod`, `moderator:manage:unban_requests`, `moderator:read:suspicious_users`: the Twitch inbox
- `moderator:manage:shoutouts`, `user:write:chat`: shoutouts and the optional shoutout chat message
- `moderator:read:followers`: follows in the activity feed
- `moderator:read:blocked_terms`, `moderator:read:chat_settings`, `moderator:read:moderators`, `moderator:read:vips`: required by Twitch for the mod log (`channel.moderate`)

When a new version adds a permission, the app asks you to log in once more.

## References

Twitch's API reference lists the required permission ("Authorization") for every endpoint and event:

- API endpoints: <https://dev.twitch.tv/docs/api/reference/>
- EventSub events and their permissions: <https://dev.twitch.tv/docs/eventsub/eventsub-subscription-types/>
- Permission (scope) list: <https://dev.twitch.tv/docs/authentication/scopes/>
