# goose distributed rank: the group's formation handshake (Q-136), run by every rank of a JACCL
# launch right after `mx.distributed.init` and before any other collective. Concatenated after
# rank_env.py (`mx`, `emit`), before the rank program (both kinds).
#
# Why. jaccl pairs messages by ORDER alone (unreliable-connected queue pairs, no sequence of its
# own, a completion's status never read), and on the Thunderbolt RDMA driver the first message one
# way over a fresh connection can go nowhere while its sender sees it complete. Measured 2026-09-26
# with a verbs logger on both ranks: rank 1's first send completed in 8 µs, its data never reached
# rank 0's posted receive, and its SECOND send waited ~8.7 s before the transport took it — it
# filled rank 0's first receive at 10.0 s; every later message arrived, in order. Every group gets
# the same queue pair numbers (2336 ↔ 2320) and jaccl's fixed starting packet number, so nothing
# tells one connection from the last. From that point rank 0's k-th collective paired with rank
# 1's (k+1)-th: the doorbell's port read 0x3F800000 + port (the peer's post-load barrier, float32
# 1.0) and rank 0 died at its seed all_sum (2 of 8 starts after an abnormal end on 3.0.49). Bare
# pairs forming this way lost one first message in 13 of 30 clean starts (12 of them rank 1's);
# the message after it arrived every time.
#
# What. Rank 0 leads: in round 1 it sends its part to every worker, which only receives — of 33
# losses measured, 31 were the worker's (the Studio's) first send, made before it had received
# anything on the connection, and bare pairs whose rank 0 sent first while the worker only
# received lost no first message in 40 starts. Rounds 2.. are lockstep all_gathers — the pattern of jaccl's
# own collectives, every receive posted before its send, one message each way per round. Each
# rank's part is [nonce, round, rank, check]. A part naming a LATER round than the one read means
# the peer's messages before it went nowhere; that rank then sends its next round(s) to the peer
# point-to-point without receiving — the peer waits in its own round with its receive posted —
# until the two are in step. Never a stream of point-to-point sends the receiver does not pace: 33
# in a row lost or reordered messages on the same link (19 of 33 once, and the leftovers shifted
# the collectives after them). With two ranks a lost message is absorbed when the one after it
# arrives; with more, an all_gather cannot skip one peer's receive, so a loss ends the rank in
# words. Anything that is not this launch's part — another launch's nonce, the wrong rank, a check
# that does not hold (a later collective's bytes over a formation buffer), a round out of order —
# ends the rank in words: never read as data.
#
# Each round is announced (GOOSE_RANK_FORMING) before the rank waits in it: two ranks that both
# lost their message in the same round (never seen) wait for each other, and goosed reads that
# standstill — no round advancing on a forming rank — as the start's stall, not the spinning CPU.
#
# Only a launch whose spec carries `formation` runs it (the program tag makes an older peer's
# goosed refuse such a spec before it would wait here forever, launch.rs).


class FormationRefused(SystemExit):
    pass


def formation_part(nonce, index, rank):
    check = (nonce ^ (index * 40503) ^ ((rank + 1) * 9973) ^ 0x2B5A5A5) & 0x7FFFFFFF
    return [nonce, index, rank, check]


def formation_read(parts_by_peer, nonce, index, rounds, rank, size, read, lost):
    """Takes each peer's part read in round `index`: a LATER round's means the peer's messages
    before it went nowhere (counted in `lost`, the peer then `read` ahead)."""
    for peer, part in parts_by_peer.items():
        got = part[1]
        if not (index <= got <= rounds and part == formation_part(nonce, got, peer)):
            raise FormationRefused(
                f"goose rank: group formation: rank {rank} read {part} from rank {peer} in "
                f"round {index} of {rounds}, not this launch's part (nonce {nonce}); the "
                "connection carries another launch's or a later collective's message"
            )
        if got > index:
            if size > 2:
                raise FormationRefused(
                    f"goose rank: group formation: {got - index} message(s) from rank {peer} "
                    f"to rank {rank} went nowhere in round {index}; a group of {size} cannot "
                    "skip one peer's receive to get back in step"
                )
            lost[peer] += got - index
        read[peer] = got


def form_group(group, formation):
    rank, size = group.rank(), group.size()
    nonce, rounds = int(formation["nonce"]), int(formation["rounds"])
    peers = [peer for peer in range(size) if peer != rank]
    read = {peer: 0 for peer in peers}
    lost = {peer: 0 for peer in peers}
    for index in range(1, rounds + 1 if peers else 1):
        mine = mx.array(formation_part(nonce, index, rank), dtype=mx.int32)
        early = [peer for peer in peers if read[peer] >= index]
        if early:
            # Two ranks only (a loss among more is refused): the peer's part for this round is
            # already read, and the peer waits in this round for ours.
            mx.eval(mx.distributed.send(mine, early[0], stream=mx.cpu))
            continue
        emit("RANK_FORMING", {"rank": rank, "round": index})
        if index == 1 and rank == 0:
            # Rank 0 leads: no worker sends before it has received (see above).
            for peer in peers:
                mx.eval(mx.distributed.send(mine, peer, stream=mx.cpu))
            continue
        if index == 1:
            part = mx.distributed.recv((4,), mx.int32, 0, stream=mx.cpu)
            mx.eval(part)
            parts_by_peer = {0: [int(value) for value in part.tolist()]}
        else:
            gathered = mx.distributed.all_gather(mine, stream=mx.cpu)
            mx.eval(gathered)
            parts = [int(value) for value in gathered.tolist()]
            parts_by_peer = {peer: parts[4 * peer : 4 * peer + 4] for peer in peers}
        formation_read(parts_by_peer, nonce, index, rounds, rank, size, read, lost)
    emit(
        "RANK_FORMATION",
        {"rank": rank, "rounds": rounds, "lost": {str(peer): n for peer, n in lost.items()}},
    )
