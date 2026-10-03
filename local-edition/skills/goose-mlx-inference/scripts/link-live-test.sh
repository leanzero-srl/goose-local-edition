#!/bin/zsh
# LIVE two-Mac test of goose's distributed engine driven over LeanZero Link (laptop = requester,
# workhorse = the node that serves rank 1). READ-ONLY on both Macs: it never starts, stops or
# signals anything, never touches the personal Tailscale (only `tailscale status`), port 8090 or
# the installed app's CDP port. The owner drives the app; this script proves what happened.
#
#   link-live-test.sh pre      before: personal identity snapshots, both Link daemons up, each sees the other
#   link-live-test.sh during   while the run is ready: the rank on the workhorse is a child of ITS goosed
#   link-live-test.sh post     after Stop: zero goose ranks on either Mac, personal identity unchanged
set -u
here=${0:A:h}
out=~/goose-builds/link-live-test; mkdir -p $out
link_sock='$HOME/.leanzero/tailscale/tailscaled.sock'
wh() { ssh workhorse "zsh -lc '$1'"; }
fail=0; bad() { print -r -- "FAIL: $*"; fail=1; }

identity() {  # $1 = mac|wh
  if [[ $1 == mac ]]; then tailscale status --json | python3 $here/tailscale_identity.py
  else scp -q $here/tailscale_identity.py workhorse:/tmp/tailscale_identity.py &&
       wh "tailscale status --json | python3 /tmp/tailscale_identity.py"; fi
}
link_status() {  # the LeanZero Link daemon's OWN socket, never the personal one
  if [[ $1 == mac ]]; then tailscale --socket "${link_sock/\$HOME/$HOME}" status --json
  else wh "tailscale --socket $link_sock status --json"; fi
}
ranks() {  # pid ppid command of every goose rank
  if [[ $1 == mac ]]; then ps -axo pid=,ppid=,command= | grep goose-distributed-rank | grep -v grep
  else wh "ps -axo pid=,ppid=,command= | grep goose-distributed-rank | grep -v grep"; fi
}

case ${1:-} in
pre)
  for m in mac wh; do
    identity $m > $out/personal_$m.before.json || bad "$m: personal tailscale status unreadable"
    link_status $m > $out/link_$m.json 2>/dev/null || bad "$m: no LeanZero Link daemon answers on its own socket — sign in and Connect in the app first"
  done
  python3 - $out <<'PY' || fail=1
import json, sys
out = sys.argv[1]
for me, other in (("mac", "wh"), ("wh", "mac")):
    try:
        d = json.load(open(f"{out}/link_{me}.json"))
        o = json.load(open(f"{out}/link_{other}.json"))
    except Exception as e:
        print(f"FAIL: {me}: Link status unreadable ({e})"); sys.exit(1)
    if d.get("BackendState") != "Running":
        print(f"FAIL: {me}: Link is {d.get('BackendState')}"); sys.exit(1)
    want = (o.get("Self") or {}).get("ID")
    peer = [p for p in (d.get("Peer") or {}).values() if p.get("ID") == want]
    if not peer or not peer[0].get("Online"):
        print(f"FAIL: {me}: the other Mac is not an ONLINE Link peer (same account signed in on both?)"); sys.exit(1)
    print(f"ok: {me} sees {peer[0].get('HostName')} online on Link at {peer[0].get('TailscaleIPs')}")
PY
  for m in mac wh; do [[ -n "$(ranks $m)" ]] && bad "$m: a goose rank already runs: $(ranks $m)"; done
  ;;
during)
  r=$(ranks wh); print -r -- "workhorse ranks: $r"
  [[ -z $r ]] && bad "no goose rank runs on the workhorse"
  for ppid in ${(f)"$(print -r -- $r | awk '{print $2}')"}; do
    parent=$(wh "ps -o command= -p $ppid")
    print -r -- "  parent $ppid: $parent"
    [[ $parent == *goose* && $parent != *sshd* ]] || bad "rank's parent is not the workhorse's own goosed: $parent"
  done
  ssh_ranks=$(ps -axo command= | grep -E '^/usr/bin/ssh .*goose-distributed-rank' | grep -v grep)
  [[ -n $ssh_ranks ]] && bad "a rank was started over ssh, not Link: $ssh_ranks"
  print -r -- "laptop ranks: $(ranks mac)"
  ;;
post)
  for m in mac wh; do
    [[ -n "$(ranks $m)" ]] && bad "$m: a goose rank survived Stop: $(ranks $m)"
    identity $m > $out/personal_$m.after.json
    if diff -q $out/personal_$m.before.json $out/personal_$m.after.json >/dev/null; then
      print "ok: $m personal Tailscale identity unchanged"
    else bad "$m: personal Tailscale identity CHANGED"; diff $out/personal_$m.before.json $out/personal_$m.after.json; fi
  done
  ;;
*) print "usage: $0 pre|during|post"; exit 2 ;;
esac
(( fail )) && { print "RESULT: FAIL"; exit 1; }
print "RESULT: ok"
