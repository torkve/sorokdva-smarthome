# Sorokdva-smarthome

This idea of this project is to make an adapter of my own smarthome to Yandex.Alice dialog platform.

It consists of four main parts:

1. OAuth2 implementation for the account linking.
2. More or less full [Yandex.Alice platform protocol](https://yandex.ru/dev/dialogs/alice/doc/smart-home/about.html) implementation.
3. Web server (hand-rolled dispatcher on hyper), providing necessary
   authorization, routing and access to devices.
4. Some set of devices implementation. They are mostly the ones, that I have but can be used as base and/or
   inspiration for writing your own device adapters.

## Running

This server is intended to let you make your own secure smarthome platform.
No public access. No open access to MQTT or anything else.
No proprietary clouds from some faraway country.
You own your devices and give access to them only to a single specific service on your own choice.

To make your own smarthome platform you need:

1. Register your own skill on the [dialog platform](https://dialogs.yandex.ru/developer). Do not forget to make it private.

2. Next you need your own server (we're making private platform after all). You will need some https certificate (e.g. Letsencrypt).
   Choose subpath on the server, that'll host our platform, e.g. `/alice/`. Take a look at example config for nginx:

   ```nginx
   upstream alice {
       server 127.0.0.1:8888;
   }

   server {
       listen 443 default_server ssl http2;
       listen [::]:443 default_server ssl http2;
       port_in_redirect off;

       ssl_certificate /etc/letsencrypt/live/mydomain.com/fullchain.pem;
       ssl_certificate_key /etc/letsencrypt/live/mydomain.com/privkey.pem;
       # ... other default ssl and server params

       location /alice/ {
           proxy_pass http://alice;
           proxy_set_header Host $http_host;
           proxy_set_header X-Real-IP $remote_addr;
           proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
           proxy_set_header X-Forwarded-Host $server_name;
           proxy_set_header X-Forwarded-Proto $scheme;
           proxy_redirect off;
           proxy_buffering off;
       }
   }
   ```

   Here `127.0.0.1:8888` is the address of the interface platform will listen, and `mydomain.com` is the domain we use.

3. Bootstrap the app:

   ```sh
   git clone https://github.com/torkve/sorokdva-smarthome
   cd sorokdva-smarthome
   cargo build --release
   cp app.toml.example app.toml
   ```

4. Run app in debug mode:

   ```sh
   ./target/release/sorokdva-smarthome -p 8888 --prefix /alice/ --proxy \
       --db db.json \
       --debug
   ```

5. Debug mode allows us to register users. We will use this feature only once to register a user for Yandex.Alice platform.

   Navigate to `https://mydomain.com/alice/auth`

   Remember to replace `mydomain.com` with your domain, and `/alice` with your selected prefix.

   Register some user, e.g. with name `alice_user`.

   Login as this user on the same page. You will see the "Create client" button.

   According with the Alice platform, some fields must be fixed:

   * client URI: `https://dialogs.yandex.ru`
   * redirect URI: `https://social.yandex.net/broker/redirect`
   * allowed grant types: `authorization_code` and `refresh_token` (split by newline)
   * allowed response types: `code`

   You can define any space-separated scopes if you wish to separate some privileges in the future.

   Client name may be anything.

   Write down `client_id` and `client_secret` of the app created.

6. Edit file `app.toml` to configure devices you have. Take `app.toml.example` for inspiration.

7. Restart app without debug mode:

   ```sh
   ./target/release/sorokdva-smarthome -p 8888 --prefix /alice/ --proxy \
       --db db.json
   ```

8. Now you may edit you smarthome skill and add backend endpoint url to the skill: `https://mydomain.com/alice`.

9. Open account linking settings and set `client identifier` and `client secret` with the values you wrote in step 5.

   Set authorization url to `https://mydomain.com/alice/auth`

   Set token endpoint to `https://mydomain.com/alice/oauth/token`

   Set refresh token endpoint to `https://mydomain.com/alice/oauth/token`

   Set scopes to the scopes you selected while creating the app.

10. Add your skill in the Yandex app. It will require you to authorize with the login you created in the step 5, then will request oauth access.

11. After that you should see your devices in the Yandex app and Yandex.Alice will respond to your requests.

### I want easier way, why so many steps and no Docker?

Feel free to send PRs.

## Database

The handful of records the server keeps (users, the OAuth client, tokens)
lives in memory and is persisted as a single JSON file with an atomic
write on every mutation — sqlite is not used. If you come from the old
Python/sqlite deployment, convert the legacy database once (issued
OAuth tokens keep working, so the Yandex account link survives; browser
sessions ask for one re-login):

```sh
python3 scripts/migrate-db.py db.sqlite db.json
```

and pass `--db db.json` from then on. The server refuses to open a
sqlite file with a message pointing at the migration script.

## Deployment build

The deployment build is a **fully static** musl binary, so it runs on any
aarch64 Linux controller regardless of the libc there. One-time setup:

```sh
rustup toolchain install nightly            # build-std needs nightly
rustup target add aarch64-unknown-linux-musl
rustup component add rust-src
# prebuilt musl cross toolchain, no root needed:
mkdir -p ~/.local/opt && cd ~/.local/opt
curl -O https://musl.cc/aarch64-linux-musl-cross.tgz
tar xzf aarch64-linux-musl-cross.tgz
```

Then:

```sh
./build-static.sh
# -> target/aarch64-unknown-linux-musl/release/sorokdva-smarthome
```

The script rebuilds std with `-Z build-std` (size-optimized, immediate
panic-abort) and links with the bundled lld (identical-code folding); the
static stripped binary is about 1.6 MB and idles at about 4 MB RSS.

`./build-static.sh armv7-unknown-linux-musleabihf` builds the same for
32-bit armhf controllers (ARMv7, VFPv3 hard-float; toolchain
`armv7l-linux-musleabihf-cross` from musl.cc), and
`./build-static.sh x86_64-unknown-linux-musl` for the build host
(toolchain `x86_64-linux-musl-native`), which is handy for running it
locally before deploying.

## Notes

* The server itself never enforces https — it is expected to sit behind
  an https-terminating nginx; pass `--proxy` there so redirect URLs
  honour the `X-Forwarded-*` headers.
* `--debug` enables the extra routes (`/auth/register`,
  `/oauth/create-client`) and debug logging, including a dump of every
  request and response (`request` log target: headers with
  credential-bearing values redacted; query strings and bodies only for
  the `/v1.0/` API routes, never for the login/oauth forms). Without
  `--debug`, `RUST_LOG=info,request=debug` enables just that dump —
  the way to see what Yandex's endpoint validator sends.
* Access tokens live a year (Yandex refreshes the link only when the
  token expires, so a short lifetime leaves no slack for delays); the
  refresh token is not rotated and stays valid until the account is
  unlinked: revoking any token of a link retires the whole link, and a
  new link (code grant) supersedes the previous one for that user. The
  store prunes revoked and dead tokens and expired authorization codes,
  so `db.json` holds one live link per user and does not grow with use.
* Nothing the daemon keeps grows with uptime or bus traffic: `db.json`
  holds one live link (root token pair plus one refreshed token) per user
  and at most 16 pending authorization codes, every write is
  all-or-nothing (a failed write rolls the in-memory state back), the
  HTTP server handles at most 64 concurrent connections and drops one
  that sends no request headers within 30 seconds, the notification queue
  is capped, device commands waiting for an unreachable broker are
  capped at 64 (oldest dropped), and MQTT reconnects back off up to 60
  seconds. The one queue bounded by time rather than size is the state
  transitions waiting for a notification POST, at most its 30-second
  timeout. Per-message MQTT and poller traces are logged at `debug`
  only, so the `info` log volume stays proportional to state changes,
  not to bus traffic.
* The `[mqtt]` section accepts an optional `client_id` (default
  `sorokdva-dialogs-rs`); give every instance sharing a broker its own
  id, or they kick each other off (client-id takeover).
* The protocol layer is synced with the current Yandex Smart Home API:
  all device types (sensor.*, smart_meter.*, camera, ventilation.*,
  pet_feeder/pet_drinking_fountain, light and switch subtypes,
  openable.valve), all float property instances (meters, air quality
  densities, pressure, illumination, battery/food level, unitless
  `meter`), the `food_level` event and the extended `water_level`
  events, lighting color scenes in `color_setting`, and the
  `video_stream` capability for cameras (device logic can return the
  stream URL payload from an action). Mode/range/toggle instances and
  mode values are plain strings, so newer identifiers
  (`ventilation_mode`, `smart`, `wet_cleaning`, ...) need no code
  changes. Float property specifications include the `reportable` field,
  and declared color scenes are validated against the known scene ids at
  startup.
* Event transitions (PIR motion, water leak) are pushed to Yandex
  immediately when they happen, carrying the values observed at the
  edges, so a pulse shorter than the sampling period is not lost. A
  shared one-second send gate keeps the request rate bounded; the
  10-second sampling pass covers gradual changes (temperatures,
  brightness). A flapping source (a bouncing contact) cannot starve the
  sampler or grow memory: the transition queue is capped at 64 entries
  dropping the oldest, and a sampling pass deferred for a full extra
  period runs even while transitions are queued.
* `WbMixwhiteLight` never derives its state from a half-updated
  warm/cold channel pair: paired channel updates apply immediately, a
  lone one settles after a quiet period (`settle_ms` device option,
  default 200) with the partner channel at its last-known value.
  Commands additionally track their expected echoes — a lone echo of a
  two-channel command waits up to `5 * settle_ms` for its partner, a
  mismatching echo resolves that channel to the observed reality, and
  the restore-on-turn-on values are only ever latched from fully
  consistent confirmed pairs, so a transient half-applied mix is never
  reported.
* Tests (`cargo test`) cover the full HTTP surface (routing, sessions,
  OAuth flows, device actions) with golden assertions pinning the exact
  response bodies and headers.
