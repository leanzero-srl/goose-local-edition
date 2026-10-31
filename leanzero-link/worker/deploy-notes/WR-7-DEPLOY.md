# WR-7 deploy step (director, AFTER MiMo run e0dc60d4 is finished)

The live Link worker is the self-hosted Node server on the workhorse: launchd
`com.leanzero.link-auth` runs `run-node.sh` (tsx src/node-server.ts) from the MAIN
checkout `~/Projects/goose/leanzero-link/worker`. A restart drops in-flight Link calls,
so do it only when nothing uses the mesh control plane. No registry change is needed.

1. cd ~/Projects/goose && git fetch origin && git merge --ff-only origin/wk/wr7-mesh
   (if main moved: git merge origin/wk/wr7-mesh, then re-run the worker tests below)
2. cd leanzero-link/worker && npx tsc --noEmit && npx vitest run   (expect 303 passed)
3. launchctl kickstart -k gui/$(id -u)/com.leanzero.link-auth
4. curl -s http://127.0.0.1:8791/v1/health    (expect "ok":true)
5. Proof on a Studio: Providers > MLX > My Macs > Mesh > Retry connect joins while the
   three controller enrollments stay confirmed, as long as no phone node is in
   `~/.leanzero/bin/headscale -c ~/.leanzero/headscale/config.yaml nodes list`.
   A phone node present (online or offline) still gives 409 by design.

Rollback: git -C ~/Projects/goose revert <wr7 sha>, then step 3.
The desktop message part ships with the next DMG; it needs no worker change.
