# Waveform deployment on redstonefun

Read-only inspection of SSH alias `urmom` on 27 October 2026. No server files,
containers, DNS records or services were changed.

Local preparation is implemented in `docker-compose-prod.yml`: the separate
`waveform-viewer` service builds the existing frontend Dockerfile, publishes only
`127.0.0.1:8092`, and reaches `mroww:8091` through the internal `backend` network.
The Rust API has no host port mapping. No host nginx configuration has been
modified; public routing remains a later step requiring explicit permission.
`docker compose -f docker-compose-prod.yml config --quiet` passes. The resolved
configuration was also checked for the loopback-only frontend port, shared
internal network and absence of a published Rust API port. These checks did not
start containers or modify the remote host.

## Recommendation

Publish the viewer at **https://redstonefun.pl/waveform/**. This reuses the
existing domain, HTTPS certificate and host nginx. A separate frontend container
serves the React build and proxies the waveform API to `mroww:8091` over the
private Docker network. Do not publish the Rust API port to the Internet.

The browser uses the existing origin and absolute API paths under
`/api/v1/plots/`. Only that prefix is routed to the viewer container; the site's
other `/api/` endpoints continue to reach the current application. No CORS or
Minecraft login is needed for these public reads.

## What is running

| Component | Observed configuration |
| --- | --- |
| Host proxy | systemd `nginx.service`, `/usr/bin/nginx`, nginx 1.30.4. |
| Main nginx config | `/etc/nginx/nginx.conf`; explicitly includes `/etc/nginx/hosts/redstonefun.pl.conf` and other named host files. There is no `sites-enabled` directory. |
| Main website | HTTPS vhost for `redstonefun.pl` and `www.redstonefun.pl`; `location /` proxies to `127.0.0.1:5000`. HTTP redirects to HTTPS. |
| Website container | `redstonefun-web`, Compose project `redstonefun_site`, source `/srv/redstonefun_site/docker-compose.yml`, loopback port `127.0.0.1:5000:5000`. |
| Website application | Flask `/srv/redstonefun_site/app.py`; current API routes include `/api/videos`, `/api/reditor/...`, `/api/redlang/compile`. No waveform or `/api/v1/plots/` routes were found. |
| Other website routing | `/map/` has its own upstream. `api.redstonefun.pl` is an existing separate PHP/upload API vhost; do not replace it. |
| Minecraft backend | Container `mroww`, image `mroww:latest`, published Minecraft port `25578:25565`. No waveform API port is published. |
| Backend networks | `mchprs_backend` is internal; `mroww` is also attached to external `redstonefun-network`. Its Docker DNS alias is `mroww`. |
| Current backend config | Bind mount `/srv/mchprs/backend-config` to `/run/config`, with `MCHPRS_CONFIG=/run/config/Config.toml`. The inspected config has no waveform entries yet. |
| Existing TLS | Main vhost uses the existing redstonefun certificate lineage under `/etc/letsencrypt/live/redstonefun.pl-0004/`. Its keys were not read. |

Ports 8091 and 8092 had no listening TCP sockets during inspection. Recheck before
deployment. The SSH account has Docker access, but no passwordless sudo; changing
root-owned nginx configuration and reloading nginx requires the server operator.

The running `mroww` container's Compose labels point to this repository's
`docker-compose-prod.yml` from the Windows checkout. The server also contains
`/srv/mchprs/deployment/docker-compose.yml`, but it describes an older `mchprs` /
Velocity deployment and different images/ports. **Do not use that stale file to
recreate the running backend.** Use the current repository deployment definition.

## Prepared containers and later routing

### 1. Backend configuration

Deploy the new Rust build and set these top-level values in
`/srv/mchprs/backend-config/Config.toml`:

```toml
waveform_enabled = true
waveform_bind_address = "0.0.0.0:8091"
waveform_public_url = "https://redstonefun.pl/waveform/"
```

The wildcard bind is needed for access from the frontend container. Omit any
host `ports:` mapping for 8091. The API remains reachable by containers sharing
the backend's networks; it is intentionally public data once proxied. Configure
the documented waveform command and numeric permissions before enabling recording.

### 2. Separate frontend container

This service is now in the current repository's `docker-compose-prod.yml`. It joins
the existing internal `backend` network for API traffic and `redstonefun-network`
for the loopback port publication. Docker does not activate published ports on a
container attached only to internal networks. No changes to the website's Flask app or
its Compose project are needed.

```yaml
services:
  waveform-viewer:
    image: mroww-waveform-viewer:latest
    build:
      context: ./waveform-viewer
    restart: unless-stopped
    environment:
      WAVEFORM_BACKEND: mroww:8091
    ports:
      - "127.0.0.1:8092:8080"
    networks:
      - backend
      - redstonefun
```

The existing `backend` network definition and `mroww` service stay in the same
Compose project. The frontend Dockerfile runs its own nginx on 8080, serves
relative static asset URLs and forwards `/api/` to `WAVEFORM_BACKEND`. Its proxy
already disables response buffering for SSE. Build/start only this new service
after deploying/enabling the Rust backend; starting it does not require restarting
the main website.

### 3. Existing host HTTPS vhost

This is a proposal for later review, not an authorized deployment step. Once the
backend and container connection work, obtain explicit permission before changing
the host nginx configuration or reloading it.

The proposed locations belong **inside the existing HTTPS server block** in
`/etc/nginx/hosts/redstonefun.pl.conf`. Keep the current `location /`, other
locations and TLS settings.

```nginx
location = /waveform {
    return 308 /waveform/$is_args$args;
}

location ^~ /waveform/ {
    # Trailing slash strips /waveform/ before forwarding static asset requests.
    proxy_pass http://127.0.0.1:8092/;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}

location ^~ /api/v1/plots/ {
    # No trailing URI: preserve the API path expected by Rust.
    proxy_pass http://127.0.0.1:8092;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header Connection "";
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_buffering off;
    proxy_cache off;
    proxy_read_timeout 30s;
    send_timeout 5s;
    gzip off;
}
```

Both proxy layers must allow the 250 ms SSE batches through immediately. The
30-second upstream idle timeout exceeds the Rust stream's 15-second heartbeat.
Do not add WebSocket upgrade headers: the live transport is ordinary HTTP SSE.
The exact `/waveform` redirect preserves coordinate query parameters.

Before exposing public traffic, apply appropriate request/connection limits at
the host proxy using its existing operational policy. Limit API requests without
counting each SSE event as a new request. Rust also bounds connections, requests,
page sizes, work memory and blocked stream writes.

## Operator verification and rollback

1. Validate the intended Compose definition with `docker compose config` without
   printing secrets from unrelated services. Build the frontend image and deploy
   the Rust backend in the normal maintenance window.
2. Check the loopback frontend: `curl -I http://127.0.0.1:8092/`. Query an unloaded
   plot's API path through it and expect JSON `404 plot_not_loaded`, not an SPA
   HTML page or a connection failure.
3. Have an editor in a loaded interpreter plot run `/waveform on`, add a dust
   probe, and execute a command-block `/say` or `/tellraw`. Confirm snapshot
   responses contain both binary changes and plain text records.
4. Run privileged `nginx -t` against the real host configuration, then reload
   nginx using the normal operator workflow. No reload was performed by inspection.
5. Open `/waveform/?x=3&y=-2`. Verify assets load under that path, the selected
   loaded plot connects, SSE batches arrive without accumulating in proxy buffers,
   and command output dots/previews/expanded rows work. An unloaded plot must
   remain unloaded; the browser must report that state.
6. Confirm existing home page, `/api/videos`, Reditor and `/map/` still route as
   before. Closing/switching the viewer should close the old stream.

Rollback removes only the three host locations and reloads a validated nginx
configuration, then stops the frontend service. Disable the Rust feature with
`waveform_enabled = false` on the next backend restart if needed. No new DNS
record or certificate needs rollback. In-memory captures are lost on backend
restart, `/waveform off`, `/waveform clear`, or `/waveform reset confirm`.

## Alternative: a subdomain

`waveform.redstonefun.pl` can point to the same container using a dedicated vhost;
proxy both `/` and `/api/v1/plots/` at that origin. That avoids sharing the main
site's URL namespace, but adds DNS and certificate issuance/renewal work. Neither
its DNS nor certificate coverage was verified during this inspection. Use the
path route initially unless a separate origin is an explicit product preference.

The proposed live configuration was not applied or validated with host `nginx -t`;
the existing site, DNS and TLS were left running unchanged.
